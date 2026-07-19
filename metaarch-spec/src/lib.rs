//! The typed system description: what a `.arch` file parses into.
//!
//! `SystemSpec` is the single source of truth the generators consume. The
//! parser (`metaarch-parser`) produces it; `validate` checks architectural
//! invariants over it before any code is generated.

pub mod validate;

/// Source position of an item in the `.arch` file, for diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub line: u32,
    pub col: u32,
}

impl std::fmt::Display for Span {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "line {}, col {}", self.line, self.col)
    }
}

/// Implementation language of a service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Rust,
    Python,
}

impl std::fmt::Display for Lang {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Lang::Rust => write!(f, "rust"),
            Lang::Python => write!(f, "python"),
        }
    }
}

/// Database engine backing a service's store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    Postgres,
    Sqlite,
}

/// A field type. The set is closed on purpose: every type must have a known
/// mapping in every target (SQL column, Rust type, Python type).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ty {
    Uuid,
    Int,
    Money,
    Text,
    Bool,
    Timestamp,
    Enum(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub name: String,
    pub ty: Ty,
    pub pk: bool,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    pub name: String,
    pub fields: Vec<Field>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Database {
    pub engine: Engine,
    pub tables: Vec<Table>,
    pub span: Span,
}

/// An event a service publishes. Events are the cross-service contract:
/// `consumes` entries anywhere in the system resolve against these by name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventType {
    pub name: String,
    pub fields: Vec<Field>,
    pub span: Span,
}

/// HTTP method of an `impl` route.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Method {
    Get,
    Post,
}

impl Method {
    /// Lowercase spelling, as written in `.arch` source (and axum's routing
    /// function names).
    pub fn verb(&self) -> &'static str {
        match self {
            Method::Get => "get",
            Method::Post => "post",
        }
    }

    /// Uppercase spelling, for HTTP-facing labels (docs, smoke output).
    pub fn upper(&self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
        }
    }
}

impl std::fmt::Display for Method {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.verb())
    }
}

/// An `impl` entry: a route whose handler body is written inline in the
/// `.arch` file between `↖ … ↗` and carried as an opaque, dedented string in
/// the service's implementation language (the phase-4 escape hatch; inline
/// quotes replace the string later). The fragment is the *body* of the
/// handler: in Rust the tail expression of an `impl IntoResponse` fn, in
/// Python a function body that returns the response text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImplRoute {
    pub method: Method,
    pub path: String,
    pub body: String,
    pub span: Span,
}

impl ImplRoute {
    /// The generated handler's name, shared by every generator (and checked
    /// for collisions by validation): `impl_<method>_<path>` with non-
    /// alphanumeric path characters flattened to `_`.
    pub fn handler_name(&self) -> String {
        let sanitized: String = self
            .path
            .trim_start_matches('/')
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();
        format!("impl_{}_{sanitized}", self.method.verb())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Service {
    pub name: String,
    /// `None` until declared; validation requires it.
    pub lang: Option<Lang>,
    pub port: Option<u16>,
    pub db: Option<Database>,
    pub emits: Vec<EventType>,
    /// Event names this service subscribes to, with the reference site.
    pub consumes: Vec<(String, Span)>,
    /// Inline `impl` routes (the phase-4 escape hatch).
    pub impls: Vec<ImplRoute>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemSpec {
    pub name: String,
    pub services: Vec<Service>,
}
