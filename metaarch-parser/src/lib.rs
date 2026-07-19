//! Parser for the `.arch` DSL: hand-rolled lexer + recursive descent.
//!
//! Hand-rolled (rather than pest/nom) for two reasons: full control over
//! error messages with positions, and no parser-framework shape to unlearn
//! when the DSL later becomes a first-class quilt `Language` with inline
//! quote brackets (see docs/wiki/plan.md, phase 4).

use metaarch_spec::{
    Database, Engine, EventType, Field, ImplRoute, Lang, Method, Service, Span, SystemSpec, Table,
    Ty,
};

mod lex;

use lex::{Lexer, Tok, Token};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub message: String,
    pub span: Span,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "parse error: {} ({})", self.message, self.span)
    }
}

impl std::error::Error for ParseError {}

pub fn parse(src: &str) -> Result<SystemSpec, ParseError> {
    Parser::new(src)?.system()
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn new(src: &str) -> Result<Self, ParseError> {
        Ok(Parser {
            tokens: Lexer::new(src).lex()?,
            pos: 0,
        })
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.pos.min(self.tokens.len() - 1)]
    }

    fn next(&mut self) -> Token {
        let tok = self.peek().clone();
        if self.pos < self.tokens.len() - 1 {
            self.pos += 1;
        }
        tok
    }

    fn err<T>(&self, span: Span, message: impl Into<String>) -> Result<T, ParseError> {
        Err(ParseError {
            message: message.into(),
            span,
        })
    }

    fn expect(&mut self, want: &Tok, what: &str) -> Result<Token, ParseError> {
        let tok = self.next();
        if &tok.tok == want {
            Ok(tok)
        } else {
            self.err(tok.span, format!("expected {what}, found {}", tok.tok))
        }
    }

    /// Consume an identifier and return (text, span).
    fn ident(&mut self, what: &str) -> Result<(String, Span), ParseError> {
        let tok = self.next();
        match tok.tok {
            Tok::Ident(name) => Ok((name, tok.span)),
            other => self.err(tok.span, format!("expected {what}, found {other}")),
        }
    }

    /// Consume a specific keyword (an identifier with fixed text).
    fn keyword(&mut self, kw: &str) -> Result<Span, ParseError> {
        let (name, span) = self.ident(&format!("`{kw}`"))?;
        if name == kw {
            Ok(span)
        } else {
            self.err(span, format!("expected `{kw}`, found `{name}`"))
        }
    }

    fn eat(&mut self, want: &Tok) -> bool {
        if &self.peek().tok == want {
            self.next();
            true
        } else {
            false
        }
    }

    // system := "system" ident service* EOF
    fn system(&mut self) -> Result<SystemSpec, ParseError> {
        self.keyword("system")?;
        let (name, _) = self.ident("system name")?;

        let mut services = Vec::new();
        loop {
            let tok = self.peek().clone();
            match &tok.tok {
                Tok::Eof => break,
                Tok::Ident(kw) if kw == "service" => services.push(self.service()?),
                other => {
                    return self.err(tok.span, format!("expected `service`, found {other}"));
                }
            }
        }

        Ok(SystemSpec { name, services })
    }

    // service := "service" ident "{" entry* "}"
    fn service(&mut self) -> Result<Service, ParseError> {
        self.keyword("service")?;
        let (name, span) = self.ident("service name")?;
        self.expect(&Tok::LBrace, "`{`")?;

        let mut service = Service {
            name,
            lang: None,
            port: None,
            db: None,
            emits: Vec::new(),
            consumes: Vec::new(),
            impls: Vec::new(),
            span,
        };

        while !self.eat(&Tok::RBrace) {
            let (kw, kw_span) = self
                .ident("a service entry (`lang`, `port`, `db`, `emits`, `consumes`, `impl`)")?;
            match kw.as_str() {
                "lang" => {
                    if service.lang.is_some() {
                        return self.err(kw_span, "duplicate `lang` entry");
                    }
                    let (lang, lang_span) = self.ident("`rust` or `python`")?;
                    service.lang = Some(match lang.as_str() {
                        "rust" => Lang::Rust,
                        "python" => Lang::Python,
                        other => {
                            return self.err(
                                lang_span,
                                format!("unknown lang `{other}` (expected `rust` or `python`)"),
                            );
                        }
                    });
                }
                "port" => {
                    if service.port.is_some() {
                        return self.err(kw_span, "duplicate `port` entry");
                    }
                    let tok = self.next();
                    match tok.tok {
                        Tok::Int(n) => match u16::try_from(n) {
                            Ok(port) => service.port = Some(port),
                            Err(_) => {
                                return self.err(tok.span, format!("port {n} is out of range"));
                            }
                        },
                        other => {
                            return self
                                .err(tok.span, format!("expected a port number, found {other}"));
                        }
                    }
                }
                "db" => {
                    if service.db.is_some() {
                        return self.err(kw_span, "duplicate `db` entry");
                    }
                    service.db = Some(self.database(kw_span)?);
                }
                "emits" => {
                    let (name, span) = self.ident("event name")?;
                    self.expect(&Tok::LBrace, "`{`")?;
                    let fields = self.fields()?;
                    service.emits.push(EventType { name, fields, span });
                }
                "consumes" => {
                    let (name, span) = self.ident("event name")?;
                    service.consumes.push((name, span));
                }
                // impl := "impl" ("get" | "post") path "↖" fragment "↗"
                "impl" => {
                    let (verb, verb_span) = self.ident("`get` or `post`")?;
                    let method = match verb.as_str() {
                        "get" => Method::Get,
                        "post" => Method::Post,
                        other => {
                            return self.err(
                                verb_span,
                                format!("unknown method `{other}` (expected `get` or `post`)"),
                            );
                        }
                    };
                    let tok = self.next();
                    let path = match tok.tok {
                        Tok::Path(path) => path,
                        other => {
                            return self.err(
                                tok.span,
                                format!("expected a route path starting with `/`, found {other}"),
                            );
                        }
                    };
                    let tok = self.next();
                    let body = match tok.tok {
                        Tok::Fragment(body) => body,
                        other => {
                            return self.err(
                                tok.span,
                                format!("expected a `↖ … ↗` fragment, found {other}"),
                            );
                        }
                    };
                    service.impls.push(ImplRoute {
                        method,
                        path,
                        body,
                        span: kw_span,
                    });
                }
                other => {
                    return self.err(
                        kw_span,
                        format!(
                            "unknown service entry `{other}` (expected `lang`, `port`, `db`, `emits`, `consumes`, or `impl`)"
                        ),
                    );
                }
            }
        }

        Ok(service)
    }

    // database := ("postgres" | "sqlite") "{" table* "}"
    fn database(&mut self, span: Span) -> Result<Database, ParseError> {
        let (engine, engine_span) = self.ident("`postgres` or `sqlite`")?;
        let engine = match engine.as_str() {
            "postgres" => Engine::Postgres,
            "sqlite" => Engine::Sqlite,
            other => {
                return self.err(
                    engine_span,
                    format!("unknown db engine `{other}` (expected `postgres` or `sqlite`)"),
                );
            }
        };
        self.expect(&Tok::LBrace, "`{`")?;

        let mut tables = Vec::new();
        while !self.eat(&Tok::RBrace) {
            self.keyword("table")?;
            let (name, span) = self.ident("table name")?;
            self.expect(&Tok::LBrace, "`{`")?;
            let fields = self.fields()?;
            tables.push(Table { name, fields, span });
        }

        Ok(Database {
            engine,
            tables,
            span,
        })
    }

    // fields := (field ","?)* "}"     (the opening "{" is already consumed)
    // field  := ident ":" type "pk"?
    fn fields(&mut self) -> Result<Vec<Field>, ParseError> {
        let mut fields = Vec::new();
        while !self.eat(&Tok::RBrace) {
            let (name, span) = self.ident("field name")?;
            self.expect(&Tok::Colon, "`:`")?;
            let ty = self.ty()?;
            let pk = if let Tok::Ident(kw) = &self.peek().tok {
                kw == "pk" && {
                    self.next();
                    true
                }
            } else {
                false
            };
            self.eat(&Tok::Comma);
            fields.push(Field { name, ty, pk, span });
        }
        Ok(fields)
    }

    // type := "uuid" | "int" | "money" | "text" | "bool" | "timestamp"
    //       | "enum" "(" ident ("," ident)* ")"
    fn ty(&mut self) -> Result<Ty, ParseError> {
        let (name, span) = self.ident("a type")?;
        Ok(match name.as_str() {
            "uuid" => Ty::Uuid,
            "int" => Ty::Int,
            "money" => Ty::Money,
            "text" => Ty::Text,
            "bool" => Ty::Bool,
            "timestamp" => Ty::Timestamp,
            "enum" => {
                self.expect(&Tok::LParen, "`(`")?;
                let mut variants = Vec::new();
                loop {
                    let (variant, _) = self.ident("an enum variant")?;
                    variants.push(variant);
                    let tok = self.next();
                    match tok.tok {
                        Tok::Comma => continue,
                        Tok::RParen => break,
                        other => {
                            return self
                                .err(tok.span, format!("expected `,` or `)`, found {other}"));
                        }
                    }
                }
                Ty::Enum(variants)
            }
            other => {
                return self.err(
                    span,
                    format!(
                        "unknown type `{other}` (expected `uuid`, `int`, `money`, `text`, `bool`, `timestamp`, or `enum(...)`)"
                    ),
                );
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHOP: &str = r#"
# A tiny web shop.
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
"#;

    #[test]
    fn parses_shop() {
        let spec = parse(SHOP).unwrap();
        assert_eq!(spec.name, "shop");
        assert_eq!(spec.services.len(), 2);

        let orders = &spec.services[0];
        assert_eq!(orders.name, "orders");
        assert_eq!(orders.lang, Some(Lang::Rust));
        assert_eq!(orders.port, Some(8081));
        let db = orders.db.as_ref().unwrap();
        assert_eq!(db.engine, Engine::Postgres);
        assert_eq!(db.tables.len(), 1);
        let table = &db.tables[0];
        assert_eq!(table.fields.len(), 3);
        assert!(table.fields[0].pk);
        assert_eq!(
            table.fields[2].ty,
            Ty::Enum(vec!["pending".into(), "paid".into(), "shipped".into()])
        );
        assert_eq!(orders.emits.len(), 1);
        assert_eq!(orders.emits[0].name, "OrderPlaced");

        let notifier = &spec.services[1];
        assert_eq!(notifier.lang, Some(Lang::Python));
        assert_eq!(notifier.consumes[0].0, "OrderPlaced");
    }

    #[test]
    fn parses_without_commas() {
        let spec = parse(
            "system s\nservice a {\n lang rust\n db sqlite {\n table t { id: uuid pk n: int }\n }\n}",
        )
        .unwrap();
        let fields = &spec.services[0].db.as_ref().unwrap().tables[0].fields;
        assert_eq!(fields.len(), 2);
    }

    #[test]
    fn error_has_position() {
        let err = parse("system s\nservice a {\n speed 9\n}").unwrap_err();
        assert!(
            err.message.contains("unknown service entry `speed`"),
            "{err}"
        );
        assert_eq!(err.span.line, 3);
    }

    #[test]
    fn rejects_unknown_type() {
        let err =
            parse("system s\nservice a {\n db sqlite { table t { id: uuidd pk } }\n}").unwrap_err();
        assert!(err.message.contains("unknown type `uuidd`"), "{err}");
    }

    #[test]
    fn parses_impl_route() {
        let spec = parse(
            "system s\nservice a {\n lang rust\n impl get /hello ↖\n   \"hi\".to_string()\n ↗\n}",
        )
        .unwrap();
        let impls = &spec.services[0].impls;
        assert_eq!(impls.len(), 1);
        assert_eq!(impls[0].method, Method::Get);
        assert_eq!(impls[0].path, "/hello");
        assert_eq!(impls[0].body, "\"hi\".to_string()");
        assert_eq!(impls[0].handler_name(), "impl_get_hello");
    }

    #[test]
    fn dedents_multiline_fragment() {
        let spec = parse(
            "system s\nservice a {\n lang rust\n impl post /go ↖\n    let x = 1;\n      x + 1\n ↗\n}",
        )
        .unwrap();
        assert_eq!(spec.services[0].impls[0].body, "let x = 1;\n  x + 1");
        assert_eq!(spec.services[0].impls[0].handler_name(), "impl_post_go");
    }

    #[test]
    fn rejects_unterminated_fragment() {
        let err = parse("system s\nservice a {\n lang rust\n impl get /x ↖ oops\n}").unwrap_err();
        assert!(err.message.contains("unterminated"), "{err}");
    }

    #[test]
    fn rejects_impl_without_path() {
        let err = parse("system s\nservice a {\n impl get hello ↖ x ↗\n}").unwrap_err();
        assert!(
            err.message.contains("expected a route path"),
            "{err}"
        );
    }

    #[test]
    fn rejects_unterminated_service() {
        let err = parse("system s\nservice a {\n lang rust\n").unwrap_err();
        assert!(err.message.contains("found end of file"), "{err}");
    }
}
