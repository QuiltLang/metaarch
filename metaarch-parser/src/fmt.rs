//! Canonical formatter for `.arch` source: `metaarch fmt`.
//!
//! Trivia-preserving: `#` comments and (single) blank-line groupings
//! survive; everything else is re-printed in the canonical style the
//! examples use — two-space indent, `name: type` fields, field blocks
//! inline when the whole line fits in 80 columns and one-per-line with
//! trailing commas otherwise, fragment bodies re-indented one level under
//! their `impl`. Formatting is only attempted on files [`crate::parse`]
//! accepts, so `fmt` can never mangle something it doesn't understand; and
//! because the spec dedents fragment bodies at lex time, re-indenting them
//! is semantics-preserving too.
//!
//! The walk below mirrors the parser's grammar function-for-function; the
//! `formatting_preserves_the_spec` test is what keeps the two in sync.

use metaarch_spec::Span;

use crate::lex::{Lexer, Tok, Token};
use crate::ParseError;

/// Target line width for the inline-vs-block field layout choice.
const WIDTH: usize = 80;

/// Format `.arch` source canonically. Errors are the parser's own — an
/// unparseable file is returned untouched by the CLI, never half-printed.
pub fn format(src: &str) -> Result<String, ParseError> {
    crate::parse(src)?;
    let mut fmt = Fmt {
        tokens: Lexer::new(src).lex()?,
        pos: 0,
        out: String::new(),
        line: String::new(),
        pending: Vec::new(),
        indent: 0,
        last_line: 0,
    };
    fmt.system()?;
    Ok(fmt.out)
}

/// Canonical spelling of a comment: `#` + text, with a space inserted
/// before flush-against-the-`#` words (`#foo` → `# foo`; banner rows like
/// `####` and hand-aligned text keep their shape).
fn comment_line(raw: &str) -> String {
    let text = raw.trim_end();
    match text.chars().next() {
        None => "#".into(),
        Some(c) if c.is_whitespace() || c == '#' => format!("#{text}"),
        Some(_) => format!("# {text}"),
    }
}

/// One line of a block-rendered field list.
enum FieldItem {
    /// `name: type pk` plus an optional trailing comment; `gap` marks a
    /// blank line above it in the source.
    Field {
        text: String,
        gap: bool,
        trailing: Option<String>,
    },
    Comment {
        text: String,
        gap: bool,
    },
}

struct Fmt {
    tokens: Vec<Token>,
    pos: usize,
    out: String,
    /// The physical line being assembled, without its indentation.
    line: String,
    /// Own-line comments found mid-entry; emitted right after the entry.
    pending: Vec<String>,
    indent: usize,
    /// Source line the last consumed token or comment ended on.
    last_line: u32,
}

impl Fmt {
    // -- output ------------------------------------------------------------

    fn emit_line(&mut self, text: &str) {
        for _ in 0..self.indent {
            self.out.push_str("  ");
        }
        self.out.push_str(text);
        self.out.push('\n');
    }

    /// At most one blank line, and never at the top of the file.
    fn blank(&mut self) {
        if !self.out.is_empty() && !self.out.ends_with("\n\n") {
            self.out.push('\n');
        }
    }

    /// Preserve a source blank-line grouping before the item on `next_line`.
    fn maybe_blank(&mut self, next_line: u32) {
        if self.line.is_empty() && self.last_line != 0 && next_line > self.last_line + 1 {
            self.blank();
        }
    }

    /// Append a word to the current line, space-separated.
    fn word(&mut self, s: &str) {
        if !self.line.is_empty() {
            self.line.push(' ');
        }
        self.line.push_str(s);
    }

    fn flush(&mut self) {
        if !self.line.is_empty() {
            let line = std::mem::take(&mut self.line);
            self.emit_line(&line);
        }
        for comment in std::mem::take(&mut self.pending) {
            self.emit_line(&comment);
        }
    }

    fn width_of(&self, extra: usize) -> usize {
        self.indent * 2 + self.line.len() + 1 + extra
    }

    // -- token access ------------------------------------------------------

    /// Print every comment sitting before the next token. A comment on the
    /// same line as the last flushed entry stays trailing; one inside a
    /// half-built entry is deferred to just below it; anything else gets its
    /// own line here.
    fn drain_comments(&mut self) {
        while let Tok::Comment(raw) = &self.tokens[self.pos].tok {
            let text = comment_line(raw);
            let line = self.tokens[self.pos].span.line;
            if !self.line.is_empty() {
                self.pending.push(text);
            } else if line == self.last_line && self.out.ends_with('\n') {
                self.out.pop();
                self.out.push(' ');
                self.out.push_str(&text);
                self.out.push('\n');
            } else {
                self.maybe_blank(line);
                self.emit_line(&text);
            }
            self.last_line = line;
            self.pos += 1;
        }
    }

