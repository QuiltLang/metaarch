//! End-to-end tests of arch as a dynamically registered quilt language:
//! `.arch.quilt` sources parse through the same `DictMulti` as the built-in
//! set, and host metaprograms can quote (and splice into) arch.

use metaarch_expand::{arch_multi, lang_chain};
use quilt::prelude::STerm;

#[test]
fn arch_resolves_in_the_chain() {
    let multi = arch_multi();
    assert_eq!(lang_chain(&multi, "shop.arch"), vec!["arch"]);
    assert_eq!(lang_chain(&multi, "rust_service.rs"), vec!["rs"]);
    // rust as the default embedded language for un-annotated quotes.
    assert_eq!(lang_chain(&multi, "shop.rs.arch"), vec!["arch", "rs"]);
}

#[test]
fn arch_quilt_files_parse_with_embedded_rust_quotes() {
    let src = "system hello\n\nservice greeter {\n  lang rust\n  port 8080\n  impl get /hello rust\u{2196}\n    \"hello from a parsed quote!\\n\"\n  \u{2197}\n}\n";
    let mut multi = arch_multi();
    let term = multi.parse_chain(&["arch"], src).unwrap();
    // quilt normalizes the closing `↗`'s indentation, so coparse is not
    // byte-identical to the source — but it must be a fixpoint: reparsing
    // the coparse reproduces it exactly, and the quote body survives.
    let out = term.coparse();
    assert!(out.contains("rust\u{2196}"), "{out}");
    assert!(out.contains("hello from a parsed quote!"), "{out}");
    let again = arch_multi().parse_chain(&["arch"], &out).unwrap();
    assert_eq!(again.coparse(), out);
}

#[test]
fn arch_rejects_broken_sources_at_parse_time() {
    let mut multi = arch_multi();
    let err = multi
        .parse_chain(&["arch"], "system s\nservice a {\n speed 9\n}")
        .unwrap_err();
    assert!(err.to_string().contains("unknown service entry"), "{err}");
}

#[test]
fn rust_hosts_can_quote_and_splice_arch() {
    // A term splice (`↙name↘`): the ground variable holds an already-built
    // QTerm. (`.↑` lifts into arch are not possible yet — quilt's rust meta
    // keys lift spellings statically per target, so a dynamic language has
    // none; phase 4c revisits this.)
    let src = "fn spec() {\n    let t = arch\u{2196}system \u{2199}name\u{2198} service s { lang rust }\u{2197};\n}\n";
    let mut multi = arch_multi();
    let term = multi.parse_chain(&["rs"], src).unwrap();
    let expanded = multi.expand_lang("rs", &term).unwrap();
    let code = expanded.coparse();
    // The quote expands to Rust that rebuilds the arch term: its production
    // tags and the ground token writes must appear in the generated code.
    assert!(code.contains("arch_file"), "{code}");
    assert!(code.contains("system"), "{code}");
    assert!(code.contains("name"), "{code}");
}
