# Code generation

`metaarch-codegen` turns a validated `SystemSpec` into a runnable system.
Phases 1–3 of the [plan](plan.md) implemented what is described here.

## How the generators work

Generators are **quilt metaprograms**: `.rs.quilt` sources in
`metaarch-codegen` that loop over the spec and *quote* their output —
`python↖ ... ↗`, `zsh↖ ... ↗`, `sql↖ ... ↗`, `nix` — splicing spec data in
with unquotes and lifts. This is the nanobots pattern:

- `.rs.quilt` files live next to their expanded `.rs` siblings (gitignored)
- `bin/expand` re-expands them via `metaarch-expand`, the workspace's own
  expander binary — quilt's built-in languages plus `arch` registered
  dynamically (`DictMulti::add_lang`, phase 4b); run it after editing any
  `.quilt` source, before `cargo build`
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

Lessons (phases 1–2) about where quotes stop and builders start:

- **Identifier positions take no holes in the Rust grammar** (types and
  patterns do), so items whose *names* come from the spec — the serde event
  structs, the client structs/methods — are built with the `tb`/`leaf` term
  builders instead of quotes.
- **Python quotes expand variadic blocks as fluent chains** with no named
  builder, so ground emit loops (`←`) cannot run inside them. Dynamic
  statement lists are built at ground (`py_block`) and spliced through a
  single hole; Rust and bash quotes take emit loops directly.
- **A lone item quote in statement/tail position expands as an emit
  statement** (`….emit(&mut b_)`), not a value — bind it to a local first
  and return the local (multi-item quotes wrap themselves in a
  `source_file` builder and are fine).
- **HTML splices are `raw_text` leaves** (the `html_report` pattern): the
  page skeleton is one html quote; spec-driven rows/sections are plain HTML
  strings built at ground and injected with a `raw()` helper.
- **Generated Nix sticks to three splice positions** (phase 3): expression
  values, lifted strings, and variadic list interiors. Anything keyed by a
  spec name goes through `builtins.listToAttrs` (attrset keys are plain
  strings there) and the NixOS modules bind their varying names in a `let`
  and select with `${name}` dynamics — so no identifier or attrset-key
  position is ever spliced. Multi-line shell text inside Nix is built as a
  ground Rust string and lifted whole (`\n` escapes in the emitted literal).
