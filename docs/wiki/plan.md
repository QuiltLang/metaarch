# Build Plan

metaarch is built in phases; each phase ends with something demonstrable.
The guiding constraint throughout: **the `.arch` file is the only source of
truth**, and anything the generators emit must be derivable from it.

## Phase 0 — foundation ✅

Repo scaffold (workspace, `nix/` env, `bin/main` entrypoint, wiki) plus the
front half of the pipeline:

- [x] `.arch` DSL v0: `system`, `service`, `lang`, `port`, `db` (postgres/sqlite),
      `table`, typed fields with `pk` and `enum(...)`, `emits`, `consumes`
- [x] `metaarch-parser`: hand-rolled lexer + recursive descent, positioned errors
- [x] `metaarch-spec`: typed `SystemSpec` + validation (see [validation](validation.md))
- [x] CLI: `bin/main check` / `dump`; `generate` validates then bails

**Demo:** `bin/main check examples/shop.arch` rejects broken architectures
with line/col diagnostics.

## Phase 1 — generation MVP ✅

The first end-to-end `generate`: one event-driven system, runnable locally.

- [x] Add quilt to `metaarch-codegen` (git dep on the pinned `v0.5.0` tag,
      `default-features = false`, like nanobots) and wire `bin/expand`
- [x] Generator metaprograms as `.rs.quilt` sources in `metaarch-codegen`,
      expanded to `.rs` siblings (gitignored) and compiled into the crate:
  - [x] **Rust service** — axum skeleton per `lang rust` service: health
        endpoint, typed event structs (serde) for its `emits`/`consumes`
  - [x] **SQL schema** — DDL per `db` block (postgres + sqlite dialects from
        the same tables; `enum` → `CHECK` constraint)
  - [x] **Python service** — stdlib-only package per `lang python` service
        with typed event dataclasses
  - [x] **Event bus** — MVP transport: HTTP POST fan-out from emitter to each
        consumer's `/events/<Name>` endpoint (no broker to deploy; swappable
        later)
- [x] Generated-system layout mirrors this repo (see [codegen](codegen.md)):
      `nix/` env, `bin/main` boot script, one directory per service
- [x] `metaarch generate` writes `out/<system>/` and prints what it made

**Demo (works):** `bin/main generate examples/shop.arch && cd out/shop &&
bin/main` boots the shop; `curl -X POST 127.0.0.1:8081/emit/OrderPlaced -d
'{"order_id": "…", "total": 4999}'` makes the Python notifier print the
typed event.

## Phase 2 — the full artifact fan-out ✅

Widen what one `.arch` line touches. Each item is a new generator over the
same `SystemSpec`; phase 1's determinism is what makes the diff demo work:

- [x] Typed Rust **clients** for every service's API (`src/clients.rs`: one
      struct per peer — `health()`, `emit_x`/`deliver_x` typed by the shared
      `events` module); every Rust service, the gateway included, uses them
      to serve `GET /peers`, a fleet health check through the typed clients
- [x] **Migrations**: diff the previous generated spec (the `system.arch`
      snapshot `generate` leaves in the output root), emit numbered
      `sql/migrations/NNNN.sql` with `CREATE`/`DROP TABLE` and
      `ADD`/`DROP COLUMN` steps; type changes become `-- manual migration
      required` comments
- [x] **HTML docs**: `docs/index.html` — topology table + per-service
      route/table reference + event contracts with sample payloads (quilt's
      html target)
