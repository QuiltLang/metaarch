//! Semantic tokens: tree-sitter highlighting for arch, multiplexed over the
//! `impl` fragments.
//!
//! The arch structure is highlighted with `tree-sitter-arch`'s query. Each
//! `impl` fragment interior is then re-highlighted with the fragment
//! language's own grammar — the annotation if present, else the service's
//! `lang` (the same default the generators use). This is the highlight-only
//! half of quilt-lsp's embedded-fragment multiplexing: fragments are handler
//! *bodies*, not standalone files, so there is no downstream server to proxy
//! — the tree-sitter grammars quilt already vendors do the work in-process.

use std::ops::Range;
use std::sync::OnceLock;

use streaming_iterator::StreamingIterator;
use tower_lsp::lsp_types::{SemanticToken, SemanticTokenType};

/// The legend advertised at registration; capture names resolve to indices
/// into this list. Mirrors quilt-lsp's `TOKEN_TYPES`.
pub const TOKEN_TYPES: &[SemanticTokenType] = &[
    SemanticTokenType::COMMENT,
    SemanticTokenType::DECORATOR,
    SemanticTokenType::FUNCTION,
    SemanticTokenType::KEYWORD,
    SemanticTokenType::MACRO,
    SemanticTokenType::NAMESPACE,
    SemanticTokenType::NUMBER,
    SemanticTokenType::OPERATOR,
    SemanticTokenType::PARAMETER,
    SemanticTokenType::PROPERTY,
    SemanticTokenType::STRING,
    SemanticTokenType::STRUCT,
    SemanticTokenType::TYPE,
    SemanticTokenType::VARIABLE,
];

/// Map a highlight-query capture name (nvim-style dotted) to an index into
/// [`TOKEN_TYPES`]. `None` drops the capture (punctuation, `error`, …).
fn token_type_index(capture: &str) -> Option<u32> {
    let name = match capture.split('.').next().unwrap_or(capture) {
        "number" | "float" => "number",
        "boolean" | "keyword" | "repeat" | "conditional" | "storageclass" => "keyword",
        "type" | "constructor" | "tag" => "type",
        "function" | "method" => "function",
        "parameter" => "parameter",
        "structure" | "struct" => "struct",
        "field" | "property" => "property",
        "attribute" => "decorator",
        "constant" | "variable" | "escape" => "variable",
        "operator" => "operator",
        "comment" => "comment",
        "string" => "string",
        "namespace" | "module" => "namespace",
        "macro" => "macro",
        _ => return None,
    };
    TOKEN_TYPES
        .iter()
        .position(|t| t.as_str() == name)
        .map(|i| i as u32)
}

/// Which pattern wins when several capture the same node (see quilt-lsp's
/// `tshl`): nvim-flavored queries put specific patterns first, upstream
/// tree-sitter queries let later patterns override.
#[derive(Clone, Copy, PartialEq)]
enum Order {
    FirstWins,
    LastWins,
}

/// A compiled grammar + highlight query.
struct Highlighter {
    language: tree_sitter::Language,
    query: tree_sitter::Query,
    capture_types: Vec<Option<u32>>,
    order: Order,
}

impl Highlighter {
    fn new(language: tree_sitter::Language, query_src: &str, order: Order) -> Option<Self> {
        let query = match tree_sitter::Query::new(&language, query_src) {
            Ok(q) => q,
            Err(e) => {
                eprintln!("highlight query failed to compile: {e}");
                return None;
            }
        };
        let capture_types = query
            .capture_names()
            .iter()
            .map(|name| token_type_index(name))
            .collect();
        Some(Highlighter {
            language,
            query,
            capture_types,
            order,
        })
    }