- **SQL splices in three positions and no more** (issue #43): an
  `object_reference` (the table a statement names), a column name or type
  inside a definition, and a list interior — `VALUES (…)`, an `INSERT`'s
  column list, a CHECK's `IN (…)`. Two things the grammar gives no hole for
  are handled with builders: a *column definition on its own* (nothing in
  `program` accepts one, so each is written inside a throwaway one-column
  `CREATE TABLE` and lifted back out — quilt's own wrap-and-strip trick for
  bare expressions, one level up), and the `column_definitions` list
  interior, where a hole parses as a column *name* and leaves the type
  MISSING. Whole files are assembled through a *statement*-position hole.
  Every value that comes from the spec is lifted, so no `'…'` is written in
  a generator; each generator reparses its own output with quilt's SQL
  grammar in a test.

The config files (TOML, YAML, `.envrc`, README, `Cargo.lock`) and the
topology SVG are plain text built in ordinary Rust — quilt has no grammar
for them yet. The migrations and seed generators share the DDL spelling
(`column_def`, `create_table`, `sql_ty`) with the schema generator so an
`ALTER` adds a column spelled exactly as a fresh `CREATE` would.

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
├── Cargo.lock              # GENERATED from the vendored canonical lock:
│                           #   pinned deps, valid for every generated system
├── flake.nix               # GENERATED Nix (root): per-service package
│                           #   builds, nix run apps, fleet + fleet-pc apps,
│                           #   NixOS modules
├── process-compose.yaml    # GENERATED: the fleet as a process-compose
│                           #   project — bin/main --process-compose
├── system.arch             # byte-identical copy of the source .arch:
│                           #   provenance + the migration diff base
├── nix/
│   ├── flake.nix           # GENERATED Nix: dev shell — toolchains, db
│   │                       #   engines, curl, process-compose (the .envrc
│   │                       #   loads this one)
│   └── modules/            # GENERATED NixOS module per service:
│                           #   services.<system>.<svc>.enable → systemd unit
├── bin/
│   ├── main                # GENERATED bash: provisions dbs, builds, boots
│   │                       #   the whole fleet, kills it together — or hands
│   │                       #   it to process-compose (--process-compose)
│   ├── db                  # GENERATED bash: postgres initdb/start (unix
│   │                       #   socket under .pgdata/) + schema/seed apply
│   └── smoke               # GENERATED end-to-end test: boots the fleet,
│                           #   curls every derived route, reports pass/fail
├── gateway/                # lang rust  → axum service crate
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs         #   health + /peers routes
│       ├── events.rs       #   serde structs (system-wide: clients use them)
│       └── clients.rs      #   typed clients for the peer services
├── orders/                 # lang rust + db postgres
│   ├── Cargo.toml
│   ├── src/main.rs         #   axum skeleton, health + peers + event routes
│   ├── src/events.rs       #   serde structs for the system's events
│   ├── src/bus.rs          #   HTTP fan-out shim (emitters only)
│   ├── src/clients.rs      #   typed clients for the peer services
│   └── sql/
│       ├── schema.sql      #   DDL derived from the `db` block
│       ├── seed.sql        #   three deterministic rows per table
│       └── migrations/     #   numbered ALTER steps, appended per generate
├── notifier/               # lang python → stdlib-only package
│   ├── notifier/__init__.py    # HTTP server, typed routes, serve()
│   ├── notifier/__main__.py    # python3 -m notifier
│   └── notifier/events.py      # dataclasses mirroring orders/src/events.rs
└── docs/
    ├── index.html          # GENERATED topology + API/event reference
    └── topology.svg        #   the architecture diagram (also inlined above)
```

The layout rule: **one service, one directory, named by the service**;
system-level concerns (env, boot, tests, docs) sit at the root exactly where
metaarch itself keeps them. Both flakes and the NixOS modules are emitted by
the `nix.rs.quilt` generator as quilt `nix↖…↗` target quotes — generated Nix
ASTs built from the same spec, not a copied template. The *root* flake is
the one deviation from the mirror-this-repo shape: `nix build` can only see
sources inside the flake's own directory, so the buildable packages live at
the root while the dev shell stays at `nix/flake.nix` (initialize git in a
generated system before `nix build` so the source copy filters `target/`).

## Running the fleet

Three runners, one topology, all derived from the same spec:

| runner | what it runs | supervision |
|---|---|---|
| `bin/main` | the source tree (`cargo run`, `python3 -m`) | `trap 'kill 0' EXIT` |
| `bin/main --process-compose` | the source tree, via `process-compose.yaml` | ordering, readiness probes, restart policy, TUI |
| `nix run` / `nix run .#fleet-pc` | the *built* services from the store | `kill 0` / process-compose |

`process-compose.yaml` gives each service an HTTP readiness probe on the
`/health` route the spec already derives, and puts the two things `bin/main`
does before booting — `bin/db up`, `cargo build` — in the graph as one-shots
the services `depend_on` with `process_completed_successfully`. The event
topology is deliberately *not* in that graph: two services may legally
consume each other's events, and a cycle would deadlock `process-compose up`
on a system `metaarch check` accepts.

Containers are the fourth option and the one that needs a Linux builder
(`nix build .#<svc>-image`). process-compose covers the same ground —
declared dependencies, health, restarts, one log stream — without them.

## Artifact map (target state)

| `.arch` construct | generates |
|---|---|
| `service` + `lang rust` | axum crate: routes, health + peers endpoints, typed peer clients |
| `service` + `lang python` | python package: consumer loop, typed handlers |
| `port` | bind config, `bin/main` orchestration entry, smoke-test URL, process-compose readiness probe |
| `db` block | `sql/schema.sql` + `sql/seed.sql` (per engine), `sql/migrations/` diffs, db provisioning in `nix/` (phase 3), `analytics/<table>.wgsl` compute shaders |
| `table` | DDL + seed rows + migration steps; Rust structs + query helpers (phase 4) |
| `emits E { ... }` | Rust struct / Python dataclass for `E`, client `emit_e` method, emit route, smoke check, docs entry |
| `consumes E` | delivery route + typed handler, client `deliver_e` method, smoke check |
| `impl get /x rust↖…↗` | route + handler with the parsed inline fragment as its body, smoke check, docs row |
| whole system | `nix/flake.nix`, `bin/main`, `bin/smoke`, `process-compose.yaml`, `docs/index.html`, `docs/topology.svg`, README, `system.arch` snapshot |
| each service, in the root flake | `packages.<svc>` build, `nix run` app, `nixosModules.<svc>`, `packages.<svc>-image` container image, a `fleet-pc` process |

## Event transport (MVP)

Phase 1 uses HTTP fan-out: emitting `E` POSTs its JSON to
`/events/E` on every consumer, addresses resolved from the spec at
generation time. No broker to deploy, fully traceable with curl. The
transport is a generator concern invisible to `.arch` files, so swapping in
NATS/Redis later (or per-event, by annotation) regenerates the wiring
without touching any system description.

Concretely, every service (either language) serves the same route shape:

- `GET /health` — liveness.
- `GET /peers` — Rust services in a multi-service system: the health of
  every peer, checked through the generated typed clients (`clients.rs`) —
  the route exists so the clients are exercised by generated code.
- `POST /emit/<E>` — on the emitter of `E`: parse the JSON into the typed
  event, then fan out to each consumer's `/events/<E>`. This is the manual
  trigger for every event path until phase 4 gives services real handlers.
- `POST /events/<E>` — on each consumer of `E`: parse into the typed
  struct/dataclass and log the delivery.
- any `impl` route the service declares (phase 4a) — the inline fragment
  becomes the handler body. Since phase 4c the fragment is parsed with the
  real grammar of the service's language (`metaarch-codegen`'s `fragment`
  module) and the term is spliced into a builder-built handler; the route is
  wired into the router/dispatch by generated name (`impl_<method>_<path>`).

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

Migrations need history without breaking that: `migrations` is a separate
pure function of (previous spec, next spec, existing migration counts). The
CLI supplies the inputs from the output directory — the `system.arch`
snapshot the last run wrote, and a scan of each `sql/migrations/` for the
next number. An unchanged spec adds no files, so regenerating stays
idempotent.
