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

## Status

Phase 0 is done: the repo, the DSL v0, the parser, the validator, and the
`check`/`dump` CLI. `bin/main check examples/shop.arch` works today.
Generation is phase 1 — see the [plan](plan.md).