    fn peek(&mut self) -> &Tok {
        self.drain_comments();
        &self.tokens[self.pos].tok
    }

    fn next(&mut self) -> Token {
        self.drain_comments();
        let tok = self.tokens[self.pos].clone();
        if self.pos < self.tokens.len() - 1 {
            self.pos += 1;
        }
        self.maybe_blank(tok.span.line);
        self.last_line = tok.end_line;
        tok
    }

    fn err<T>(&self, span: Span, message: impl Into<String>) -> Result<T, ParseError> {
        Err(ParseError {
            message: message.into(),
            span,
        })
    }

    fn ident(&mut self, what: &str) -> Result<(String, Span), ParseError> {
        let tok = self.next();
        match tok.tok {
            Tok::Ident(name) => Ok((name, tok.span)),
            other => self.err(tok.span, format!("expected {what}, found {other}")),
        }
    }

    fn keyword(&mut self, kw: &str) -> Result<Span, ParseError> {
        let (name, span) = self.ident(&format!("`{kw}`"))?;
        if name == kw {
            Ok(span)
        } else {
            self.err(span, format!("expected `{kw}`, found `{name}`"))
        }
    }

    fn expect(&mut self, want: &Tok, what: &str) -> Result<Token, ParseError> {
        let tok = self.next();
        if &tok.tok == want {
            Ok(tok)
        } else {
            self.err(tok.span, format!("expected {what}, found {}", tok.tok))
        }
    }

    fn eat(&mut self, want: &Tok) -> bool {
        if self.peek() == want {
            self.next();
            true
        } else {
            false
        }
    }

    // -- the grammar walk --------------------------------------------------

    fn system(&mut self) -> Result<(), ParseError> {
        self.keyword("system")?;
        self.word("system");
        let (name, _) = self.ident("system name")?;
        self.word(&name);
        self.flush();

        loop {
            match self.peek() {
                Tok::Eof => break,
                Tok::Ident(kw) if kw == "service" => self.service()?,
                other => {
                    let (other, span) = (other.to_string(), self.tokens[self.pos].span);
                    return self.err(span, format!("expected `service`, found {other}"));
                }
            }
        }
        Ok(())
    }

    fn service(&mut self) -> Result<(), ParseError> {
        self.keyword("service")?;
        self.word("service");
        let (name, _) = self.ident("service name")?;
        self.word(&name);
        self.expect(&Tok::LBrace, "`{`")?;
        self.word("{");
        self.flush();
        self.indent += 1;
        while !self.eat(&Tok::RBrace) {
            self.entry()?;
        }
        self.indent -= 1;
        self.word("}");
        self.flush();
        Ok(())
    }

    fn entry(&mut self) -> Result<(), ParseError> {
        let (kw, kw_span) = self
            .ident("a service entry (`lang`, `port`, `db`, `emits`, `consumes`, `impl`)")?;
        match kw.as_str() {
            "lang" => {
                self.word("lang");
                let (lang, _) = self.ident("`rust` or `python`")?;
                self.word(&lang);
                self.flush();
            }
            "port" => {
                self.word("port");
                let tok = self.next();
                match tok.tok {
                    Tok::Int(n) => self.word(&n.to_string()),
                    other => {
                        return self
                            .err(tok.span, format!("expected a port number, found {other}"));
                    }
                }
                self.flush();
            }
            "consumes" => {
                self.word("consumes");
                let (event, _) = self.ident("event name")?;
                self.word(&event);
                self.flush();
            }
            "emits" => {
                self.word("emits");
                let (event, _) = self.ident("event name")?;
                self.word(&event);
                self.expect(&Tok::LBrace, "`{`")?;
                self.fields_block()?;
            }
            "db" => {
                self.word("db");
                let (engine, _) = self.ident("`postgres` or `sqlite`")?;
                self.word(&engine);
                self.expect(&Tok::LBrace, "`{`")?;
                self.word("{");
                self.flush();
                self.indent += 1;
                while !self.eat(&Tok::RBrace) {
                    self.keyword("table")?;
                    self.word("table");
                    let (table, _) = self.ident("table name")?;
                    self.word(&table);
                    self.expect(&Tok::LBrace, "`{`")?;
                    self.fields_block()?;
                }
                self.indent -= 1;
                self.word("}");
                self.flush();
            }
            "impl" => self.impl_route()?,
            other => {
                return self.err(
                    kw_span,
                    format!(
                        "unknown service entry `{other}` (expected `lang`, `port`, `db`, `emits`, `consumes`, or `impl`)"
                    ),
                );
            }
        }
        Ok(())
    }

