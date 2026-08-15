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

## Phase 4 — quilt integration (the endgame) ✅

Make the DSL a first-class quilt language so `.arch` files can carry inline
fragments of other languages for fine-grained control of the generated code
(arch is *not* folded into quilt itself — see the dynamic-registration
decision below):

- [x] Escape hatch first: `impl get|post /path ↖ … ↗` service entries — an
      HTTP route whose handler body is an inline fragment in the service's
      own language, carried as an opaque dedented string, spliced into the
      generated service, and covered by docs rows, smoke checks, and route
      validation
- [x] Register arch as a *dynamic* quilt language — and it took **zero
      quilt changes**: the `Box<dyn Language>` hook already exists at the
      pinned rev (`DictMulti::add_lang`). `metaarch-lang` implements the
      `Language` trait by hand (flat-node lexer + recursive descent to
      `QTerm`, trivia-preserving, holes at name/value/entry/field/fragment
      positions); `metaarch-expand` registers it beside the built-in set and
      now drives `bin/expand` (its output is byte-identical to the quilt
      CLI's). `.arch.quilt` files parse — with `impl` fragments as real
      parsed `rust↖…↗` quotes — and host metaprograms can quote and splice
      arch (`arch↖system ↙name↘ …↗`)
- [x] Real-AST fragments: `impl` bodies (optionally annotated
      `rust↖ … ↗`/`python↖ … ↗`, the `.arch.quilt` quote spelling, checked
      against the service's `lang`) are parsed with the real tree-sitter
      grammars — `metaarch-codegen`'s new `fragment` module, quilt's `parse`
      feature — and spliced into the generated handlers as terms, replacing
      the 4a text-append; a malformed fragment now fails `metaarch check`
      with a positioned diagnostic. Generated output for the shop is
      byte-identical to 4a's
- [x] An arch `MetaLanguage` — again zero quilt changes (`DictMulti::add_meta`
      is the hook). arch is a *data* language: it stages no computation, so
      expanding an arch host is an identity rebuild. The piece that makes
      quilt's expander agree is in the parser: quote plugs are demoted to
      coparse-identical plain tuples (a `rust↖…↗` fragment is carried syntax,
      not staged code), so nothing quote-shaped remains for the ground
      expander to evaluate — and the registered `ArchMetaLanguage`'s hooks
      are unreachable by construction (each one errors, explaining why;
      `↑`/`↓` refuse outright — arch has no runtime). `metaarch-expand
      expand` now handles `.arch.quilt` files (bin/expand is uniform again,
      writing `examples/hello.arch`), and the generate path converges on the
      registry: the CLI loads `.arch.quilt` directly — registry parse →
      expand → coparse → ordinary parser → `SystemSpec` — and round-trips
      fully annotated plain `.arch` files through the registry as a
      convergence assertion
- [x] tree-sitter-arch grammar + LSP wiring for highlighting and diagnostics
      in editors. quilt-lsp could *not* multiplex arch — its adapter
      registries are static matches with no `add_lang`-style hook — so, the
      `metaarch-expand` move again, the repo grew its own: `tree-sitter-arch`
      (a deliberately loose grammar with committed generated parser, corpus
      tests, highlight + injection queries; `bin/grammar` regenerates) and
      `metaarch-lsp` (diagnostics are exactly the `metaarch check` pipeline;
      semantic tokens come from the arch grammar with each `impl` fragment
      interior re-highlighted by its own language's grammar — the in-process,
      highlight-only half of quilt-lsp's embedded multiplexing). See
      [lsp](lsp.md) for editor wiring

**Demo (4a works):** `examples/shop.arch` gives the gateway
`impl get /hello ↖ "hello from an inline fragment!\n" ↗` and the notifier a
Python `/stats` route; one `generate` wires both into router/dispatch, docs,
and `bin/smoke` — 9/9 routes pass.

**Demo (4b works):** `cargo run -p metaarch-expand -- parse
examples/hello.arch.quilt` parses an `.arch.quilt` file whose impl body is a
real tree-sitter-parsed `rust↖…↗` quote, through the same dynamic registry
that `bin/expand` now uses for every generator.

**Demo (4c works):** `examples/shop.arch` spells the gateway's fragment
`impl get /hello rust↖ … ↗`; `check` rejects `let x = ;` inside the brackets
with a tree-sitter-backed positioned error, and `generate` splices the
parsed expression term into the handler — emitting bytes identical to the
4a text-append for the whole shop. `bin/smoke`: 9/9 routes pass.

**Demo (4d works):** `bin/main generate examples/hello.arch.quilt` builds the
hello system straight from the quilt file — the spec is derived from the
expanded term's coparse, and the `system.arch` snapshot it leaves is plain
arch, so the migration diff keeps working on the next run. `bin/expand` now
expands `examples/hello.arch.quilt` to `examples/hello.arch` like any other
`.quilt` source, and `bin/main check` accepts source, sibling, and shop
alike. Shop's generated output is byte-identical to 4c's. (One caveat,
inherited from quilt's quote-body reindenting: a block-opened fragment's
closing `↗` coparses flush-left, so only inline-bodied files round-trip
byte-for-byte — semantically identical either way.)

**Demo (4e works):** `cargo test -p metaarch-lsp --test lsp` drives the real
`metaarch-lsp` binary over stdio: opening a broken `.arch` file publishes a
positioned diagnostic (the same error `metaarch check` prints), fixing it
clears the squiggle, and `semanticTokens/full` on `examples/shop.arch`
returns a full highlight — arch keywords/names from `tree-sitter-arch`, the
gateway's fragment interior as Rust, the notifier's as Python (defaulted
from `lang`, no annotation needed). `bin/grammar`: 8/8 corpus tests pass.

**Demo (endgame, reached modulo type-checking):** an `.arch` file where one
endpoint's body is written inline in Rust between arrow brackets — parsed
with the real Rust grammar (`check` rejects malformed bodies in place),
highlighted in place by the LSP. Full *type*-checking in place means
proxying rust-analyzer over a projected fragment, which wants the upstream
quilt-lsp hook — logged in the decisions below.

## Stretch

- [x] WGSL analytics: `<svc>/analytics/<table>.wgsl` — a compute shader per
      table specialized to its schema (quilt's wgsl target), aggregating the
      numeric columns (sum for `int`/`money`, true-count for `bool`) with a
      row-count, ready for any wgpu host to bind and dispatch. Emitted
      shaders parse with quilt's wgsl grammar (asserted by test) and pass
      full naga validation
- [x] Architecture visualizer: `docs/topology.svg` — service boxes
      (lang-accented), an arrow lane per event from emitter to every
      consumer, a cylinder per database — generated beside the docs page
      and inlined into its Topology section
- [x] `metaarch fmt` for `.arch` files: `bin/main fmt [--check]` re-prints
      a file in the canonical style (two-space indent, width-80 inline/block
      field lists, trailing commas in block lists) while preserving comments,
      blank-line groupings, and each fragment's block/inline spelling;
      unparseable files are refused untouched
- [x] Container images per service: `packages.<svc>-image` in the root
      flake (`nix build .#orders-image` → a loadable `docker load` tarball
      via `dockerTools.buildLayeredImage`, the service's closure as layers).
      Like the NixOS modules they build on Linux; a darwin host wants a
      linux builder
- [x] **process-compose instead of containers** (#41): a generated
      `process-compose.yaml` — one process per service (`cargo run` /
      `python3 -m`), the `bin/db up` and `cargo build` one-shots they
      `depends_on`, an HTTP readiness probe on each service's `/health`, and
      restart-on-failure. `bin/main --process-compose` runs it; `nix run
      .#fleet-pc` runs the same fleet over the *built* services from a
      `builtins.toJSON` config in the store. `pkgs.process-compose` joins
      the dev shell

## Decisions log

Every decision here is also filed as a GitHub issue with the `decision`
label (issues #1–#31, #33, #34, #36, #37, #39 and #41 as of 2026-08-09); new
decisions get both an entry here and an issue.

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
  keep one spelling per target in `metaarch-codegen/src/lib.rs` — except
  SQL's, which moved to `sql.rs.quilt` when the DDL became quoted: it is a
  type *node* there, not a string.
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
  to a tag at the next quilt release. Moved on to `9348f65` (2026-08-15) for
  the SQL target — re-expansion of every existing generator was
  byte-identical across the bump.
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
- **The hook already existed — `metaarch-expand` replaces the sibling CLI**
  (2026-07-19): at the pinned rev, `DictMulti::add_lang` *is* the
  `Box<dyn Language>` hook, so 4b shipped with zero quilt changes. What was
  actually missing was a driver: quilt's stock CLI is hardwired to the
  closed `Omni` set, so the workspace grew `metaarch-expand` (~60 lines of
  library glue) and `bin/expand` now runs it — proven by byte-identical
  re-expansion of every existing generator.
- **The arch `Language` is syntax; `metaarch-spec` stays the semantics**
  (2026-07-19): the quilt-side parser (`metaarch-lang`) is deliberately
  loose — any identifier parses as a type — and produces a trivia-preserving
  `QTerm` with splice holes. The closed type set, topology checks, and
  `SystemSpec` remain in `metaarch-parser`/`metaarch-spec`, which validate
  the expanded/coparsed output. Two parsers, two jobs; they converge when
  the spec is derived from the QTerm (now the 4d MetaLanguage bullet).
  Known 4b limits, revisited there: arch has no `MetaLanguage` yet (so
  `.arch.quilt` is parse-only — no expansion), and `.↑` lifts into arch are
  impossible because quilt's rust meta keys lift spellings statically per
  target.
- **Fragments parse where the grammars live — codegen, not spec** (2026-07-19):
  the parser and spec crates stay quilt-free; `metaarch-codegen` (already
  the only quilt-runtime consumer) enables quilt's `parse` feature and
  parses `impl` fragments with the same rust/python `Language`
  implementations quilt's own quotes use — Rust fragments as an *expression*
  (the handler's tail), Python fragments as a statement suite, per the 4a
  fragments-are-handler-bodies decision. `check_fragments` runs beside
  `validate` in `metaarch check`, so a malformed fragment fails `check`
  (never `generate`) and generators treat "fragment parses" as one more
  established invariant. The spec still carries the fragment as a dedented
  string: the `.arch` file stays the source of truth, and the string is
  what the snapshot/migration path already round-trips.
- **Optional fragment language annotation, checked against `lang`**
  (2026-07-19): plain `.arch` accepts `impl get /x rust↖ … ↗` — the same
  spelling a `.arch.quilt` quote uses, so a service body can move between
  the two files unchanged. The annotation is optional (the service's `lang`
  is the only possible default) and validation rejects a mismatch, because
  a fragment is always spliced into its own service's generated code.
- **arch quotes are data — demoted to plain tuples at parse** (2026-07-19):
  quilt's ground expander evaluates a quote into host code and drops its
  brackets — right for computational hosts, wrong for a data language whose
  quotes *carry* syntax. `ArchPost::parse_post` therefore demotes every
  quote plug to a coparse-identical plain tuple, so a well-formed arch host
  contains nothing staged: `expand_lang("arch", …)` is an identity rebuild,
  and the registered `ArchMetaLanguage`'s hooks are unreachable by
  construction (each errors descriptively if a staged construct ever
  arrives; `↑`/`↓` refuse — arch has no runtime to lift from or reduce
  with). Ground `↙…↘` splices are already rejected at parse by quilt's
  unquote-depth check.
- **`.arch.quilt` loads via expand → coparse → ordinary parser**
  (2026-07-19): the quilt registry owns quilt syntax; `metaarch-parser` /
  `metaarch-spec` stay the semantic authority (closed types, topology,
  positioned diagnostics). The CLI derives the spec from the expanded
  term's coparse and `generate` snapshots that plain text as `system.arch`,
  so the migration diff keeps working when the source of truth is a
  `.arch.quilt` file.
- **Registry convergence requires annotated fragments** (2026-07-19): quilt
  resolves an un-annotated quote's language from the file-extension chain,
  which a plain `.arch` file doesn't provide — an un-annotated fragment
  body would parse as arch and fail. So the plain-`.arch` path asserts
  registry/hand-parser agreement only when every `impl` fragment carries
  its annotation; `shop.arch`'s python route deliberately stays
  un-annotated to keep the optional-annotation feature exercised. A
  quilt-side "ask the outer language for the default inner language" hook
  would lift the limit.
- **quilt-lsp has no dynamic hook — metaarch grows its own LSP** (2026-07-19):
  4b was free because `DictMulti::add_lang` already existed; quilt-lsp's
  registries (`is_known_lang`, `language_adapter`, `meta_adapter`,
  `highlighter`) are static matches with nothing to register into. Per the
  arch-stays-out-of-quilt decision, the repo grew `metaarch-lsp` instead —
  and its diagnostics are *exactly* the `metaarch check` pipeline (parse →
  `validate` → `check_fragments`), so the editor and the CLI can never
  disagree. `.arch.quilt` files keep quilt's own syntactic support only
  until an upstream adapter-registration hook exists.
- **tree-sitter-arch is loose, vendored, and committed** (2026-07-19): the
  editor grammar only segments the file (any identifier parses as a
  type/method/language — the closed sets stay in `metaarch-parser`/
  `metaarch-spec`, mirroring the `metaarch-lang` decision), and its generated
  `src/parser.c` is committed like quilt's vendored grammars so `cargo build`
  never needs the tree-sitter CLI. `bin/grammar` (CLI + node, now in the dev
  shell) regenerates and runs the corpus tests.
- **Fragment highlighting is in-process and highlight-only** (2026-07-19):
  `metaarch-lsp` re-highlights each `impl` fragment interior with the rust/
  python tree-sitter grammars quilt vendors — the annotation if present,
  else the service's `lang`, the generators' own default. No downstream
  rust-analyzer/pyright proxying: fragments are handler *bodies*, not
  standalone files, so a proxied server would mostly report false context
  errors; real in-place type-checking wants quilt-lsp's projection machinery
  behind an upstream hook. The nvim injection query covers annotated
  fragments only (a query can't reach the sibling `lang` entry); un-annotated
  ones are covered by the LSP semantic tokens.
- **Bare `bin/main` runs the demo tour** (2026-07-19): with no arguments the
  entrypoint execs `bin/demo` — a six-act guided tour (source file → rejected
  broken copy → fmt → generate with artifact highlights → one-line-change
  migration diff → fleet boot + 9/9 smoke inside the generated dev shell);
  any argument still reaches the CLI unchanged. The tour only exercises the
  public commands (nothing demo-only in the CLI), works on out/shop so
  repeat runs stay warm, does its mutation experiments in a mktemp dir, and
  skips the live boot with an explanation when `nix` is absent.
- **Analytics ships shaders, not a wgpu service** (2026-07-19): the WGSL
  stretch item emits `analytics/<table>.wgsl` files, and the generated
  systems stay wgpu-free — a GPU host would explode the closed dependency
  set behind the vendored canonical `Cargo.lock`. The schema reaches the
  shader through the wgsl target's covered splice positions only
  (expressions, statements, heterogeneous lifts — struct members and
  bindings are identifier positions with no tested splices): the buffer
  layout is a fixed row-major `i32` array with a lifted `N_COLS`, column
  selection (`int`/`money`/`bool`; `bool` as 0/1) specializes per table, and
  the column→index mapping is a ground comment header. Sums are `i32` —
  WGSL atomics have no 64-bit variant. Tables with no numeric columns get
  no shader. The codegen tests parse every emitted shader with quilt's own
  wgsl `Language` (the `wgsl` feature joins `parse`/`python`/`rust`), and
  the shop's shader passes full naga validation.
- **Container images are ground Nix in the root flake** (2026-07-19): the
  `<svc>-image` packages are one static `builtins.listToAttrs (map …)` block
  over the already-spliced `services` attrset — `dockerTools.buildLayeredImage`
  with `config.Cmd = [ "${services.${name}}/bin/${name}" ]`, so the image
  layers are exactly the service's closure (the python services' flake-source
  store path rides along via the closure, nothing hand-listed). Zero new
  splice positions: the whole block is ground Nix inside the existing quote,
  keeping the three-splice-positions rule intact. Images build where Linux
  derivations build — on darwin that means a linux builder, same as the
  NixOS modules.
- **`fmt` is the parser's grammar walk over a comment-keeping token stream**
  (2026-07-19): the lexer now emits `#` comments as tokens (the parser
  filters them out; the formatter is who asks for them), and `fmt.rs`
  mirrors the parser function-for-function, re-printing tokens instead of
  building a spec — so there is no third grammar and no CST to maintain, and
  a `formatting_preserves_the_spec` test (span-stripped spec equality) keeps
  the walk honest. `format` runs the real parser first, so fmt can never
  mangle a file it doesn't understand. Field-list layout is fmt's to choose
  (inline iff the line fits 80 columns); a fragment body's block/inline
  spelling is the author's — it is foreign code, and the spec dedents it
  anyway, which is also why re-indenting bodies is semantics-preserving.
- **The topology diagram is plain-Rust SVG, standalone + inlined** (2026-07-19):
  the visualizer builds its SVG as ground strings in `viz.rs` (the `sql.rs`
  precedent), not as a quilt quote — every spec-driven value in an SVG lands
  at *attribute* position (coordinates, sizes, the viewBox), and the html
  target's covered splice position is text interiors (`raw_text`), per the
  only-tested-positions rule the nix generator set. One `<svg>` element
  serves both artifacts: written standalone as `docs/topology.svg` and
  spliced into `docs/index.html`'s Topology section, so the page needs no
  file fetch and the file needs no page. Layout is integer arithmetic over
  declaration order — deterministic like every other artifact.
- **process-compose is a fourth runner, not a replacement** (2026-08-09,
  #41): `process-compose.yaml` is plain text in `process_compose.rs` (no
  quilt YAML grammar — the `sql.rs` precedent), and `bin/main` reaches it
  only behind `--process-compose`, so the default boot path keeps needing
  nothing but bash. Three things were decided against: (a) the event
  topology does **not** become `depends_on` edges — nothing in validation
  forbids two services consuming each other's events, and a cycle would
  deadlock `process-compose up` on a system `check` accepts, so only the
  `bin/db up` / `cargo build` one-shots are dependencies and readiness
  probes carry the health story; (b) process-compose's REST API is put on a
  unix socket (`-U`), because its TCP default is `:8080` — a port a
  generated service may itself want (the shop's gateway does) — and the
  socket path is pinned by the `.envrc` (`PC_SOCKET_PATH`), since
  process-compose otherwise names it after the *calling* process and a
  second terminal cannot reach the fleet; (c) the store-path runner
  (`nix run .#fleet-pc`) reuses the probe/restart constants from
  `process_compose.rs` rather than restating them, and its config is
  `builtins.toJSON` of a ground-Nix attrset — JSON is YAML, so no YAML is
  spelled in Nix and the three-splice-positions rule holds.
- **`bin/db` provisions fresh, migrations stay manual** (2026-07-19):
  `bin/db up` applies schema + seed only when it creates the database;
  migrations under `sql/migrations/` target *pre-existing* databases and
  applying them needs bookkeeping (a schema-version table) the generated
  systems don't have yet. Postgres runs unix-socket-only under
  `.pgdata/<svc>` — nothing to collide with, nothing listening on TCP —
  and outlives the fleet (`bin/main` starts it via `bin/db up`; `bin/db
  down` stops it).
- **SQL is quoted, not built** (2026-08-15, issue #43): quilt gained a SQL
  target (QuiltLang/quilt#219, #234), so `sql.rs`, `seed.rs` and
  `migrations.rs` became `.rs.quilt` metaprograms like every other
  language-emitting generator. Every statement is a `sql↖…↗` quote and every
  spec value crosses in through a hole or a lift, which is what retires the
  two string habits the text version had: `'{v}'` around enum variants and
  seed strings (a variant holding an apostrophe used to produce SQL that
  ended the literal early), and `column_def(…).replace(" NOT NULL", "")` for
  the `ADD COLUMN` spelling. Two positions the grammar gives no hole — a
  column definition standing alone, and the `column_definitions` list
  interior — are handled the way the nix and rust generators handle theirs:
  the definitions come out of one-column `CREATE TABLE` quotes (the
  wrap-and-strip trick `SqlLanguage::parse_pre` uses for bare expressions),
  and only the list wrapper is a term builder. Output is byte-identical to
  the text generators', and each generator now reparses what it emits with
  quilt's own SQL grammar.
