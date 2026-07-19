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
use quilt::prelude::{bx, Result};

/// The built-in language set plus dynamic `arch`.
pub fn arch_multi() -> DictMulti {
    let mut multi = dict_omni_language();
    multi.add_lang(metaarch_lang::LANG, bx(metaarch_lang::ArchLanguage));
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