    fn impl_route(&mut self) -> Result<(), ParseError> {
        self.word("impl");
        let (verb, _) = self.ident("`get` or `post`")?;
        self.word(&verb);
        let tok = self.next();
        match tok.tok {
            Tok::Path(path) => self.word(&path),
            other => {
                return self.err(
                    tok.span,
                    format!("expected a route path starting with `/`, found {other}"),
                );
            }
        }
        let tok = self.next();
        let (annotation, tok) = match tok.tok {
            Tok::Ident(name) => (Some(name), self.next()),
            _ => (None, tok),
        };
        // A fragment written in block form stays block even when its body
        // would fit inline: the body is foreign code, so its author's layout
        // choice stands (unlike field lists, whose layout is ours to pick).
        let block = tok.span.line != tok.end_line;
        let body = match tok.tok {
            Tok::Fragment(body) => body,
            other => {
                return self.err(tok.span, format!("expected a `↖ … ↗` fragment, found {other}"));
            }
        };
        // The annotation is spelled flush against the bracket (`rust↖`), the
        // same spelling a `.arch.quilt` quote uses.
        let opener = match annotation {
            Some(lang) => format!("{lang}↖"),
            None => "↖".into(),
        };
        if !block && !body.contains('\n') {
            let inline = if body.is_empty() {
                format!("{opener} ↗")
            } else {
                format!("{opener} {body} ↗")
            };
            if self.width_of(inline.len()) <= WIDTH {
                self.word(&inline);
                self.flush();
                return Ok(());
            }
        }
        self.word(&opener);
        self.flush();
        self.indent += 1;
        for line in body.lines() {
            if line.is_empty() {
                self.out.push('\n');
            } else {
                self.emit_line(line);
            }
        }
        self.indent -= 1;
        self.word("↗");
        self.flush();
        Ok(())
    }

    /// A `{ field, … }` block whose opening `{` is already consumed (but not
    /// yet printed): inline when comment-free and the line fits [`WIDTH`],
    /// one field per line with trailing commas otherwise. Buffers its items
    /// first, so it intercepts interior comments before [`Self::next`]'s
    /// draining can misroute them.
    fn fields_block(&mut self) -> Result<(), ParseError> {
        let mut items: Vec<FieldItem> = Vec::new();
        loop {
            while let Tok::Comment(raw) = &self.tokens[self.pos].tok {
                let text = comment_line(raw);
                let line = self.tokens[self.pos].span.line;
                match items.last_mut() {
                    Some(FieldItem::Field { trailing, .. }) if line == self.last_line => {
                        *trailing = Some(text);
                    }
                    _ => items.push(FieldItem::Comment {
                        text,
                        gap: line > self.last_line + 1,
                    }),
                }
                self.last_line = line;
                self.pos += 1;
            }
            if matches!(self.tokens[self.pos].tok, Tok::RBrace) {
                let tok = self.tokens[self.pos].clone();
                self.pos += 1;
                self.last_line = tok.end_line;
                break;
            }
            let gap = self.tokens[self.pos].span.line > self.last_line + 1;
            let (name, _) = self.ident("field name")?;
            self.expect(&Tok::Colon, "`:`")?;
            let ty = self.ty_text()?;
            let pk = if let Tok::Ident(kw) = self.peek() {
                kw == "pk" && {
                    self.next();
                    true
                }
            } else {
                false
            };
            self.eat(&Tok::Comma);
            items.push(FieldItem::Field {
                text: format!("{name}: {ty}{}", if pk { " pk" } else { "" }),
                gap,
                trailing: None,
            });
        }

        let inline_ok = items
            .iter()
            .all(|i| matches!(i, FieldItem::Field { trailing: None, .. }));
        if inline_ok {
            let fields: Vec<&str> = items
                .iter()
                .filter_map(|i| match i {
                    FieldItem::Field { text, .. } => Some(text.as_str()),
                    FieldItem::Comment { .. } => None,
                })
                .collect();
            let inline = if fields.is_empty() {
                "{}".into()
            } else {
                format!("{{ {} }}", fields.join(", "))
            };
            if self.width_of(inline.len()) <= WIDTH {
                self.word(&inline);
                self.flush();
                return Ok(());
            }
        }

        self.word("{");
        self.flush();
        self.indent += 1;
        for item in &items {
            match item {
                FieldItem::Field { text, gap, trailing } => {
                    if *gap {
                        self.blank();
                    }
                    let trailing = match trailing {
                        Some(comment) => format!(" {comment}"),
                        None => String::new(),
                    };
                    let line = format!("{text},{trailing}");
                    self.emit_line(&line);
                }
                FieldItem::Comment { text, gap } => {
                    if *gap {
                        self.blank();
                    }
                    let text = text.clone();
                    self.emit_line(&text);
                }
            }
        }
        self.indent -= 1;
        self.word("}");
        self.flush();
        Ok(())
    }

