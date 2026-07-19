//! arch as a *dynamic* quilt language.
//!
//! This crate implements quilt's [`Language`] trait for the `.arch` DSL by
//! hand — no tree-sitter grammar — so arch can be registered at runtime via
//! `DictMulti::add_lang` (quilt's `Box<dyn Language>` hook) instead of being
//! hardcoded into quilt's built-in set. `metaarch-expand` does that
//! registration; see docs/wiki/plan.md, phase 4b.
//!
//! What "parsing" means here is different from `metaarch-parser`: that crate
//! turns a `.arch` *string* into the typed `SystemSpec` the generators
//! consume, and stays the semantic authority (types, validation). This crate
//! turns quilt's flat node stream — text interleaved with **holes** where
//! `↙…↘` splices and `↖…↗` quotes sat — into a syntactic `QTerm` that
//! coparses back to the source, records a [`Hole`] per splice position, and
//! can be filled with plugs. That is exactly what quilt needs to let host
//! metaprograms quote arch (`arch↖ … ↗`) and to parse `.arch.quilt` files,
//! where an `impl` route's fragment arrives as a nested quote (the phase-4c
//! mechanism the 4a escape hatch was shaped for).
//!
//! Structure is kept where splices need it: tuples for the container
//! productions (`arch_file`, `service`, `database`, `table`, `event`,
//! `field`, `impl_route`) with plain tokens written into their command
//! streams. Hole positions and their `otag`s:
//! `system_name`, `service_name`, `lang_value`, `port_value`, `engine`,
//! `event_name`, `table_name` (via `ident_or_splice`), `field_name`, `type`,
//! `variant`, `method`, `path`, `impl_fragment`, and the variadic
//! `entry` / `field` / `table` / `service` positions.
//!
//! The type set is *not* enforced here — any identifier parses as a type.
//! Semantic strictness (closed types, topology checks) stays in
//! `metaarch-spec::validate`, which runs on the expanded/coparsed output.

use std::sync::Arc;

use quilt::lang::{Arity, FlatNode, Hole, InnerKind, Language, LanguagePost};
use quilt::prelude::{bx, cmd, leaf, miette, tuple, write, QTerm, Result, HOLE, NL};
use quilt::term::CmdOrHole;

/// The canonical language name to register under (`add_lang(LANG, …)`).
pub const LANG: &str = "arch";

/// Tag of the placeholder leaf a splice leaves in the pre-parse term;
/// `parse_post` replaces these with the plugs.
const HOLE_TAG: &str = "arch_hole";

/**************************************************************/

/// Trivia preceding a token: literal text (whitespace, `#` comments) and
/// newlines, kept apart so newlines become [`StrCmd::NewLine`] commands and
/// re-indent correctly when the term is spliced under a prefix.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Lead {
    Text(String),
    Newline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Ident,
    Int,
    Path,
    Colon,
    Comma,
    LBrace,
    RBrace,
    LParen,
    RParen,
    /// A `FlatNode::Hole` — where a splice or nested quote sat.
    Splice,
    Eof,
}

#[derive(Debug, Clone)]
struct FTok {
    kind: Kind,
    text: String,
    lead: Vec<Lead>,
}

impl FTok {
    fn describe(&self) -> String {
        match self.kind {
            Kind::Splice => "a splice".into(),
            Kind::Eof => "end of input".into(),
            _ => format!("`{}`", self.text),
        }
    }
}

fn push_text(lead: &mut Vec<Lead>, s: &str) {
    if s.is_empty() {
        return;
    }
    if let Some(Lead::Text(t)) = lead.last_mut() {
        t.push_str(s);
    } else {
        lead.push(Lead::Text(s.into()));
    }
}

