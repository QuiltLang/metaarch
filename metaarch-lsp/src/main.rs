//! `metaarch-lsp` binary: serve LSP over stdio. Point your editor at this
//! for `.arch` files — see docs/wiki/lsp.md for nvim/VS Code wiring.

#[tokio::main]
async fn main() {
    metaarch_lsp::server::run_stdio().await;
}
