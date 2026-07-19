//! `metaarch-lsp`: a language server for `.arch` files (phase 4e).
//!
//! quilt-lsp's registries (`language_adapter`, `meta_adapter`, `highlighter`)
//! are static matches with no dynamic hook at the pinned rev — unlike
//! `DictMulti::add_lang`, which made 4b free — so arch gets its own small
//! server instead, the same move that produced `metaarch-expand`. It serves
//! exactly what the plan bullet asks for:
//!
//! * **Diagnostics** — the same pipeline `metaarch check` runs (parse →
//!   `validate` → `check_fragments`), so the editor and the CLI can never
//!   disagree about what's wrong.
//! * **Semantic tokens** — the `tree-sitter-arch` grammar highlights the arch
//!   structure, and each `impl` fragment body is re-highlighted with its own
//!   language's grammar (the annotation, else the service's `lang`) — the
//!   in-process multiplexing quilt-lsp does for embedded fragments,
//!   highlight-only.

pub mod analysis;
pub mod highlight;
pub mod server;