/// Tokenize the flat node stream. `Str` segments are lexed like
/// `metaarch-parser`'s lexer (idents, ints, `/paths`, punctuation, `#`
/// comments), holes become [`Kind::Splice`] tokens, and all trivia is
/// attached to the following token so coparse reproduces the source.
fn lex(code: &[FlatNode]) -> Result<Vec<FTok>> {
    let mut toks = Vec::new();
    let mut lead: Vec<Lead> = Vec::new();
    for node in code {
        match node {
            FlatNode::NewLine => lead.push(Lead::Newline),
            FlatNode::Hole => toks.push(FTok {
                kind: Kind::Splice,
                text: String::new(),
                lead: std::mem::take(&mut lead),
            }),
            FlatNode::Str(s) => {
                let mut it = s.chars().peekable();
                while let Some(&c) = it.peek() {
                    if c == '\n' {
                        it.next();
                        lead.push(Lead::Newline);
                        continue;
                    }
                    if c.is_whitespace() {
                        it.next();
                        push_text(&mut lead, &c.to_string());
                        continue;
                    }
                    if c == '#' {
                        let mut t = String::new();
                        while let Some(&d) = it.peek() {
                            if d == '\n' {
                                break;
                            }
                            t.push(d);
                            it.next();
                        }
                        push_text(&mut lead, &t);
                        continue;
                    }
                    let (kind, text) = match c {
                        ':' => (Kind::Colon, {
                            it.next();
                            ":".to_string()
                        }),
                        ',' => (Kind::Comma, {
                            it.next();
                            ",".to_string()
                        }),
                        '{' => (Kind::LBrace, {
                            it.next();
                            "{".to_string()
                        }),
                        '}' => (Kind::RBrace, {
                            it.next();
                            "}".to_string()
                        }),
                        '(' => (Kind::LParen, {
                            it.next();
                            "(".to_string()
                        }),
                        ')' => (Kind::RParen, {
                            it.next();
                            ")".to_string()
                        }),
                        '/' => {
                            let mut t = String::new();
                            while let Some(&d) = it.peek() {
                                if d.is_ascii_alphanumeric() || d == '_' || d == '-' || d == '/' {
                                    t.push(d);
                                    it.next();
                                } else {
                                    break;
                                }
                            }
                            (Kind::Path, t)
                        }
                        c if c.is_ascii_digit() => {
                            let mut t = String::new();
                            while let Some(&d) = it.peek() {
                                if d.is_ascii_digit() {
                                    t.push(d);
                                    it.next();
                                } else {
                                    break;
                                }
                            }
                            (Kind::Int, t)
                        }
                        c if c.is_alphabetic() || c == '_' => {
                            let mut t = String::new();
                            while let Some(&d) = it.peek() {
                                if d.is_alphanumeric() || d == '_' {
                                    t.push(d);
                                    it.next();
                                } else {
                                    break;
                                }
                            }
                            (Kind::Ident, t)
                        }
                        other => return Err(miette!("arch: unexpected character `{other}`")),
                    };
                    toks.push(FTok {
                        kind,
                        text,
                        lead: std::mem::take(&mut lead),
                    });
                }
            }
        }
    }
    toks.push(FTok {
        kind: Kind::Eof,
        text: String::new(),
        lead,
    });
    Ok(toks)
}

/**************************************************************/

/// Builder for one production tuple: commands (writes, newlines, child
/// holes) plus the child terms, in source order.
struct B {
    tag: &'static str,
    cmds: Vec<CmdOrHole>,
    children: Vec<Arc<QTerm>>,
}

impl B {
    fn new(tag: &'static str) -> Self {
        B {
            tag,
            cmds: Vec::new(),
            children: Vec::new(),
        }
    }

    fn lead(&mut self, lead: &[Lead]) {
        for l in lead {
            match l {
                Lead::Text(s) => self.cmds.push(cmd(write(s))),
                Lead::Newline => self.cmds.push(cmd(NL)),
            }
        }
    }

    fn tok(&mut self, t: &FTok) {
        self.lead(&t.lead);
        if !t.text.is_empty() {
            self.cmds.push(cmd(write(&t.text)));
        }
    }

    fn child(&mut self, term: Arc<QTerm>) {
        self.cmds.push(HOLE);
        self.children.push(term);
    }

    fn build(self) -> Arc<QTerm> {
        tuple(self.tag, &self.children, &self.cmds)
    }
}

struct P<'t> {
    toks: &'t [FTok],
    pos: usize,
    holes: Vec<Hole>,
}

