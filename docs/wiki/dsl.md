# The .arch DSL

A `.arch` file describes one distributed system: its services, their
databases, and the events that connect them. It is deliberately small — the
DSL carries *architecture*; everything mechanical is derived by the
generators, and (in phase 4) anything bespoke is written inline in a real
language rather than grown into DSL features.

## Example

```text
# Comments run from `#` to end of line.
system shop

service orders {
  lang rust
  port 8081
  db postgres {
    table orders {
      id: uuid pk,
      total: money,
      status: enum(pending, paid, shipped),
    }
  }
  emits OrderPlaced { order_id: uuid, total: money }
}

service notifier {
  lang python
  consumes OrderPlaced
}
```

## Grammar (v0)

```ebnf
system   = "system" ident service* ;
service  = "service" ident "{" entry* "}" ;
entry    = "lang" ("rust" | "python")
         | "port" integer
         | "db" ("postgres" | "sqlite") "{" table* "}"
         | "emits" ident "{" field* "}"
         | "consumes" ident
         | "impl" ("get" | "post") path [ "rust" | "python" ] "↖" fragment "↗" ;
path     = "/" { letter | digit | "_" | "-" | "/" } ;
table    = "table" ident "{" field* "}" ;
field    = ident ":" type [ "pk" ] [ "," ] ;
type     = "uuid" | "int" | "money" | "text" | "bool" | "timestamp"
         | "enum" "(" ident { "," ident } ")" ;
```

Whitespace is insignificant; commas between fields are optional. `lang`,
`port`, and `db` may each appear at most once per service (a parse error
otherwise); `emits` and `consumes` may repeat.

## Semantics

- **Services** are the unit of deployment: one process, one directory in the
  generated output, its own database if it declares one. `lang` picks the
  implementation language of the *generated* service.
- **`port`** is the service's HTTP port. Optional — a service with no port is
  a pure consumer.
- **`db`** declares a service-private store. There is no shared database:
  the only cross-service channel is events, and the generators enforce that
  by construction.
- **Events** (`emits`) live in a single global namespace and are the
  cross-service contract. `consumes` names an event some service must emit;
  the validator resolves these references globally. One event, one emitter —
  many consumers.
- **`impl`** (phase 4a; real ASTs since 4c) is the hatch for bespoke logic:
  an HTTP route whose handler body is written inline between quilt's arrow
  brackets, in the service's own language. The fragment is parsed with the
  real grammar of that language (the same tree-sitter `Language`s quilt's
  quotes use — a syntax error fails `check`) and the parsed term is spliced
  into the generated service — in Rust as the tail expression of an
  `impl IntoResponse` handler, in Python as the body of a function returning
  the response text. The brackets may carry the quote's language annotation
  (`rust↖ … ↗`, the `.arch.quilt` spelling); it is optional in plain `.arch`
  and must match the service's `lang`. Impl routes get docs rows and smoke
  checks like every derived route; the validator rejects routes that shadow
  a derived one (`/health`, `/peers`, `/emit/*`, `/events/*`).
- **Types** are a closed set on purpose: each must map cleanly onto every
  target (SQL column, Rust type, Python type). `money` exists to force the
  interesting mapping question (integer cents, `NUMERIC`, `Decimal`) through
  one place instead of ad-hoc choices per service.

What the DSL deliberately does **not** have: per-endpoint routing tables,
middleware config, retry policies. Defaults are generated; overrides arrive
in phase 4 as inline code, not as DSL surface area.

## Planned: inline languages (phase 4)

The DSL stays declarative, but bespoke logic will be written inline by
quoting other languages with quilt's arrow brackets once arch is a quilt
`Language`:

```text
service orders {
  lang rust
  port 8081
  handler place_order: rust↖
      let order = Order::new(req.user_id, req.total);
      db.insert(&order).await?;
      emit!(OrderPlaced { order_id: order.id, total: order.total });
  ↗
}
```

The quoted body is a real Rust AST spliced into the generated handler at
expansion time — highlighted, LSP-checked, and immune to escaping bugs — not
a string pasted into a template. See [plan](plan.md), phase 4, for the
stepping stones (string escape hatch → `Language` impl → inline quotes →
tree-sitter grammar + LSP).