    /// The canonical spelling of a field type: the bare name, or
    /// `enum(a, b, c)`.
    fn ty_text(&mut self) -> Result<String, ParseError> {
        let (name, _) = self.ident("a type")?;
        if name != "enum" {
            return Ok(name);
        }
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
                    return self.err(tok.span, format!("expected `,` or `)`, found {other}"));
                }
            }
        }
        Ok(format!("enum({})", variants.join(", ")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use metaarch_spec::SystemSpec;

    /// The example file is kept canonical; `bin/main fmt --check` in CI-ish
    /// flows depends on that being true.
    const SHOP: &str = include_str!("../../examples/shop.arch");

    fn wipe_spans(spec: &mut SystemSpec) {
        let zero = Span { line: 0, col: 0 };
        for service in &mut spec.services {
            service.span = zero;
            if let Some(db) = &mut service.db {
                db.span = zero;
                for table in &mut db.tables {
                    table.span = zero;
                    for field in &mut table.fields {
                        field.span = zero;
                    }
                }
            }
            for event in &mut service.emits {
                event.span = zero;
                for field in &mut event.fields {
                    field.span = zero;
                }
            }
            for (_, span) in &mut service.consumes {
                *span = zero;
            }
            for route in &mut service.impls {
                route.span = zero;
            }
        }
    }

    #[track_caller]
    fn assert_spec_preserved(src: &str) {
        let formatted = format(src).unwrap();
        let mut before = crate::parse(src).unwrap();
        let mut after = crate::parse(&formatted).unwrap();
        wipe_spans(&mut before);
        wipe_spans(&mut after);
        assert_eq!(before, after, "formatting changed the spec:\n{formatted}");
    }

    #[test]
    fn shop_example_is_canonical() {
        assert_eq!(format(SHOP).unwrap(), SHOP);
    }

    #[test]
    fn formatting_is_idempotent() {
        let once = format(SHOP).unwrap();
        assert_eq!(format(&once).unwrap(), once);
    }

    #[test]
    fn formatting_preserves_the_spec() {
        assert_spec_preserved(SHOP);
        assert_spec_preserved(
            "system s\nservice a{lang rust\nport 80\nemits E{a:uuid,b:money}}",
        );
    }

    #[test]
    fn canonicalizes_messy_source() {
        let messy = "system   s\nservice a{lang rust\n  port    8080 }";
        assert_eq!(
            format(messy).unwrap(),
            "system s\nservice a {\n  lang rust\n  port 8080\n}\n"
        );
    }

    #[test]
    fn wide_field_lists_go_one_per_line() {
        let src = "system s\nservice a { lang rust\n db sqlite { table t { id: uuid pk, user_id: uuid, total: money, status: enum(pending, paid, shipped), placed_at: timestamp } } }";
        let formatted = format(src).unwrap();
        assert!(formatted.contains("    table t {\n      id: uuid pk,\n"));
        assert!(formatted.contains("      status: enum(pending, paid, shipped),\n"));
        assert!(formatted.trim_end().ends_with('}'));
        assert_spec_preserved(src);
    }

    #[test]
    fn comments_survive_in_place() {
        let src = "# top\nsystem s\n\n# svc\nservice a {\n  lang rust # trailing\n  db sqlite {\n    table t {\n      # inside\n      id: uuid pk, # key\n    }\n  }\n}\n";
        let formatted = format(src).unwrap();
        assert_eq!(formatted, src);
    }

    #[test]
    fn blank_lines_collapse_to_one() {
        let src = "system s\n\n\n\nservice a {\n  lang rust\n}\n";
        assert_eq!(
            format(src).unwrap(),
            "system s\n\nservice a {\n  lang rust\n}\n"
        );
    }

    #[test]
    fn fragments_reindent_under_their_impl() {
        let src = "system s\nservice a {\n  lang rust\n  impl get /x rust↖\n        \"deep\"\n  ↗\n}\n";
        let formatted = format(src).unwrap();
        assert!(formatted.contains("  impl get /x rust↖\n    \"deep\"\n  ↗\n"));
        assert_spec_preserved(src);
    }

    #[test]
    fn short_fragments_stay_inline() {
        let src = "system s\nservice a {\n  lang rust\n  impl get /x ↖ \"hi\" ↗\n}\n";
        assert_eq!(format(src).unwrap(), src);
    }

    #[test]
    fn rejects_what_the_parser_rejects() {
        assert!(format("system s\nservice a {\n  speed 9\n}").is_err());
    }
}
