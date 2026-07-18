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
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemSpec {
    pub name: String,
    pub services: Vec<Service>,
}