impl P<'_> {
    fn peek(&self) -> &FTok {
        &self.toks[self.pos.min(self.toks.len() - 1)]
    }

    fn bump(&mut self) -> &FTok {
        let t = &self.toks[self.pos.min(self.toks.len() - 1)];
        if self.pos < self.toks.len() - 1 {
            self.pos += 1;
        }
        t
    }

    fn at_kw(&self, kw: &str) -> bool {
        let t = self.peek();
        t.kind == Kind::Ident && t.text == kw
    }

    /// Consume the current token into `b`, checking its kind.
    fn expect(&mut self, b: &mut B, kind: Kind, what: &str) -> Result<()> {
        if self.peek().kind != kind {
            return Err(miette!("arch: expected {what}, found {}", self.peek().describe()));
        }
        let t = self.bump().clone();
        b.tok(&t);
        Ok(())
    }

    fn expect_kw(&mut self, b: &mut B, kw: &str) -> Result<()> {
        if !self.at_kw(kw) {
            return Err(miette!("arch: expected `{kw}`, found {}", self.peek().describe()));
        }
        let t = self.bump().clone();
        b.tok(&t);
        Ok(())
    }

    /// Consume a splice token: record its [`Hole`] and leave a placeholder
    /// leaf child for `parse_post` to fill.
    fn splice(&mut self, b: &mut B, otag: &str, ikind: Option<InnerKind>) {
        let t = self.bump().clone();
        b.lead(&t.lead);
        self.holes.push(Hole {
            otag: otag.into(),
            ikind,
            prefix: Box::default(),
        });
        b.child(leaf(HOLE_TAG, ""));
    }

    /// An identifier written through, or a splice recorded under `otag`.
    fn ident_or_splice(&mut self, b: &mut B, otag: &str) -> Result<()> {
        match self.peek().kind {
            Kind::Splice => {
                self.splice(b, otag, Some(InnerKind::Expr));
                Ok(())
            }
            Kind::Ident => {
                let t = self.bump().clone();
                b.tok(&t);
                Ok(())
            }
            _ => Err(miette!(
                "arch: expected an identifier or a splice for {otag}, found {}",
                self.peek().describe()
            )),
        }
    }

    /// Top-level dispatch: a whole file (`system …`), a run of services, or
    /// a run of service entries — so quoted arch fragments can be any of the
    /// three grains.
    fn top(&mut self) -> Result<Arc<QTerm>> {
        if self.at_kw("system") {
            return self.file();
        }
        if self.at_kw("service") || self.peek().kind == Kind::Splice {
            return self.service_seq();
        }
        self.entry_seq()
    }

    fn file(&mut self) -> Result<Arc<QTerm>> {
        let mut b = B::new("arch_file");
        self.expect_kw(&mut b, "system")?;
        self.ident_or_splice(&mut b, "system_name")?;
        loop {
            match self.peek().kind {
                Kind::Eof => {
                    let t = self.bump().clone();
                    b.tok(&t);
                    break;
                }
                Kind::Splice => self.splice(&mut b, "service", Some(InnerKind::Stmt)),
                _ if self.at_kw("service") => {
                    let svc = self.service()?;
                    b.child(svc);
                }
                _ => {
                    return Err(miette!(
                        "arch: expected `service`, found {}",
                        self.peek().describe()
                    ));
                }
            }
        }
        Ok(b.build())
    }

    fn service_seq(&mut self) -> Result<Arc<QTerm>> {
        let mut b = B::new("service_seq");
        loop {
            match self.peek().kind {
                Kind::Eof => {
                    let t = self.bump().clone();
                    b.tok(&t);
                    break;
                }
                Kind::Splice => self.splice(&mut b, "service", Some(InnerKind::Stmt)),
                _ if self.at_kw("service") => {
                    let svc = self.service()?;
                    b.child(svc);
                }
                _ => {
                    return Err(miette!(
                        "arch: expected `service`, found {}",
                        self.peek().describe()
                    ));
                }
            }
        }
        Ok(b.build())
    }

    fn entry_seq(&mut self) -> Result<Arc<QTerm>> {
        let mut b = B::new("entry_seq");
        while self.peek().kind != Kind::Eof {
            self.entry(&mut b)?;
        }
        let t = self.bump().clone();
        b.tok(&t);
        Ok(b.build())
    }

    fn service(&mut self) -> Result<Arc<QTerm>> {
        let mut b = B::new("service");
        self.expect_kw(&mut b, "service")?;
        self.ident_or_splice(&mut b, "service_name")?;
        self.expect(&mut b, Kind::LBrace, "`{`")?;
        loop {
            match self.peek().kind {
                Kind::RBrace => {
                    let t = self.bump().clone();
                    b.tok(&t);
                    break;
                }
                Kind::Eof => return Err(miette!("arch: unterminated service (missing `}}`)")),
                _ => self.entry(&mut b)?,
            }
        }
        Ok(b.build())
    }

    fn entry(&mut self, b: &mut B) -> Result<()> {
        if self.peek().kind == Kind::Splice {
            self.splice(b, "entry", Some(InnerKind::Stmt));
            return Ok(());
        }
        if self.peek().kind != Kind::Ident {
            return Err(miette!(
                "arch: expected a service entry, found {}",
                self.peek().describe()
            ));
        }
        match self.peek().text.as_str() {
            "lang" => {
                self.expect_kw(b, "lang")?;
                self.ident_or_splice(b, "lang_value")
            }
            "port" => {
                self.expect_kw(b, "port")?;
                match self.peek().kind {
                    Kind::Splice => {
                        self.splice(b, "port_value", Some(InnerKind::Expr));
                        Ok(())
                    }
                    Kind::Int => {
                        let t = self.bump().clone();
                        b.tok(&t);
                        Ok(())
                    }
                    _ => Err(miette!(
                        "arch: expected a port number, found {}",
                        self.peek().describe()
                    )),
                }
            }
            "db" => {
                let db = self.database()?;
                b.child(db);
                Ok(())
            }
            "emits" => {
                let ev = self.event()?;
                b.child(ev);
                Ok(())
            }
            "consumes" => {
                self.expect_kw(b, "consumes")?;
                self.ident_or_splice(b, "event_name")
            }
            "impl" => {
                let route = self.impl_route()?;
                b.child(route);
                Ok(())
            }
            other => Err(miette!(
                "arch: unknown service entry `{other}` (expected `lang`, `port`, `db`, `emits`, `consumes`, or `impl`)"
            )),
        }
    }

    fn database(&mut self) -> Result<Arc<QTerm>> {
        let mut b = B::new("database");
        self.expect_kw(&mut b, "db")?;
        self.ident_or_splice(&mut b, "engine")?;
        self.expect(&mut b, Kind::LBrace, "`{`")?;
        loop {
            match self.peek().kind {
                Kind::RBrace => {
                    let t = self.bump().clone();
                    b.tok(&t);
                    break;
                }
                Kind::Splice => self.splice(&mut b, "table", Some(InnerKind::Stmt)),
                _ if self.at_kw("table") => {
                    let t = self.table()?;
                    b.child(t);
                }
                _ => {
                    return Err(miette!(
                        "arch: expected `table`, found {}",
                        self.peek().describe()
                    ));
                }
            }
        }
        Ok(b.build())
    }

    fn table(&mut self) -> Result<Arc<QTerm>> {
        let mut b = B::new("table");
        self.expect_kw(&mut b, "table")?;
        self.ident_or_splice(&mut b, "table_name")?;
        self.expect(&mut b, Kind::LBrace, "`{`")?;
        self.fields(&mut b)?;
        Ok(b.build())
    }

    fn event(&mut self) -> Result<Arc<QTerm>> {
        let mut b = B::new("event");
        self.expect_kw(&mut b, "emits")?;
        self.ident_or_splice(&mut b, "event_name")?;
        self.expect(&mut b, Kind::LBrace, "`{`")?;
        self.fields(&mut b)?;
        Ok(b.build())
    }

    /// Fields up to and including the closing `}` (the `{` is consumed).
    fn fields(&mut self, b: &mut B) -> Result<()> {
        loop {
            match self.peek().kind {
                Kind::RBrace => {
                    let t = self.bump().clone();
                    b.tok(&t);
                    return Ok(());
                }
                Kind::Eof => return Err(miette!("arch: unterminated field block (missing `}}`)")),
                Kind::Splice => self.splice(b, "field", Some(InnerKind::Stmt)),
                _ => {
                    let f = self.field()?;
                    b.child(f);
                }
            }
        }
    }

    fn field(&mut self) -> Result<Arc<QTerm>> {
        let mut b = B::new("field");
        self.ident_or_splice(&mut b, "field_name")?;
        self.expect(&mut b, Kind::Colon, "`:`")?;
        self.ty(&mut b)?;
        if self.at_kw("pk") {
            let t = self.bump().clone();
            b.tok(&t);
        }
        if self.peek().kind == Kind::Comma {
            let t = self.bump().clone();
            b.tok(&t);
        }
        Ok(b.build())
    }

    fn ty(&mut self, b: &mut B) -> Result<()> {
        match self.peek().kind {
            Kind::Splice => {
                self.splice(b, "type", Some(InnerKind::Expr));
                Ok(())
            }
            Kind::Ident if self.peek().text == "enum" => {
                self.expect_kw(b, "enum")?;
                self.expect(b, Kind::LParen, "`(`")?;
                loop {
                    match self.peek().kind {
                        Kind::Splice => self.splice(b, "variant", Some(InnerKind::Expr)),
                        Kind::Ident => {
                            let t = self.bump().clone();
                            b.tok(&t);
                        }
                        _ => {
                            return Err(miette!(
                                "arch: expected an enum variant, found {}",
                                self.peek().describe()
                            ));
                        }
                    }
                    match self.peek().kind {
                        Kind::Comma => {
                            let t = self.bump().clone();
                            b.tok(&t);
                        }
                        Kind::RParen => {
                            let t = self.bump().clone();
                            b.tok(&t);
                            return Ok(());
                        }
                        _ => {
                            return Err(miette!(
                                "arch: expected `,` or `)`, found {}",
                                self.peek().describe()
                            ));
                        }
                    }
                }
            }
            Kind::Ident => {
                let t = self.bump().clone();
                b.tok(&t);
                Ok(())
            }
            _ => Err(miette!(
                "arch: expected a type, found {}",
                self.peek().describe()
            )),
        }
    }

    fn impl_route(&mut self) -> Result<Arc<QTerm>> {
        let mut b = B::new("impl_route");
        self.expect_kw(&mut b, "impl")?;
        match self.peek().kind {
            Kind::Splice => self.splice(&mut b, "method", Some(InnerKind::Expr)),
            Kind::Ident if matches!(self.peek().text.as_str(), "get" | "post") => {
                let t = self.bump().clone();
                b.tok(&t);
            }
            _ => {
                return Err(miette!(
                    "arch: expected `get` or `post`, found {}",
                    self.peek().describe()
                ));
            }
        }
        match self.peek().kind {
            Kind::Splice => self.splice(&mut b, "path", Some(InnerKind::Expr)),
            Kind::Path => {
                let t = self.bump().clone();
                b.tok(&t);
            }
            _ => {
                return Err(miette!(
                    "arch: expected a route path starting with `/`, found {}",
                    self.peek().describe()
                ));
            }
        }
        // The fragment is always a hole at this level: in a `.quilt` context
        // the `↖…↗` body is a nested quote quilt has already extracted.
        if self.peek().kind != Kind::Splice {
            return Err(miette!(
                "arch: expected a `↖ … ↗` fragment after the route path, found {}",
                self.peek().describe()
            ));
        }
        self.splice(&mut b, "impl_fragment", None);
        Ok(b.build())
    }
}

