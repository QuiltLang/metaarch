//! Phase-4d integration: arch hosts expand — as identity, because arch is a
//! data language (quote plugs demote to plain tuples at parse, so quilt's
//! expander has nothing staged to evaluate).

use metaarch_expand::{arch_multi, expand_stem, parse_stem};
use quilt::prelude::STerm;

#[test]
fn hello_arch_quilt_expands_to_itself() {
    let src = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../examples/hello.arch.quilt"
    ))
    .unwrap();
    let mut multi = arch_multi();
    let (chain, parsed) = parse_stem(&mut multi, "hello.arch", &src).unwrap();
    assert_eq!(chain, ["arch"]);
    let expanded = multi.expand_lang(chain[0], &parsed).unwrap();
    // Expansion is identity over the parsed term. (The parsed term's coparse
    // is not byte-identical to the *source* for block-opened quote bodies —
    // quilt reindents the closing bracket — but it is the same architecture,
    // fragment brackets included.)
    assert_eq!(expanded.coparse(), parsed.coparse());
    assert!(expanded.coparse().contains("rust↖"));
    assert!(expanded.coparse().contains('↗'));
}

#[test]
fn inline_fragments_round_trip_byte_for_byte() {
    // With an inline quote body there is no reindent, so the whole file —
    // fragment brackets included — coparses back byte-identically through
    // parse + expand.
    let src = "system s\n\nservice a {\n  lang rust\n  impl get /x rust↖ \"x\" ↗\n}\n";
    let mut multi = arch_multi();
    let (_, expanded) = expand_stem(&mut multi, "s.arch", src).unwrap();
    assert_eq!(expanded.coparse(), src);
}

#[test]
fn ground_unquotes_are_rejected() {
    // arch has no ground values: a splice in a bare arch host cannot be
    // filled and is rejected at parse (quilt's unquote-depth check).
    let mut multi = arch_multi();
    assert!(parse_stem(&mut multi, "x.arch", "system ↙name↘\n").is_err());
}
