# Validation

The compiler slogan: **invalid architectures don't generate.** The parser
guarantees syntax; the validator (`metaarch_spec::validate`) guarantees the
*system* makes sense before a single line of code is emitted. This is where
metaarch earns the word "compiler" — the same class of guarantees a type
checker gives a program, applied to a topology.

Validation runs over the typed `SystemSpec`, so every check is an ordinary
Rust function over ordinary data. Diagnostics carry the source span of the
offending item and print as:

```
error: service `billing` reuses port 8081, already taken by `orders` (line 14, col 9)
warning: event `OrderShipped` is emitted but never consumed (line 22, col 9)
```

`bin/main check` prints all diagnostics; `generate` refuses to run while any
error-severity diagnostic exists. Warnings don't block.

## Current checks (v0)

Identity and uniqueness:

- duplicate service names
- duplicate table names within a service; duplicate field names within a
  table or event
- duplicate enum variants
- event names are globally unique (one event, one emitter)

Topology:

- every `consumes` resolves to an event some service `emits` (no dangling
  subscriptions)
- no two services claim the same port
- *warning:* an event emitted but never consumed
- *warning:* a service with no port, db, emits, or consumes (it does nothing)

Structure:

- every service declares a `lang`
- every table has exactly one `pk` field
- `pk` is rejected on event fields

Impl routes (phase 4a):

- an `impl` route must not collide with a route the generators derive
  (`GET /health`, `GET /peers`, `POST /emit/<E>`, `POST /events/<E>`)
- no duplicate method+path per service; no two impl routes may flatten to
  the same generated handler name (`/a/b` vs `/a_b`)
- *warning:* an empty fragment

Some overlapping checks live in the parser instead, where the error message
is better served by syntax position: duplicate `lang`/`port`/`db` entries,
unknown types, unknown languages, out-of-range ports.

## Planned checks

As the DSL grows (see [plan](plan.md)), each new construct lands with its
checks in the same commit:

- foreign-key/reference fields → referent table exists, type matches its pk
- synchronous service calls → call graph is acyclic (async event cycles stay
  legal; sync cycles are deadlock bait)
- reserved names: identifiers must be valid in *every* target — a table named
  `order` must survive SQL, a field named `type` must survive Rust and Python
- event evolution (phase 2 migrations): a regenerated event must be
  compatible with consumers' previous shape, or the diff is flagged
- per-engine limits: e.g. sqlite has no native enum; the validator knows
  what each engine's generator can honor

## Philosophy

- **Errors are for humans reading one file.** Every diagnostic names the
  `.arch` construct at fault with its position — never a generated file, and
  never a downstream compiler error a user has to trace back by hand. Rule of
  thumb: if a broken `.arch` file can produce broken *generated* code, the
  missing check is a validator bug.
- **Warnings are for suspicious-but-legal.** Dead events and inert services
  generate fine; they're still almost certainly mistakes.
- **The spec stays data.** Checks take `&SystemSpec` and return diagnostics —
  no I/O, no generation coupling — so the whole validator is unit-testable
  (see `metaarch-spec/src/validate.rs` tests).
