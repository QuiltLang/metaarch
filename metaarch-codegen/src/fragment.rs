//! `impl` fragments parsed with the real grammars (phase 4c).
//!
//! A fragment travels through the spec as the dedented string the `.arch`
//! parser carried (the spec stays quilt-free), but it stops being opaque
//! here: [`term`] parses it with the tree-sitter grammar of the service's
//! language — the same `Language` implementations quilt's own quotes use —
//! and the service generators splice the resulting term into the generated
//! handler instead of appending text. [`check_fragments`] runs the same
//! parse up front so `metaarch check` rejects a malformed fragment with a
//! positioned diagnostic before any code is generated.

use std::sync::Arc;

use metaarch_spec::validate::{Diagnostic, Severity};
use metaarch_spec::{Lang, SystemSpec};
use quilt::lang::{flat_nodes, Language};
use quilt::langs::python::lang::PythonLanguage;
use quilt::langs::rust::lang::RustLanguage;
use quilt::prelude::QTerm;

/// Parse a fragment body as a term of `lang`. Rust fragments are the tail
/// *expression* of the generated handler; Python fragments are the handler's
/// statement suite (one or more statements, typically ending in `return`).
pub(crate) fn term(lang: Lang, body: &str) -> Result<Arc<QTerm>, String> {
    let nodes = flat_nodes(body);
    let parsed = match lang {
        Lang::Rust => RustLanguage::default().parse_expr(&nodes),
        Lang::Python => PythonLanguage::default().parse(&nodes),
    };
    parsed.map_err(|e| e.to_string())
}

/// Parse every `impl` fragment in the spec, reporting failures as
/// `Error`-severity diagnostics at the route's span. Run this beside
/// `metaarch_spec::validate` before calling [`generate`](crate::generate):
/// the generators expect every fragment to parse.
pub fn check_fragments(spec: &SystemSpec) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    for service in &spec.services {
        let Some(lang) = service.lang else { continue };
        for route in &service.impls {
            if let Err(e) = term(lang, &route.body) {
                diags.push(Diagnostic {
                    severity: Severity::Error,
                    span: route.span,
                    message: format!(
                        "impl route `{} {}` fragment does not parse as {lang}: {e}",
                        route.method.upper(),
                        route.path
                    ),
                });
            }
        }
    }
    diags
}

#[cfg(test)]
mod tests {
    use super::*;
    use quilt::prelude::STerm;

    #[test]
    fn parses_a_rust_expression() {
        let term = term(Lang::Rust, "\"hi\".to_string()").unwrap();
        assert_eq!(term.coparse(), "\"hi\".to_string()");
    }

    #[test]
    fn parses_python_statements() {
        let term = term(Lang::Python, "count = 1\nreturn str(count)").unwrap();
        assert_eq!(term.coparse(), "count = 1\nreturn str(count)");
    }

    #[test]
    fn rejects_malformed_fragments() {
        assert!(term(Lang::Rust, "let x = ;").is_err());
        assert!(term(Lang::Python, "return ((").is_err());
    }
}
