//! Architectural invariants over a parsed `SystemSpec`.
//!
//! The parser guarantees syntax; this module rejects systems that are
//! well-formed text but broken architectures — dangling event references,
//! port collisions, tables without a primary key. Generation refuses to run
//! while any `Error`-severity diagnostic is present.

use std::collections::HashMap;

use crate::{Lang, Method, Service, Span, SystemSpec, Table, Ty};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,
    pub span: Span,
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let sev = match self.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        write!(f, "{sev}: {} ({})", self.message, self.span)
    }
}

fn error(diags: &mut Vec<Diagnostic>, span: Span, message: String) {
    diags.push(Diagnostic {
        severity: Severity::Error,
        message,
        span,
    });
}

fn warning(diags: &mut Vec<Diagnostic>, span: Span, message: String) {
    diags.push(Diagnostic {
        severity: Severity::Warning,
        message,
        span,
    });
}

/// Run every check. Returns all diagnostics; the caller decides what to do
/// with warnings, but must not generate while any `Error` is present.
pub fn validate(spec: &SystemSpec) -> Vec<Diagnostic> {
    let mut diags = Vec::new();

    check_service_names(spec, &mut diags);
    check_ports(spec, &mut diags);
    let events = check_events(spec, &mut diags);
    check_consumes(spec, &events, &mut diags);
    check_impls(spec, &mut diags);
    for service in &spec.services {
        check_service(service, &mut diags);
    }

    diags
}

pub fn has_errors(diags: &[Diagnostic]) -> bool {
    diags.iter().any(|d| d.severity == Severity::Error)
}

fn check_service_names(spec: &SystemSpec, diags: &mut Vec<Diagnostic>) {
    let mut seen: HashMap<&str, Span> = HashMap::new();
    for service in &spec.services {
        if let Some(first) = seen.get(service.name.as_str()) {
            error(
                diags,
                service.span,
                format!(
                    "duplicate service name `{}` (first declared at {first})",
                    service.name
                ),
            );
        } else {
            seen.insert(&service.name, service.span);
        }
    }
}

fn check_ports(spec: &SystemSpec, diags: &mut Vec<Diagnostic>) {
    let mut seen: HashMap<u16, &str> = HashMap::new();
    for service in &spec.services {
        let Some(port) = service.port else { continue };
        if let Some(other) = seen.get(&port) {
            error(
                diags,
                service.span,
                format!(
                    "service `{}` reuses port {port}, already taken by `{other}`",
                    service.name
                ),
            );
        } else {
            seen.insert(port, &service.name);
        }
    }
}

/// Check event declarations and build the global event namespace
/// (event name -> emitting service name).
fn check_events<'a>(
    spec: &'a SystemSpec,
    diags: &mut Vec<Diagnostic>,
) -> HashMap<&'a str, &'a str> {
    let mut events: HashMap<&str, &str> = HashMap::new();
    for service in &spec.services {
        for event in &service.emits {
            if let Some(other) = events.get(event.name.as_str()) {
                error(
                    diags,
                    event.span,
                    format!(
                        "event `{}` is already emitted by service `{other}`; event names are global",
                        event.name
                    ),
                );
            } else {
                events.insert(&event.name, &service.name);
            }
            check_fields(&event.fields, &format!("event `{}`", event.name), diags);
            for field in &event.fields {
                if field.pk {
                    error(
                        diags,
                        field.span,
                        format!("`pk` is not allowed on event field `{}`", field.name),
                    );
                }
            }
        }
    }

    // An emitted event nobody consumes is probably a mistake in the topology.
    for service in &spec.services {
        for event in &service.emits {
            let consumed = spec
                .services
                .iter()
                .any(|s| s.consumes.iter().any(|(name, _)| name == &event.name));
            if !consumed {
                warning(
                    diags,
                    event.span,
                    format!("event `{}` is emitted but never consumed", event.name),
                );
            }
        }
    }

    events
}

fn check_consumes(spec: &SystemSpec, events: &HashMap<&str, &str>, diags: &mut Vec<Diagnostic>) {
    for service in &spec.services {
        for (name, span) in &service.consumes {
            if !events.contains_key(name.as_str()) {
                error(
                    diags,
                    *span,
                    format!(
                        "service `{}` consumes `{name}`, but no service emits it",
                        service.name
                    ),
                );
            }
        }
    }
}