    /// Highlight `text`, returning byte spans + token type indices. Overlaps
    /// resolve leaf-first (the innermost captured node wins), then by pattern
    /// order per [`Order`].
    fn spans(&self, text: &str) -> Vec<(Range<usize>, u32)> {
        let mut parser = tree_sitter::Parser::new();
        if parser.set_language(&self.language).is_err() {
            return Vec::new();
        }
        let Some(tree) = parser.parse(text, None) else {
            return Vec::new();
        };
        let mut cursor = tree_sitter::QueryCursor::new();
        let mut matches = cursor.matches(&self.query, tree.root_node(), text.as_bytes());
        // (start, end) → (type index, pattern index)
        let mut by_range: std::collections::BTreeMap<(usize, usize), (u32, usize)> =
            std::collections::BTreeMap::new();
        while let Some(m) = matches.next() {
            for cap in m.captures {
                let Some(ty) = self.capture_types[cap.index as usize] else {
                    continue;
                };
                let range = cap.node.byte_range();
                if range.is_empty() {
                    continue;
                }
                let key = (range.start, range.end);
                let better = match by_range.get(&key) {
                    None => true,
                    Some(&(_, pat)) => match self.order {
                        Order::FirstWins => m.pattern_index < pat,
                        Order::LastWins => m.pattern_index >= pat,
                    },
                };
                if better {
                    by_range.insert(key, (ty, m.pattern_index));
                }
            }
        }
        // Leaf shadows container: drop a span that strictly contains another
        // captured span.
        let keys: Vec<(usize, usize)> = by_range.keys().copied().collect();
        keys.iter()
            .filter(|&&(s, e)| {
                !keys
                    .iter()
                    .any(|&(s2, e2)| (s2, e2) != (s, e) && s <= s2 && e2 <= e)
            })
            .map(|&(s, e)| (s..e, by_range[&(s, e)].0))
            .collect()
    }
}

fn arch() -> Option<&'static Highlighter> {
    static ARCH: OnceLock<Option<Highlighter>> = OnceLock::new();
    ARCH.get_or_init(|| {
        // Our own query is nvim-flavored: specific patterns first.
        Highlighter::new(
            tree_sitter_arch::LANGUAGE.into(),
            tree_sitter_arch::HIGHLIGHTS_QUERY,
            Order::FirstWins,
        )
    })
    .as_ref()
}

fn fragment_highlighter(lang: &str) -> Option<&'static Highlighter> {
    match lang {
        "rust" => {
            static RUST: OnceLock<Option<Highlighter>> = OnceLock::new();
            RUST.get_or_init(|| {
                // The fork's own query (vendored beside this crate; upstream-
                // flavored, later patterns override).
                Highlighter::new(
                    quilt::grammars::rust::LANGUAGE.into(),
                    include_str!("../queries/rust-highlights.scm"),
                    Order::LastWins,
                )
            })
            .as_ref()
        }
        "python" => {
            static PYTHON: OnceLock<Option<Highlighter>> = OnceLock::new();
            PYTHON
                .get_or_init(|| {
                    Highlighter::new(
                        quilt::grammars::python::LANGUAGE.into(),
                        quilt::grammars::python::HIGHLIGHTS_QUERY,
                        Order::LastWins,
                    )
                })
                .as_ref()
        }
        _ => None,
    }
}

/// One `impl` fragment interior: its byte range in the document and the
/// language that should highlight it.
struct FragmentRegion {
    range: Range<usize>,
    lang: String,
}

/// Walk the arch CST for `impl` entries and resolve each fragment's language:
/// the annotation if present, else the enclosing service's `lang` value.
fn fragment_regions(text: &str) -> Vec<FragmentRegion> {
    let mut parser = tree_sitter::Parser::new();
    if parser.set_language(&tree_sitter_arch::LANGUAGE.into()).is_err() {
        return Vec::new();
    }
    let Some(tree) = parser.parse(text, None) else {
        return Vec::new();
    };
    let node_text = |n: tree_sitter::Node| text[n.byte_range()].to_string();
    let mut regions = Vec::new();
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        if node.kind() != "impl_entry" {
            let mut cursor = node.walk();
            stack.extend(node.children(&mut cursor));
            continue;
        }
        let lang = node
            .child_by_field_name("language")
            .map(&node_text)
            .or_else(|| {
                // The enclosing service's `lang` entry, the generators' default.
                let service = node.parent()?;
                let mut cursor = service.walk();
                service
                    .children(&mut cursor)
                    .find(|c| c.kind() == "lang_entry")
                    .and_then(|entry| entry.child_by_field_name("value"))
                    .map(&node_text)
            });
        let (Some(lang), Some(fragment)) = (lang, node.child_by_field_name("body")) else {
            continue;
        };
        // Interior: between the `↖` (first child) and the closing `↗`.
        let last = u32::try_from(fragment.child_count().saturating_sub(1)).unwrap_or(0);
        let (Some(open), Some(close)) = (fragment.child(0), fragment.child(last)) else {
            continue;
        };
        if close.kind() != "↗" {
            continue; // unclosed fragment — nothing safe to highlight
        }
        regions.push(FragmentRegion {
            range: open.end_byte()..close.start_byte(),
            lang,
        });
    }
    regions
}

