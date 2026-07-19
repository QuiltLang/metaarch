//! The workspace's own quilt expander, with `arch` registered as a
//! *dynamic* language.
//!
//! quilt's stock CLI drives the closed built-in set (`Omni`); its dynamic
//! registry (`DictMulti`, quilt's `Box<dyn Language>` hook) is a library
//! API. This crate is the ~60 lines that turn that hook into a working
//! expander: the built-in languages via `dict_omni_language()`, plus
//! [`metaarch_lang::ArchLanguage`] under the name `arch` — no quilt changes,
//! no fork. `bin/expand` runs the binary over every `.quilt` source.

use quilt::langs::omni::dict_omni_language;
use quilt::multi::DictMulti;
use quilt::prelude::{bx, miette, Result, STerm};

/// The built-in language set plus dynamic `arch` — the [`Language`] for
/// parsing and, since phase 4d, the [`MetaLanguage`] that lets
/// `expand_lang("arch", …)` run. arch stages no computation (quote plugs are
/// demoted to plain tuples at parse), so expanding an arch host is an
/// identity rebuild and the meta's hooks exist to explain themselves if a
/// staged construct ever reaches them.
///
/// [`Language`]: quilt::lang::Language
/// [`MetaLanguage`]: quilt::meta::MetaLanguage
pub fn arch_multi() -> DictMulti {
    let mut multi = dict_omni_language();
    multi.add_lang(metaarch_lang::LANG, bx(metaarch_lang::ArchLanguage));
    multi.add_meta(metaarch_lang::LANG, bx(metaarch_lang::ArchMetaLanguage));
    multi
}

/// Derive the language chain from a `.quilt` file's stem (name with the
/// `.quilt` suffix stripped): peel extensions right-to-left while each names
/// a registered language — the rightmost is the host, the rest are defaults
/// for nested un-annotated quotes. Mirrors quilt's own CLI, but consults the
/// dynamic registry, so `*.arch.quilt` resolves to `["arch"]`.
pub fn lang_chain<'a>(multi: &DictMulti, stem: &'a str) -> Vec<&'a str> {
    let parts: Vec<&str> = stem.split('.').collect();
    let mut chain: Vec<&str> = parts[1..]
        .iter()
        .rev()
        .copied()
        .take_while(|part| multi.get_lang(part).is_ok())
        .collect();
    if chain.is_empty() {
        chain.push(parts.last().copied().unwrap_or(""));
    }
    chain
}

/// Parse a `.quilt` source with the chain derived from `stem`, returning the
/// chain and the parsed term.
pub fn parse_stem<'a>(
    multi: &mut DictMulti,
    stem: &'a str,
    input: &str,
) -> Result<(Vec<&'a str>, std::sync::Arc<quilt::prelude::QTerm>)> {
    let chain = lang_chain(multi, stem);
    let term = multi.parse_chain(&chain, input)?;
    Ok((chain, term))
}

/// Parse and expand a `.quilt` source. For `arch` hosts expansion is an
/// identity rebuild (arch is data — see [`arch_multi`]), so the expanded term
/// coparses to the plain `.arch` file the source spells; for every other host
/// it is quilt's ordinary staged expansion.
pub fn expand_stem<'a>(
    multi: &mut DictMulti,
    stem: &'a str,
    input: &str,
) -> Result<(Vec<&'a str>, std::sync::Arc<quilt::prelude::QTerm>)> {
    let (chain, term) = parse_stem(multi, stem, input)?;
    let expanded = multi.expand_lang(chain[0], &term)?;
    Ok((chain, expanded))
}

/// Registry load for the metaarch CLI (phase 4d convergence): parse `input`
/// as an arch host — for a `.quilt` file name the chain comes from its stem,
/// a plain `.arch` file is the single-language chain — expand (identity for
/// arch), and return the plain `.arch` text the expanded term coparses to.
/// The CLI derives the `SystemSpec` from that text, so both the plain and
/// the `.arch.quilt` generate paths flow through the same registry.
pub fn arch_text(file_name: &str, input: &str) -> Result<String> {
    let mut multi = arch_multi();
    if let Some(stem) = file_name.strip_suffix(".quilt") {
        let (chain, expanded) = expand_stem(&mut multi, stem, input)?;
        if chain[0] != metaarch_lang::LANG {
            return Err(miette!(
                "{file_name}: expected an arch host, found {:?}",
                chain[0]
            ));
        }
        Ok(expanded.coparse())
    } else {
        let term = multi.parse_chain(&[metaarch_lang::LANG], input)?;
        Ok(multi.expand_lang(metaarch_lang::LANG, &term)?.coparse())
    }
}