/// Check `impl` routes: they must not shadow a route the generators derive
/// from the spec (health, peers, emit, event delivery), must be unique per
/// service, and their generated handler names must not collide.
fn check_impls(spec: &SystemSpec, diags: &mut Vec<Diagnostic>) {
    let multi = spec.services.len() > 1;
    for service in &spec.services {
        let mut derived: Vec<(Method, String, String)> =
            vec![(Method::Get, "/health".into(), "the health route".into())];
        if multi && service.lang == Some(Lang::Rust) {
            derived.push((Method::Get, "/peers".into(), "the peers route".into()));
        }
        for event in &service.emits {
            derived.push((
                Method::Post,
                format!("/emit/{}", event.name),
                format!("the emit route for event `{}`", event.name),
            ));
        }
        for (name, _) in &service.consumes {
            derived.push((
                Method::Post,
                format!("/events/{name}"),
                format!("the delivery route for event `{name}`"),
            ));
        }

        let mut seen: HashMap<(Method, &str), Span> = HashMap::new();
        let mut names: HashMap<String, &str> = HashMap::new();
        for route in &service.impls {
            let label = format!("{} {}", route.method.upper(), route.path);
            if let Some((_, _, what)) = derived
                .iter()
                .find(|(m, p, _)| *m == route.method && *p == route.path)
            {
                error(
                    diags,
                    route.span,
                    format!("impl route `{label}` collides with {what} the generators derive"),
                );
            }
            if let Some(first) = seen.get(&(route.method, route.path.as_str())) {
                error(
                    diags,
                    route.span,
                    format!("duplicate impl route `{label}` (first declared at {first})"),
                );
            } else {
                seen.insert((route.method, &route.path), route.span);
            }
            if let Some(other) = names.get(&route.handler_name()) {
                error(
                    diags,
                    route.span,
                    format!(
                        "impl routes `{}` and `{}` generate the same handler name `{}`",
                        other,
                        route.path,
                        route.handler_name()
                    ),
                );
            } else {
                names.insert(route.handler_name(), &route.path);
            }
            if route.body.is_empty() {
                warning(
                    diags,
                    route.span,
                    format!("impl route `{label}` has an empty fragment"),
                );
            }
        }
    }
}

fn check_service(service: &Service, diags: &mut Vec<Diagnostic>) {
    if service.lang.is_none() {
        error(
            diags,
            service.span,
            format!("service `{}` has no `lang` declaration", service.name),
        );
    }

    if service.port.is_none()
        && service.db.is_none()
        && service.emits.is_empty()
        && service.consumes.is_empty()
        && service.impls.is_empty()
    {
        warning(
            diags,
            service.span,
            format!(
                "service `{}` has no port, db, emits, or consumes — it does nothing",
                service.name
            ),
        );
    }

    if let Some(db) = &service.db {
        let mut seen: HashMap<&str, Span> = HashMap::new();
        for table in &db.tables {
            if let Some(first) = seen.get(table.name.as_str()) {
                error(
                    diags,
                    table.span,
                    format!(
                        "duplicate table `{}` in service `{}` (first declared at {first})",
                        table.name, service.name
                    ),
                );
            } else {
                seen.insert(&table.name, table.span);
            }
            check_table(table, diags);
        }
    }
}

fn check_table(table: &Table, diags: &mut Vec<Diagnostic>) {
    check_fields(&table.fields, &format!("table `{}`", table.name), diags);

    let pks = table.fields.iter().filter(|f| f.pk).count();
    if pks != 1 {
        error(
            diags,
            table.span,
            format!(
                "table `{}` must have exactly one `pk` field, found {pks}",
                table.name
            ),
        );
    }
}