/**************************************************************/

#[derive(Debug)]
struct ArchPost {
    holes: Box<[Hole]>,
    qterm: Arc<QTerm>,
}

impl LanguagePost for ArchPost {
    fn holes(&self) -> &[Hole] {
        &self.holes
    }

    fn parse_post(&self, plugs: &[Arc<QTerm>]) -> Result<Arc<QTerm>> {
        fn fill<'a>(
            qterm: &QTerm,
            plugs: &mut impl Iterator<Item = &'a Arc<QTerm>>,
        ) -> Arc<QTerm> {
            match qterm {
                QTerm::Quote {
                    tag,
                    index,
                    lang,
                    term,
                    cmds,
                    span,
                } => Arc::new(quilt::qterm::qquote_at(
                    tag,
                    *index,
                    lang,
                    fill(term, plugs),
                    cmds,
                    span.clone(),
                )),
                QTerm::Unquote {
                    tag,
                    index,
                    lang,
                    term,
                    cmds,
                    span,
                } => Arc::new(quilt::qterm::qunquote_at(
                    tag,
                    *index,
                    lang,
                    fill(term, plugs),
                    cmds,
                    span.clone(),
                )),
                QTerm::Tuple { tag, terms, cmds } => {
                    if &**tag == HOLE_TAG {
                        return plugs.next().expect("plug per hole").clone();
                    }
                    tuple(
                        tag,
                        &terms.iter().map(|t| fill(t, plugs)).collect::<Vec<_>>(),
                        cmds,
                    )
                }
            }
        }