- [x] **Smoke tests**: generated `bin/smoke` boots the fleet and curls every
      route the spec derives — health, peers, emit, events — with the
      canonical sample payloads (quilt's bash target)
- [x] Seed data per table: `sql/seed.sql`, three deterministic rows

**Demo (works):** add `coupon_code: text` to the `orders` table and
`OrderPlaced`; one `generate` run diffs Rust events (both services), the
Python dataclass, SQL schema + seed, docs, `bin/smoke`, and emits
`ALTER TABLE orders ADD COLUMN` in `sql/migrations/0001.sql`. Reverting
emits `0002.sql` with the `DROP COLUMN`. `bin/smoke`: 7/7 routes pass.

## Phase 3 — Nix deployment ✅

The deployment is *generated Nix*, not hand-written: quilt `nix↖…↗` target
quotes in the established `.rs.quilt` pattern (see the decisions log for why
not a `.nix.quilt` host file). This replaced phase 1's stopgap plain-text
dev shell.

- [x] `nix.rs.quilt` generator emitting generated Nix from the spec:
  - [x] **Root `flake.nix`** — one buildable package per service
        (`nix build .#orders`: rust via `buildRustPackage` + the emitted
        `Cargo.lock`, python via `writeShellApplication`), `nix run` apps
        per service, and a `fleet` app (default) that boots every *built*
        service with the same die-together semantics as `bin/main`
  - [x] **`nix/flake.nix` dev shell** — toolchains + db engines + curl;
        what the unchanged `.envrc` loads
  - [x] **Per-service NixOS modules** (`nix/modules/<svc>.nix`, exported as
        `nixosModules.<svc>`): `services.<system>.<svc>.enable` runs the
        flake-built package as a hardened systemd unit (containers via
        `dockerTools` stay a stretch item)
- [x] Emitted **`Cargo.lock`** from a vendored canonical lock — the closed
      dependency set means one lock is valid for every generated workspace;
      `cargo build --locked` passes and nix builds are pure
- [x] **Database provisioning**: generated `bin/db` — postgres `initdb` +
      unix-socket-only server under `.pgdata/<svc>`, schema + seed applied
      on first creation, idempotent `up` / `down`; sqlite files likewise.
      `bin/main` runs `bin/db up` before booting the fleet

**Demo (works):** `cd out/shop && direnv allow && bin/main` boots the fleet
with a provisioned, seeded postgres; `nix build .#orders` compiles the
service hermetically from the emitted lock and the binary serves `/health`;
`nix flake show` lists apps, packages, dev shell, and three NixOS modules.
`bin/smoke`: 7/7 routes pass.

## Phase 4 — quilt integration (the endgame; in progress)

Make the DSL a first-class quilt language so `.arch` files can carry inline
fragments of other languages for fine-grained control of the generated code
(arch is *not* folded into quilt itself — see the dynamic-registration
decision below):

- [x] Escape hatch first: `impl get|post /path ↖ … ↗` service entries — an
      HTTP route whose handler body is an inline fragment in the service's
      own language, carried as an opaque dedented string, spliced into the
      generated service, and covered by docs rows, smoke checks, and route
      validation
- [ ] Register arch as a *dynamic* quilt language: quilt grows a
      `Box<dyn Language>` registration hook (local experiments only — never
      pushed to quilt from here), and metaarch implements the trait and
      hooks arch in from its side, so `.arch.quilt` files parse and arch
      fragments can be quoted from host metaprograms
- [ ] Inline quotes inside `.arch`: `rust↖ ... ↗` bodies on services/handlers,
      spliced into the generated code at expansion time — replacing the
      string escape hatch with real ASTs
- [ ] tree-sitter-arch grammar + LSP wiring for highlighting and diagnostics
      in editors (quilt-lsp multiplexes the embedded languages)

**Demo (4a works):** `examples/shop.arch` gives the gateway
`impl get /hello ↖ "hello from an inline fragment!\n" ↗` and the notifier a
Python `/stats` route; one `generate` wires both into router/dispatch, docs,
and `bin/smoke` — 9/9 routes pass.

**Demo (endgame):** an `.arch` file where one endpoint's body is written
inline in Rust between arrow brackets, type-checked in place by the LSP.

## Stretch

- WGSL analytics service: a generator that emits compute shaders specialized
  to a table's schema (quilt's wgsl target) — GPU-accelerated aggregation
  from the same `.arch` source
- Architecture visualizer: generated HTML/SVG topology diagram
- `metaarch fmt` for `.arch` files
- Container images per service (`dockerTools.buildLayeredImage` in the root
  flake) — the NixOS modules cover deployment for now

## Decisions log

