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

mod migrations;
mod seed;
mod sql;

// Expanded from the `.rs.quilt` siblings by `bin/expand`; gitignored.
mod clients;
mod docs;
mod python_service;
mod rust_service;
mod smoke;
mod system;

pub use migrations::migrations;

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

    /// The byte-identical copy of the source `.arch` file that `generate`
    /// leaves in the output root: provenance for readers, and the diff base
    /// the next run's migrations are computed against.
    pub fn snapshot(source: String) -> Self {
        Artifact {
            path: "system.arch".into(),
            contents: source,
            executable: false,
        }
    }
}

/// Generate every artifact for a validated spec. Call only after
/// `metaarch_spec::validate` reports no errors: generators rely on the
/// invariants it establishes (every service has a lang, every consumed event
/// is emitted somewhere, tables have exactly one pk).
pub fn generate(spec: &SystemSpec) -> Vec<Artifact> {
    let mut artifacts = system::artifacts(spec);
    artifacts.push(Artifact::script("bin/smoke", smoke::bin_smoke(spec)));
    artifacts.push(Artifact::file("docs/index.html", docs::index_html(spec)));
    for (index, service) in spec.services.iter().enumerate() {
        match service.lang.expect("validated: service has a lang") {
            Lang::Rust => artifacts.extend(rust_service::artifacts(spec, index)),
            Lang::Python => artifacts.extend(python_service::artifacts(spec, index)),
        }
        if let Some(db) = &service.db {
            artifacts.push(sql::schema(spec, service, db));
            artifacts.push(seed::seed(spec, service, db));
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

/// CamelCase spelling of a snake_case name (`order_svc` → `OrderSvc`), for
/// spec-named Rust types like the generated client structs.
pub(crate) fn camel(name: &str) -> String {
    name.split('_')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

/// snake_case spelling of a CamelCase name (`OrderPlaced` → `order_placed`),
/// for spec-named Rust methods like the client emit helpers.
pub(crate) fn snake(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 4);
    for (i, c) in name.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// A deterministic sample JSON payload for an event — the same values row 1
/// of the seed data uses — so the smoke test and the docs exercise every
/// event with one canonical example.
pub(crate) fn sample_json(event: &EventType) -> String {
    let fields = event
        .fields
        .iter()
        .map(|f| format!("\"{}\": {}", f.name, sample_json_value(f)))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{{{fields}}}")
}

fn sample_json_value(field: &metaarch_spec::Field) -> String {
    match &field.ty {
        Ty::Uuid => format!("\"00000000-0000-0000-0000-{:012}\"", 1),
        Ty::Int => "1".into(),
        Ty::Money => "100".into(),
        Ty::Text => format!("\"{}_1\"", field.name),
        Ty::Bool => "true".into(),
        Ty::Timestamp => "\"2026-01-01T00:00:00Z\"".into(),
        Ty::Enum(variants) => format!("\"{}\"", variants[0]),
    }
}

/// Human spelling of a field type, as written in `.arch` source — used by
/// docs and migration comments.
pub(crate) fn ty_arch(ty: &Ty) -> String {
    match ty {
        Ty::Uuid => "uuid".into(),
        Ty::Int => "int".into(),
        Ty::Money => "money".into(),
        Ty::Text => "text".into(),
        Ty::Bool => "bool".into(),
        Ty::Timestamp => "timestamp".into(),
        Ty::Enum(variants) => format!("enum({})", variants.join(", ")),
    }
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
