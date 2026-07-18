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

## Phase 2 — the full artifact fan-out

Widen what one `.arch` line touches. Each item is a new generator over the
same `SystemSpec`:

- [ ] Typed Rust **clients** for every service's API; the gateway uses them
- [ ] **Migrations**: diff the previous generated schema, emit `ALTER` steps
- [ ] **HTML docs**: system topology page + per-service API/event reference
      (quilt's html target)
- [ ] **Smoke tests**: generated bash scripts that boot the system and
      exercise every endpoint and event path (quilt's bash target)
- [ ] Seed data generators per table

**Demo:** add one field to `OrderPlaced`; show the diff touching Rust, Python,
SQL, docs, and tests in one `generate` run.

## Phase 3 — Nix deployment

Lean into quilt's string-based Nix host: the deployment is *generated Nix*,
not hand-written.

- [ ] `.nix.quilt` generator emitting the generated system's `nix/flake.nix`:
      a dev shell with each service's toolchain, plus `nix run` apps per
      service and a process-compose/`bin/main` orchestrator for the fleet
- [ ] Per-service NixOS modules / containers as a `nix build` target
- [ ] Database provisioning (postgres init + schema apply) in the generated env

**Demo:** `cd out/shop && direnv allow && bin/main` — a reproducible boot of
the whole fleet on a clean machine.

## Phase 4 — quilt integration (the endgame)

Fold the DSL into quilt itself so `.arch` files can carry inline fragments of
other languages for fine-grained control of the generated code:

- [ ] Escape hatch first: an `impl` block in the DSL attaching a raw code
      fragment to a service/endpoint, carried as an opaque string
- [ ] Implement quilt's `Language` trait for arch (no tree-sitter needed —
      the bootstrap language shows the pattern), so `.arch.quilt` files parse
      and arch fragments can be quoted from host metaprograms
- [ ] Inline quotes inside `.arch`: `rust↖ ... ↗` bodies on services/handlers,
      spliced into the generated code at expansion time — replacing the
      string escape hatch with real ASTs
- [ ] tree-sitter-arch grammar + LSP wiring for highlighting and diagnostics
      in editors (quilt-lsp multiplexes the embedded languages)

**Demo:** an `.arch` file where one endpoint's body is written inline in Rust
between arrow brackets, type-checked in place by the LSP.

## Stretch

- WGSL analytics service: a generator that emits compute shaders specialized
  to a table's schema (quilt's wgsl target) — GPU-accelerated aggregation
  from the same `.arch` source
- Architecture visualizer: generated HTML/SVG topology diagram
- `metaarch fmt` for `.arch` files

## Decisions log

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