Every decision here is also filed as a GitHub issue with the `decision`
label (issues #1–#20 as of 2026-07-19); new decisions get both an entry
here and an issue.

- **Standalone parser first, quilt `Language` later** (2026-07-18): start
  with a hand-rolled parser so the DSL ships without touching quilt;
  integrate in phase 4 for inline languages.
- **Closed type set** (2026-07-18): every field type must have a known
  mapping in every target language; unknown types are parse errors.
- **HTTP fan-out before a broker** (2026-07-18): phase 1 needs no deployed
  infrastructure; the transport is behind a generator, so swapping in a real
  broker later touches no `.arch` files.
- **Generated systems mirror this repo's shape** (2026-07-18): `nix/` env +
  `bin/main` entrypoint + per-service directories, so navigating a generated
  system feels like navigating metaarch itself.
- **Deterministic fallback ports** (2026-07-18): a service without `port`
  listens on `9000 + its index` in the file. Every service gets an address,
  so consumers are reachable without forcing `port` into every `.arch` file.
- **`/emit/<Event>` trigger routes** (2026-07-18): the DSL has no endpoint
  surface yet, so each emitter exposes `POST /emit/<E>` — parse the typed
  event, fan it out. It is the only honest derivable "business" route until
  phase 4 inline handlers, and it makes every event path curl-able.
- **Phase-1 type spellings** (2026-07-18): `uuid`/`timestamp` travel as
  strings (no uuid/chrono deps in generated services), `money` is integer
  minor units, `enum` is TEXT + CHECK in both SQL dialects. All seven types
  keep one spelling per target in `metaarch-codegen/src/lib.rs`.
- **`system.arch` snapshot as migration base** (2026-07-18): `generate`
  copies the source `.arch` byte-for-byte into the output root; the next run
  reparses it with the ordinary parser and diffs specs. No snapshot format,
  no SQL parsing, and the `.arch` file stays the only source of truth.
  Renames read as drop + add.
- **Automate only safe ALTERs** (2026-07-18): added/dropped tables and
  columns become DDL (`ADD COLUMN` drops NOT NULL — existing rows need a
  backfill first); anything lossy or engine-fragile (type/pk/enum changes,
  engine swaps) is emitted as a `-- manual migration required` comment
  rather than guessed at.
- **One canonical sample value per type** (2026-07-18): seeds, smoke
  payloads, and docs examples all derive from the same deterministic
  per-type values (`sample_json` / `sql_literal`), so every artifact quotes
  the same example data.
- **Clients pull the whole event set** (2026-07-18): a Rust service's
  `events.rs` includes every event its typed clients can carry, not just its
  own emits/consumes — the client API is typed by the system-wide contract.
  `GET /peers` exists so the clients are exercised (and compile-checked) by
  generated code, not just offered.
- **Rust-host nix generator, not a `.nix.quilt` host file** (2026-07-19):
  generators take a `SystemSpec` at runtime, but a `.nix.quilt` file expands
  once at build time — it can't consume a spec. So phase 3 is `nix.rs.quilt`
  with `nix↖…↗` *target* quotes, matching every other generator; quilt's
  string-based Nix host stays on the table for phase 4.
- **Two flakes per generated system** (2026-07-19): `nix build` only sees
  sources inside the flake directory, so the buildable packages, apps, and
  NixOS modules live in a *root* `flake.nix`; the dev shell stays at
  `nix/flake.nix` (the mirror-this-repo shape, unchanged `.envrc`, and
  day-to-day direnv reloads never copy the source tree).
- **Generated Nix uses three splice positions** (2026-07-19): expression
  values, lifted strings, variadic list interiors — the ones quilt's nix
  target has covered by tests. Spec-named attrsets go through
  `builtins.listToAttrs`; NixOS modules bind names in a `let` and select
  with `${name}`; multi-line shell text is lifted whole from a ground
  string. No identifier or attrset-key position is ever spliced.
- **Vendored canonical `Cargo.lock`** (2026-07-19): every generated Rust
  service now has the identical dependency set (serde made unconditional),
  so one lock — captured once, registry entries vendored under `assets/`,
  member entries merged per spec — is emitted into every system. Dev builds
  pass `--locked`, and it is what makes `nix build` of a service pure.
- **quilt pinned to a rev, not a tag** (2026-07-19): the Nix lift marker
  landed after `v0.5.0`, so the pin moved to `ba27c41` (the sibling
  checkout `bin/expand` uses, keeping expander and runtime matched). Return
  to a tag at the next quilt release.
- **`impl` routes carry method + path** (2026-07-19): the plan's "attach a
  fragment to a service/endpoint" needs an endpoint surface the DSL didn't
  have, so the escape hatch *is* the endpoint surface:
  `impl get /hello ↖ … ↗`. A bare named handler would be dead code; a route
  is curl-able, smoke-checkable, and documentable on day one.
- **Fragments are handler bodies, appended as text** (2026-07-19): the
  fragment is the *body* of the handler (Rust: tail expression of an
  `impl IntoResponse` fn; Python: function body returning the response
  text), dedented at lex time. Generators append these handlers to the
  output as plain text — honest about phase 4a's opaque-string carrier —
  and wire them up by generated name (`impl_<method>_<path>`), which
  validation keeps collision-free and off the derived routes.
- **arch stays out of quilt; languages register dynamically** (2026-07-19,
  user decision): quilt must not hardcode the arch language. Instead quilt
  gets a `Box<dyn Language>` dynamic-registration hook and metaarch
  registers arch through it. Any quilt-side changes are prototyped locally
  and never pushed from this project.
- **`bin/db` provisions fresh, migrations stay manual** (2026-07-19):
  `bin/db up` applies schema + seed only when it creates the database;
  migrations under `sql/migrations/` target *pre-existing* databases and
  applying them needs bookkeeping (a schema-version table) the generated
  systems don't have yet. Postgres runs unix-socket-only under
  `.pgdata/<svc>` — nothing to collide with, nothing listening on TCP —
  and outlives the fleet (`bin/main` starts it via `bin/db up`; `bin/db
  down` stops it).
