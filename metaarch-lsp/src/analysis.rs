//! Diagnostics: the `metaarch check` pipeline, mapped to LSP ranges.

use metaarch_spec::Span;
use metaarch_spec::validate::{Severity, validate};
use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, Position, Range};

/// Run the full check pipeline over one document: parse, then (if that
/// succeeds) architectural validation and fragment parsing. Identical to the
/// CLI's `check`, so an editor squiggle and a `metaarch check` line always
/// agree.
pub fn diagnostics(text: &str) -> Vec<Diagnostic> {
    let spec = match metaarch_parser::parse(text) {
        Ok(spec) => spec,
        Err(err) => {
            return vec![Diagnostic {
                range: span_range(text, err.span),
                severity: Some(DiagnosticSeverity::ERROR),
                source: Some("metaarch".into()),
                message: err.message,
                ..Default::default()
            }];
        }
    };
    let mut diags = validate(&spec);
    diags.extend(metaarch_codegen::check_fragments(&spec));
    diags
        .into_iter()
        .map(|d| Diagnostic {
            range: span_range(text, d.span),
            severity: Some(match d.severity {
                Severity::Error => DiagnosticSeverity::ERROR,
                Severity::Warning => DiagnosticSeverity::WARNING,
            }),
            source: Some("metaarch".into()),
            message: d.message,
            ..Default::default()
        })
        .collect()
}

/// Map a 1-based `Span` (line + char column) to an LSP range covering the
/// token that starts there: identifier/path characters, or a single character
/// for punctuation. Columns are converted to UTF-16 code units.
fn span_range(text: &str, span: Span) -> Range {
    let line_idx = span.line.saturating_sub(1);
    let line = text.lines().nth(line_idx as usize).unwrap_or("");
    let chars: Vec<char> = line.chars().collect();
    let start = (span.col.saturating_sub(1) as usize).min(chars.len());
    let mut end = start;
    while end < chars.len()
        && (chars[end].is_alphanumeric() || matches!(chars[end], '_' | '-' | '/'))
    {
        end += 1;
    }
    if end == start && start < chars.len() {
        end = start + 1;
    }
    let utf16 = |n: usize| -> u32 {
        chars[..n].iter().map(|c| c.len_utf16() as u32).sum()
    };
    Range::new(
        Position::new(line_idx, utf16(start)),
        Position::new(line_idx, utf16(end)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_files_have_no_diagnostics() {
        let src = std::fs::read_to_string("../examples/shop.arch").unwrap();
        assert_eq!(diagnostics(&src), Vec::new());
    }

    #[test]
    fn parse_errors_are_positioned() {
        let diags = diagnostics("system s\nservice x {\n  port oops\n}\n");
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("port"), "{}", diags[0].message);
        assert_eq!(diags[0].range.start.line, 2);
        // `oops` starts at col 7 (0-based) and the range covers the word.
        assert_eq!(diags[0].range.start.character, 7);
        assert_eq!(diags[0].range.end.character, 11);
    }

    #[test]
    fn validation_and_fragment_errors_surface() {
        let src = "system s\nservice x {\n  lang rust\n  impl get /a rust↖ let x = ; ↗\n}\n";
        let diags = diagnostics(src);
        assert!(
            diags.iter().any(|d| d.message.contains("does not parse")),
            "{diags:?}"
        );
        assert!(diags.iter().all(|d| d.severity == Some(DiagnosticSeverity::ERROR)));
    }
}
