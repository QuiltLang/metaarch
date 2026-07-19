//! Rust binding for the arch tree-sitter grammar.
//!
//! `src/parser.c` is **generated** from `grammar.js` — never edit it by hand;
//! regenerate with `bin/grammar` (which also runs the corpus tests). The
//! binding mirrors the shape of quilt's vendored `quilt::grammars` modules,
//! so callers use it identically:
//! `parser.set_language(&tree_sitter_arch::LANGUAGE.into())`.

use tree_sitter_language::LanguageFn;

unsafe extern "C" {
    fn tree_sitter_arch() -> *const ();
}

/// The tree-sitter [`LanguageFn`] for the arch grammar.
pub const LANGUAGE: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_arch) };

/// The grammar's highlight query (`queries/highlights.scm`), nvim-flavored:
/// specific patterns first.
pub const HIGHLIGHTS_QUERY: &str = include_str!("../../queries/highlights.scm");

/// The grammar's injection query (`queries/injections.scm`): annotated
/// `impl` fragment bodies inject their fragment language.
pub const INJECTIONS_QUERY: &str = include_str!("../../queries/injections.scm");

#[cfg(test)]
mod tests {
    fn parse(src: &str) -> tree_sitter::Tree {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&super::LANGUAGE.into())
            .expect("grammar loads");
        parser.parse(src, None).expect("parse returns a tree")
    }

    #[test]
    fn examples_parse_without_errors() {
        for name in ["shop.arch", "hello.arch"] {
            let src = std::fs::read_to_string(format!("../examples/{name}")).expect(name);
            let tree = parse(&src);
            assert!(!tree.root_node().has_error(), "{name} has parse errors");
            assert_eq!(tree.root_node().kind(), "source_file");
        }
    }

    #[test]
    fn queries_compile() {
        let language: tree_sitter::Language = super::LANGUAGE.into();
        tree_sitter::Query::new(&language, super::HIGHLIGHTS_QUERY).expect("highlights compile");
        tree_sitter::Query::new(&language, super::INJECTIONS_QUERY).expect("injections compile");
    }

    #[test]
    fn malformed_input_yields_localized_errors() {
        let tree = parse("system s\nservice x {\n  lang rust\n  port notanumber\n}\n");
        assert!(tree.root_node().has_error());
        // The service name and lang entry survive around the ERROR node.
        let text = tree.root_node().to_sexp();
        assert!(text.contains("lang_entry"), "recovery kept: {text}");
    }
}