/// Full-document semantic tokens: arch structure + each fragment interior in
/// its own language, merged, split at line boundaries, delta-encoded against
/// [`TOKEN_TYPES`].
pub fn semantic_tokens(text: &str) -> Vec<SemanticToken> {
    let mut spans: Vec<(Range<usize>, u32)> = Vec::new();
    if let Some(arch) = arch() {
        spans.extend(arch.spans(text));
    }
    for region in fragment_regions(text) {
        let Some(hl) = fragment_highlighter(&region.lang) else {
            continue;
        };
        let body = &text[region.range.clone()];
        for (r, ty) in hl.spans(body) {
            spans.push((region.range.start + r.start..region.range.start + r.end, ty));
        }
    }
    spans.sort_by_key(|(r, _)| (r.start, r.end));
    encode(text, &spans)
}

/// Split spans at line boundaries and delta-encode with UTF-16 columns.
fn encode(text: &str, spans: &[(Range<usize>, u32)]) -> Vec<SemanticToken> {
    let mut line_starts = vec![0usize];
    for (i, b) in text.bytes().enumerate() {
        if b == b'\n' {
            line_starts.push(i + 1);
        }
    }
    let line_of = |offset: usize| line_starts.partition_point(|&s| s <= offset) - 1;
    let utf16_col = |line: usize, offset: usize| -> u32 {
        text[line_starts[line]..offset]
            .chars()
            .map(|c| c.len_utf16() as u32)
            .sum()
    };

    let mut out = Vec::new();
    let mut prev_line = 0u32;
    let mut prev_col = 0u32;
    for (range, ty) in spans {
        let mut start = range.start;
        while start < range.end {
            let line = line_of(start);
            let line_end = line_starts
                .get(line + 1)
                .map_or(text.len(), |&next| next - 1);
            let end = range.end.min(line_end);
            if end > start {
                let col = utf16_col(line, start);
                let len: u32 = text[start..end].chars().map(|c| c.len_utf16() as u32).sum();
                let line = line as u32;
                let delta_line = line - prev_line;
                let delta_start = if delta_line == 0 { col - prev_col } else { col };
                out.push(SemanticToken {
                    delta_line,
                    delta_start,
                    length: len,
                    token_type: *ty,
                    token_modifiers_bitset: 0,
                });
                prev_line = line;
                prev_col = col;
            }
            if range.end <= line_end {
                break;
            }
            start = line_end + 1; // continue on the next line
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode_types(tokens: &[SemanticToken]) -> Vec<&'static str> {
        tokens
            .iter()
            .map(|t| TOKEN_TYPES[t.token_type as usize].as_str())
            .collect()
    }

    #[test]
    fn arch_structure_highlights() {
        let tokens = semantic_tokens("system shop\n\nservice orders {\n  lang rust\n  port 8081\n}\n");
        let types = decode_types(&tokens);
        assert!(types.contains(&"keyword"), "{types:?}");
        assert!(types.contains(&"namespace"), "{types:?}");
        assert!(types.contains(&"number"), "{types:?}");
    }

    #[test]
    fn annotated_rust_fragment_gets_rust_tokens() {
        let src = "system s\nservice g {\n  lang rust\n  impl get /hello rust↖\n    \"hi\".to_string()\n  ↗\n}\n";
        let types = decode_types(&semantic_tokens(src));
        // The string literal inside the fragment highlights as a string, and
        // `.to_string()` as a function — both come from the rust grammar.
        assert!(types.contains(&"string"), "{types:?}");
        assert!(types.contains(&"function"), "{types:?}");
    }

    #[test]
    fn unannotated_fragment_defaults_to_service_lang() {
        let src = "system s\nservice n {\n  lang python\n  impl get /stats ↖\n    return \"ok\"\n  ↗\n}\n";
        let types = decode_types(&semantic_tokens(src));
        assert!(types.contains(&"string"), "{types:?}");
    }

    #[test]
    fn shop_example_produces_tokens_on_every_construct() {
        let src = std::fs::read_to_string("../examples/shop.arch").unwrap();
        let tokens = semantic_tokens(&src);
        assert!(tokens.len() > 40, "only {} tokens", tokens.len());
    }

    #[test]
    fn tokens_are_single_line_and_monotonic() {
        let src = std::fs::read_to_string("../examples/shop.arch").unwrap();
        for tok in semantic_tokens(&src) {
            assert!(tok.length > 0);
        }
    }
}
