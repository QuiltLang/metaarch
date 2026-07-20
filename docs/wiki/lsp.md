# Editor support: tree-sitter-arch + metaarch-lsp

Phase 4e gives `.arch` files first-class editor support from two pieces that
live in this repo:

* **`tree-sitter-arch/`** — the editor-facing grammar. Deliberately loose,
  like the quilt-side `metaarch-lang` parser: any identifier parses as a
  type/method/language, because the closed sets and topology checks belong to
  `metaarch-parser`/`metaarch-spec`. `src/parser.c` is generated and
  committed (like quilt's vendored grammars) so builds never need the
  tree-sitter CLI; regenerate with `bin/grammar` after editing `grammar.js`.
* **`metaarch-lsp/`** — a small language server (`cargo build -p
  metaarch-lsp` → `target/debug/metaarch-lsp`, stdio transport).

## What the server does

**Diagnostics** are exactly `metaarch check`: parse →
`metaarch_spec::validate` → `metaarch_codegen::check_fragments`. The editor
squiggle and the CLI line can never disagree, and a malformed `impl` fragment
is rejected by the same tree-sitter-backed parse the generators rely on.

**Semantic tokens** highlight the arch structure from
`tree-sitter-arch`'s query, then re-highlight each `impl` fragment interior with
its own language's grammar — the annotation (`rust↖…↗`) if present, else the
service's `lang`, the same default the generators use. This is the
highlight-only half of quilt-lsp's embedded-fragment multiplexing, done
in-process with the grammars quilt already vendors; fragments are handler
*bodies*, not standalone files, so there is no downstream rust-analyzer /
pyright to proxy (see the decisions log).

## Neovim

```lua
vim.filetype.add({ extension = { arch = "arch" } })

vim.api.nvim_create_autocmd("FileType", {
  pattern = "arch",
  callback = function()
    vim.lsp.start({
      name = "metaarch-lsp",
      cmd = { "/path/to/metaarch/target/debug/metaarch-lsp" },
      root_dir = vim.fs.root(0, { ".git" }),
    })
  end,
})
```

Semantic tokens light up with any theme that colors LSP semantic token
groups. For tree-sitter highlighting/injections instead (or additionally),
register the grammar with nvim-treesitter:

```lua
require("nvim-treesitter.parsers").get_parser_configs().arch = {
  install_info = { url = "/path/to/metaarch/tree-sitter-arch", files = { "src/parser.c" } },
  filetype = "arch",
}
```

then copy `tree-sitter-arch/queries/*.scm` to
`~/.config/nvim/queries/arch/`. The injection query hands *annotated*
fragment bodies (`rust↖…↗`) to the Rust grammar; un-annotated fragments are
covered by the LSP semantic tokens only, since a query can't look across to
the service's `lang` entry.

## VS Code / other editors

Any generic LSP client works — point it at the `metaarch-lsp` binary for the
`arch` language / `*.arch` files. (A packaged VS Code extension is a stretch
item.)

## `.arch.quilt` files

quilt-lsp serves `.quilt` files, but its adapter registries are static
matches with no dynamic-registration hook at the pinned rev, so it cannot
learn arch the way `DictMulti::add_lang` let the expander do it (see the
decisions log). Until quilt grows that hook upstream, `.arch.quilt` files get
quilt's own syntactic support only; the plain `.arch` sibling (`bin/expand`
writes it) gets the full treatment described here.