        if plugs.len() != self.holes.len() {
            return Err(miette!(
                "arch: {} plug(s) for {} hole(s)",
                plugs.len(),
                self.holes.len()
            ));
        }
        Ok(fill(&self.qterm, &mut plugs.iter()))
    }
}

/**************************************************************/

/// The arch language, registered dynamically (never compiled into quilt):
/// `multi.add_lang(metaarch_lang::LANG, bx(ArchLanguage::default()))`.
#[derive(Debug, Default, Clone, Copy)]
pub struct ArchLanguage;

impl Language for ArchLanguage {
    type Post = Box<dyn LanguagePost>;

    fn parse_pre(&mut self, _ikind: Option<InnerKind>, code: &[FlatNode]) -> Result<Self::Post> {
        let toks = lex(code)?;
        let mut p = P {
            toks: &toks,
            pos: 0,
            holes: Vec::new(),
        };
        let qterm = p.top()?;
        if p.peek().kind != Kind::Eof {
            return Err(miette!(
                "arch: trailing input at {}",
                p.peek().describe()
            ));
        }
        Ok(bx(ArchPost {
            holes: p.holes.into(),
            qterm,
        }))
    }

    fn arity(&self, tag: &str) -> Arity {
        match tag {
            // Sibling positions that can absorb emit loops.
            "entry" | "field" | "table" | "service" => Arity::Variadic,
            _ => Arity::Const(1),
        }
    }

