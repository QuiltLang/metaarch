# metaarch Documentation

metaarch compiles a one-file description of a distributed system into the
whole running thing: services in Rust and Python, per-service databases,
event contracts, tests, docs, and a Nix-based deployment. It exists to show
off [quilt](https://github.com/QuiltLang/quilt)'s systems-scale story —
polyglot code generation from a single typed source of truth.

The pipeline:

```
system.arch          the DSL: services, dbs, events        (docs: dsl.md)
   │  metaarch-parser
   ▼
SystemSpec           typed Rust data — the source of truth
   │  metaarch-spec::validate
   ▼
validated spec       invalid architectures never generate  (docs: validation.md)
   │  metaarch-codegen (quilt metaprograms)
   ▼
out/<system>/        the generated system                  (docs: codegen.md)
```

The core claim: change one line of the `.arch` file — add a field to an
event, add a service, move a port — re-run `bin/main generate`, and every
artifact in every language updates consistently. Drift between services is
structurally impossible.

## Pages

1. **[Plan](plan.md)** — the phased build plan; read this first
2. **[The .arch DSL](dsl.md)** — grammar, semantics, and the planned quilt integration
3. **[Validation](validation.md)** — the architectural checks and their philosophy
4. **[Code generation](codegen.md)** — how the quilt generators work and what they emit
5. **[Editor support](lsp.md)** — the tree-sitter-arch grammar and metaarch-lsp

## Status

Phases 0–3 are done (parse → validate → generate → Nix deployment), and
phase 4 — arch as a first-class quilt language — is complete through 4e:
inline `impl` fragments, dynamic registration, real-AST fragments, the arch
`MetaLanguage`, and editor support via `tree-sitter-arch` + `metaarch-lsp`.
See the [plan](plan.md) for the demos and the decisions log.