fn check_fields(fields: &[crate::Field], owner: &str, diags: &mut Vec<Diagnostic>) {
    let mut seen: HashMap<&str, Span> = HashMap::new();
    for field in fields {
        if let Some(first) = seen.get(field.name.as_str()) {
            error(
                diags,
                field.span,
                format!(
                    "duplicate field `{}` in {owner} (first declared at {first})",
                    field.name
                ),
            );
        } else {
            seen.insert(&field.name, field.span);
        }

        if let Ty::Enum(variants) = &field.ty {
            let mut vseen: HashMap<&str, ()> = HashMap::new();
            for variant in variants {
                if vseen.insert(variant, ()).is_some() {
                    error(
                        diags,
                        field.span,
                        format!(
                            "duplicate enum variant `{variant}` in field `{}`",
                            field.name
                        ),
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EventType, Field, Lang, Service, SystemSpec};

    fn span() -> Span {
        Span { line: 1, col: 1 }
    }

    fn service(name: &str) -> Service {
        Service {
            name: name.into(),
            lang: Some(Lang::Rust),
            port: Some(8080),
            db: None,
            emits: Vec::new(),
            consumes: Vec::new(),
            impls: Vec::new(),
            span: span(),
        }
    }

    fn errors(spec: &SystemSpec) -> Vec<String> {
        validate(spec)
            .into_iter()
            .filter(|d| d.severity == Severity::Error)
            .map(|d| d.message)
            .collect()
    }

    #[test]
    fn accepts_minimal_service() {
        let spec = SystemSpec {
            name: "sys".into(),
            services: vec![service("api")],
        };
        assert_eq!(errors(&spec), Vec::<String>::new());
    }

    #[test]
    fn rejects_port_collision() {
        let spec = SystemSpec {
            name: "sys".into(),
            services: vec![service("a"), service("b")],
        };
        let errs = errors(&spec);
        assert_eq!(errs.len(), 1);
        assert!(errs[0].contains("reuses port 8080"), "{errs:?}");
    }

    #[test]
    fn rejects_dangling_consume() {
        let mut consumer = service("worker");
        consumer.port = Some(8081);
        consumer.consumes.push(("Ghost".into(), span()));
        let spec = SystemSpec {
            name: "sys".into(),
            services: vec![service("api"), consumer],
        };
        let errs = errors(&spec);
        assert_eq!(errs.len(), 1);
        assert!(errs[0].contains("no service emits it"), "{errs:?}");
    }

    #[test]
    fn warns_on_unconsumed_event() {
        let mut emitter = service("api");
        emitter.emits.push(EventType {
            name: "Ping".into(),
            fields: vec![Field {
                name: "id".into(),
                ty: Ty::Uuid,
                pk: false,
                span: span(),
            }],
            span: span(),
        });
        let spec = SystemSpec {
            name: "sys".into(),
            services: vec![emitter],
        };
        let diags = validate(&spec);
        assert!(errors(&spec).is_empty());
        assert!(
            diags
                .iter()
                .any(|d| d.severity == Severity::Warning && d.message.contains("never consumed"))
        );
    }

    #[test]
    fn rejects_impl_shadowing_derived_route() {
        let mut svc = service("api");
        svc.impls.push(crate::ImplRoute {
            method: Method::Get,
            path: "/health".into(),
            body: "\"nope\"".into(),
            span: span(),
        });
        let spec = SystemSpec {
            name: "sys".into(),
            services: vec![svc],
        };
        let errs = errors(&spec);
        assert_eq!(errs.len(), 1);
        assert!(errs[0].contains("collides with the health route"), "{errs:?}");
    }

    #[test]
    fn rejects_impl_handler_name_collision() {
        let mut svc = service("api");
        for path in ["/a/b", "/a_b"] {
            svc.impls.push(crate::ImplRoute {
                method: Method::Get,
                path: path.into(),
                body: "\"x\"".into(),
                span: span(),
            });
        }
        let spec = SystemSpec {
            name: "sys".into(),
            services: vec![svc],
        };
        let errs = errors(&spec);
        assert_eq!(errs.len(), 1);
        assert!(errs[0].contains("same handler name"), "{errs:?}");
    }

    #[test]
    fn accepts_impl_route() {
        let mut svc = service("api");
        svc.impls.push(crate::ImplRoute {
            method: Method::Get,
            path: "/hello".into(),
            body: "\"hi\"".into(),
            span: span(),
        });
        let spec = SystemSpec {
            name: "sys".into(),
            services: vec![svc],
        };
        assert_eq!(errors(&spec), Vec::<String>::new());
    }

    #[test]
    fn rejects_table_without_pk() {
        let mut svc = service("api");
        svc.db = Some(crate::Database {
            engine: crate::Engine::Postgres,
            tables: vec![Table {
                name: "users".into(),
                fields: vec![Field {
                    name: "id".into(),
                    ty: Ty::Uuid,
                    pk: false,
                    span: span(),
                }],
                span: span(),
            }],
            span: span(),
        });
        let spec = SystemSpec {
            name: "sys".into(),
            services: vec![svc],
        };
        let errs = errors(&spec);
        assert_eq!(errs.len(), 1);
        assert!(errs[0].contains("exactly one `pk`"), "{errs:?}");
    }
}