    fn typ(&self, tag: &str) -> InnerKind {
        match tag {
            "arch_file" => InnerKind::File,
            "service" | "service_seq" | "database" | "table" | "event" | "impl_route" => {
                InnerKind::Item
            }
            "entry" | "entry_seq" | "field" => InnerKind::Stmt,
            _ => InnerKind::Expr,
        }
    }
}

/**************************************************************/

#[cfg(test)]
mod tests {
    use super::*;
    use quilt::lang::flat_nodes;
    use quilt::prelude::STerm;

    const SHOP: &str = "# A tiny shop.\nsystem shop\n\nservice orders {\n  lang rust\n  port 8081\n  db postgres {\n    table orders {\n      id: uuid pk,\n      status: enum(pending, paid),\n    }\n  }\n  emits OrderPlaced { order_id: uuid }\n}\n\nservice notifier {\n  lang python\n  consumes OrderPlaced\n}\n";

    fn parse_str(src: &str) -> Result<Arc<QTerm>> {
        ArchLanguage.parse(&flat_nodes(src))
    }

    #[test]
    fn round_trips_a_file() {
        let term = parse_str(SHOP).unwrap();
        assert_eq!(term.coparse(), SHOP);
        match &*term {
            QTerm::Tuple { tag, .. } => assert_eq!(&**tag, "arch_file"),
            other => panic!("expected a tuple, got {other:?}"),
        }
    }

    #[test]
    fn round_trips_entry_fragments() {
        for src in ["lang rust\nport 8080\n", "consumes OrderPlaced"] {
            let term = parse_str(src).unwrap();
            assert_eq!(term.coparse(), src);
        }
    }

    #[test]
    fn splices_fill_holes() {
        let code = [
            FlatNode::Str("system "),
            FlatNode::Hole,
            FlatNode::NewLine,
            FlatNode::Str("service s { lang rust }"),
        ];
        let post = ArchLanguage.parse_pre(None, &code).unwrap();
        let holes = post.holes();
        assert_eq!(holes.len(), 1);
        assert_eq!(&*holes[0].otag, "system_name");
        let filled = post.parse_post(&[leaf("identifier", "shop")]).unwrap();
        assert_eq!(filled.coparse(), "system shop\nservice s { lang rust }");
    }

    #[test]
    fn impl_fragment_is_a_hole() {
        let code = [
            FlatNode::Str("service s { lang rust impl get /hello "),
            FlatNode::Hole,
            FlatNode::Str(" }"),
        ];
        let post = ArchLanguage.parse_pre(None, &code).unwrap();
        let holes = post.holes();
        assert_eq!(holes.len(), 1);
        assert_eq!(&*holes[0].otag, "impl_fragment");
        let filled = post
            .parse_post(&[leaf("string_literal", "\"hi\"")])
            .unwrap();
        assert_eq!(
            filled.coparse(),
            "service s { lang rust impl get /hello \"hi\" }"
        );
    }

    #[test]
    fn entry_splices_are_variadic_holes() {
        let code = [
            FlatNode::Str("system s service a { "),
            FlatNode::Hole,
            FlatNode::Str(" }"),
        ];
        let post = ArchLanguage.parse_pre(None, &code).unwrap();
        assert_eq!(&*post.holes()[0].otag, "entry");
        assert_eq!(ArchLanguage.arity("entry"), Arity::Variadic);
    }

    #[test]
    fn rejects_broken_syntax() {
        assert!(parse_str("system s service a speed 9").is_err());
        assert!(parse_str("system s service a { lang rust").is_err());
        // A fragment must be a quote/hole, not inline tokens.
        assert!(parse_str("system s service a { impl get /x hello }").is_err());
    }
}
