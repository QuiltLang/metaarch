# Code generation

`metaarch-codegen` turns a validated `SystemSpec` into a runnable system.
Phase 1 of the [plan](plan.md) implemented the MVP described here.

## How the generators work

Generators are **quilt metaprograms**: `.rs.quilt` sources in
`metaarch-codegen` that loop over the spec and *quote* their output —
`python↖ ... ↗`, `zsh↖ ... ↗`, SQL-as-text, `nix` — splicing spec data in
with unquotes and lifts. This is the nanobots pattern:

- `.rs.quilt` files live next to their expanded `.rs` siblings (gitignored)
- `bin/expand` re-expands them via the sibling quilt checkout (`$QUILT`
  overrides); run it after editing any `.quilt` source, before `cargo build`
- the expanded generators compile into `metaarch-codegen` as ordinary Rust,
  so `metaarch generate` is one static binary calling generator functions

Why quilt instead of string templates: the output is built as real ASTs
(`QTerm`s), so quoting, indentation, and splicing are structural. A `Vec` of
field names lifts into a Python list or a Rust initializer with `↑` — the
per-type, per-target spellings live in one place (`LiftTo` impls), not
scattered across template files.

Each artifact kind is one generator function `fn(&SystemSpec) -> Artifact`
(path + contents). Adding an artifact kind — a Grafana dashboard, a k6 load
test — is one more function over the same spec, one more loop in `generate`.

Two phase-1 lessons about where quotes stop and builders start:

- **Identifier positions take no holes in the Rust grammar** (types and
  patterns do), so items whose *names* come from the spec — the serde event
  structs — are built with the `tb`/`leaf` term builders instead of quotes.
- **Python quotes expand variadic blocks as fluent chains** with no named
  builder, so ground emit loops (`←`) cannot run inside them. Dynamic
  statement lists are built at ground (`py_block`) and spliced through a
  single hole; Rust and bash quotes take emit loops directly.

SQL and the config files (TOML, `.envrc`, README) are plain text built in
ordinary Rust — quilt has no grammar for them yet.

## Generated-system layout

A generated system is a repo that **mirrors metaarch's own shape** — `nix/`
env, `bin/main` entrypoint, one directory per component — so navigating
output feels like navigating this repo:

```
out/shop/
├── .envrc                  # use flake ./nix — same convention as this repo
├── .gitignore
├── README.md               # GENERATED topology + ports + curl instructions
├── Cargo.toml              # workspace over the rust service crates
├── nix/
│   └── flake.nix           # dev env: rust + python toolchains (plain text
│                           #   until the phase 3 .nix.quilt generator)
├── bin/
│   ├── main                # GENERATED bash: builds, boots the whole fleet,
│   │                       #   kills it together (db provisioning: phase 3)
│   └── smoke               # GENERATED end-to-end smoke test (phase 2)
├── gateway/                # lang rust  → axum service crate
│   ├── Cargo.toml
│   └── src/main.rs
├── orders/                 # lang rust + db postgres
│   ├── Cargo.toml
│   ├── src/main.rs         #   axum skeleton, health + event routes
│   ├── src/events.rs       #   serde structs for emits/consumes
│   ├── src/bus.rs          #   HTTP fan-out shim (emitters only)
│   └── sql/schema.sql      #   DDL derived from the `db` block
├── notifier/               # lang python → stdlib-only package
│   ├── notifier/__init__.py    # HTTP server, typed routes, serve()
│   ├── notifier/__main__.py    # python3 -m notifier
│   └── notifier/events.py      # dataclasses mirroring orders/src/events.rs
└── docs/
    └── index.html          # GENERATED topology + API/event reference (phase 2)
```

The layout rule: **one service, one directory, named by the service**;
system-level concerns (env, boot, tests, docs) sit at the root exactly where
metaarch itself keeps them. `nix/flake.nix` is emitted by the phase 3
`.nix.quilt` generator — quilt's string-based Nix host maps host unquotes
onto Nix's own `${...}` antiquotation, so the flake is generated Nix built
from the same spec, not a copied template.

## Artifact map (target state)

| `.arch` construct | generates |
|---|---|
| `service` + `lang rust` | axum crate: routes, state, health endpoint |
| `service` + `lang python` | python package: consumer loop, typed handlers |
| `port` | bind config, `bin/main` orchestration entry, smoke-test URL |
| `db` block | `sql/schema.sql` (per engine), migrations (phase 2), db provisioning in `nix/` (phase 3) |
| `table` | DDL + Rust structs + query helpers for the owning service |
| `emits E { ... }` | Rust struct / Python dataclass for `E`, emit helper, docs entry |
| `consumes E` | subscription wiring + typed handler stub |
| whole system | `nix/flake.nix`, `bin/main`, `bin/smoke`, `docs/index.html`, README |

## Event transport (MVP)

Phase 1 uses HTTP fan-out: emitting `E` POSTs its JSON to
`/events/E` on every consumer, addresses resolved from the spec at
generation time. No broker to deploy, fully traceable with curl. The
transport is a generator concern invisible to `.arch` files, so swapping in
NATS/Redis later (or per-event, by annotation) regenerates the wiring
without touching any system description.

Concretely, every service (either language) serves the same route shape:

- `GET /health` — liveness.
- `POST /emit/<E>` — on the emitter of `E`: parse the JSON into the typed
  event, then fan out to each consumer's `/events/<E>`. This is the manual
  trigger for every event path until phase 4 gives services real handlers.
- `POST /events/<E>` — on each consumer of `E`: parse into the typed
  struct/dataclass and log the delivery.

Services without a declared `port` listen on a deterministic fallback
(`9000 + index` of the service in the file), so every consumer is
addressable. Rust emitters carry a dependency-free `bus.rs` (hand-rolled
HTTP/1.1 POST over `TcpStream`); Python services use only the stdlib.

## Determinism

`generate` is a pure function of the `SystemSpec`: same input, byte-identical
output. No timestamps, no ordering dependent on hash maps. This keeps
generated systems diffable — the phase 2 demo ("one field added, watch the
diff") depends on it — and makes `generate` idempotent and CI-checkable,
like quilt's own `check-bootstrap`.
