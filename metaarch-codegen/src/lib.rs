//! Generators that turn a validated `SystemSpec` into a runnable system.
//!
//! The language-emitting generators are quilt metaprograms: `.rs.quilt`
//! sources in this crate, expanded to `.rs` siblings by `bin/expand` (the
//! nanobots pattern) and compiled in below. SQL and config files are built as
//! plain text here — quilt has no SQL/TOML grammar (yet). See
//! docs/wiki/codegen.md for the generated-system layout.
//!
//! `generate` is a pure function of the spec: same input, byte-identical
//! output. No timestamps, no hash-map iteration.

use metaarch_spec::{Engine, EventType, Lang, Service, SystemSpec, Ty};

mod sql;

// Expanded from the `.rs.quilt` siblings by `bin/expand`; gitignored.
mod python_service;
mod rust_service;
mod system;

/// One generated file, relative to the generated system's root.
pub struct Artifact {
    pub path: String,
    pub contents: String,
    pub executable: bool,
}

impl Artifact {
    /// A regular file. Contents are normalized to end with a newline.
    pub(crate) fn file(path: impl Into<String>, contents: impl Into<String>) -> Self {
        let mut contents = contents.into();
        if !contents.ends_with('\n') {
            contents.push('\n');
        }
        Artifact {
            path: path.into(),
            contents,
            executable: false,
        }
    }

    /// An executable file (scripts under `bin/`).
    pub(crate) fn script(path: impl Into<String>, contents: impl Into<String>) -> Self {
        let mut artifact = Self::file(path, contents);
        artifact.executable = true;
        artifact
    }
}

/// Generate every artifact for a validated spec. Call only after
/// `metaarch_spec::validate` reports no errors: generators rely on the
/// invariants it establishes (every service has a lang, every consumed event
/// is emitted somewhere, tables have exactly one pk).
pub fn generate(spec: &SystemSpec) -> Vec<Artifact> {
    let mut artifacts = system::artifacts(spec);
    for (index, service) in spec.services.iter().enumerate() {
        match service.lang.expect("validated: service has a lang") {
            Lang::Rust => artifacts.extend(rust_service::artifacts(spec, index)),
            Lang::Python => artifacts.extend(python_service::artifacts(spec, index)),
        }
        if let Some(db) = &service.db {
            artifacts.push(sql::schema(spec, service, db));
        }
    }
    artifacts
}

// ---------------------------------------------------------------------------
// Spec queries shared by the generators
// ---------------------------------------------------------------------------

/// The port a service listens on. Services without a declared `port` get a
/// deterministic fallback derived from their position in the file, so the
/// whole topology is addressable without any `.arch` changes.
pub(crate) fn service_port(service: &Service, index: usize) -> u16 {
    service.port.unwrap_or(9000 + index as u16)
}

/// Loopback address of a service; phase 1 runs the whole fleet locally.
pub(crate) fn service_addr(service: &Service, index: usize) -> String {
    format!("127.0.0.1:{}", service_port(service, index))
}

/// Resolve an event name to its declaration (validated: exactly one).
pub(crate) fn event_named<'a>(spec: &'a SystemSpec, name: &str) -> &'a EventType {
    spec.services
        .iter()
        .flat_map(|s| &s.emits)
        .find(|e| e.name == name)
        .expect("validated: consumed event is emitted somewhere")
}

/// Fan-out targets for an event: the address of every consuming service,
/// in declaration order.
pub(crate) fn consumer_addrs(spec: &SystemSpec, event: &str) -> Vec<String> {
    spec.services
        .iter()
        .enumerate()
        .filter(|(_, s)| s.consumes.iter().any(|(name, _)| name == event))
        .map(|(i, s)| service_addr(s, i))
        .collect()
}

/// Every event type a service touches — its own `emits`, then the resolved
/// declarations of its `consumes` — deduplicated, in declaration order.
pub(crate) fn service_events<'a>(spec: &'a SystemSpec, service: &'a Service) -> Vec<&'a EventType> {
    let mut events: Vec<&EventType> = service.emits.iter().collect();
    for (name, _) in &service.consumes {
        if !events.iter().any(|e| &e.name == name) {
            events.push(event_named(spec, name));
        }
    }
    events
}

/// The HTTP path a consumer exposes for an event (the MVP bus transport).
pub(crate) fn event_path(event: &str) -> String {
    format!("/events/{event}")
}

/// The HTTP path an emitter exposes to trigger an event by hand.
pub(crate) fn emit_path(event: &str) -> String {
    format!("/emit/{event}")
}

// ---------------------------------------------------------------------------
// The closed type set, spelled per target
// ---------------------------------------------------------------------------

/// Rust spelling of a field type. `uuid` and `timestamp` travel as strings in
/// phase 1 — the generated services carry no uuid/chrono dependencies; `money`
/// is integer minor units (cents).
pub(crate) fn rust_ty(ty: &Ty) -> &'static str {
    match ty {
        Ty::Uuid => "String",
        Ty::Int => "i64",
        Ty::Money => "i64",
        Ty::Text => "String",
        Ty::Bool => "bool",
        Ty::Timestamp => "String",
        Ty::Enum(_) => "String",
    }
}

/// Python spelling of a field type (same phase-1 conventions as [`rust_ty`]).
pub(crate) fn python_ty(ty: &Ty) -> &'static str {
    match ty {
        Ty::Uuid => "str",
        Ty::Int => "int",
        Ty::Money => "int",
        Ty::Text => "str",
        Ty::Bool => "bool",
        Ty::Timestamp => "str",
        Ty::Enum(_) => "str",
    }
}

/// SQL column type per engine. Enums become TEXT + CHECK (added by the DDL
/// builder); sqlite stores bools as INTEGER and timestamps as TEXT.
pub(crate) fn sql_ty(ty: &Ty, engine: Engine) -> &'static str {
    match (ty, engine) {
        (Ty::Uuid, Engine::Postgres) => "UUID",
        (Ty::Uuid, Engine::Sqlite) => "TEXT",
        (Ty::Int | Ty::Money, Engine::Postgres) => "BIGINT",
        (Ty::Int | Ty::Money, Engine::Sqlite) => "INTEGER",
        (Ty::Text | Ty::Enum(_), _) => "TEXT",
        (Ty::Bool, Engine::Postgres) => "BOOLEAN",
        (Ty::Bool, Engine::Sqlite) => "INTEGER",
        (Ty::Timestamp, Engine::Postgres) => "TIMESTAMPTZ",
        (Ty::Timestamp, Engine::Sqlite) => "TEXT",
    }
}

/// The standard do-not-edit header, per comment syntax.
pub(crate) fn header(system: &str, comment: &str) -> String {
    format!("{comment} Generated by metaarch from the `{system}` system description. Do not edit.")
}
