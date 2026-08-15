//! Generator for `process-compose.yaml`: the generated system's fleet as a
//! [process-compose](https://f1bonacc1.github.io/process-compose/) project —
//! declarative supervision (dependency ordering, readiness probes, restart
//! policy, per-process logs and a TUI) without containers, and without the
//! hand-rolled `trap 'kill 0' EXIT` supervision `bin/main` does by itself.
//!
//! Plain text rather than a quilt metaprogram: quilt has no YAML grammar, so
//! this follows the TOML/README precedent in codegen.md — the SQL half of
//! that precedent went away in #43, when quilt grew a SQL target. The shape is
//! small and fixed — a header, `version`, and one `processes` entry per
//! service plus the infrastructure one-shots — so a `writeln!` builder is
//! honest here in a way it would not be for a real language target.
//!
//! What the spec decides: which services exist and in what order, each one's
//! language (the command and working directory), and its port (the readiness
//! probe's URL). What it deliberately does *not* decide is the dependency
//! graph between services — see `depends_on` below.

use std::fmt::Write as _;

use metaarch_spec::{Lang, SystemSpec};

use crate::{Artifact, header};

/// The process-compose config schema version the emitted file declares.
pub(crate) const VERSION: &str = "0.5";

// Supervision tuning. The root flake's `fleet-pc` app describes the same
// fleet over the *built* services, so it reads these rather than repeating
// the numbers — the `column_def`/`create_table` sharing rule, applied to a
// second spelling of one policy.

/// Grace before the first `/health` poll. A service that is already built
/// binds its port in well under a second.
pub(crate) const PROBE_INITIAL_DELAY_SECONDS: u16 = 1;
/// How often to poll `/health` once probing starts.
pub(crate) const PROBE_PERIOD_SECONDS: u16 = 2;
/// How long one `/health` request may take before it counts as a failure.
pub(crate) const PROBE_TIMEOUT_SECONDS: u16 = 2;
/// One good answer means ready; `/health` is a liveness route with no state
/// to settle.
pub(crate) const PROBE_SUCCESS_THRESHOLD: u16 = 1;
/// 30 × the period before a service is declared unready: a `cargo run` of an
/// already-built workspace is fast, but a first postgres connection on a
/// loaded machine is not.
pub(crate) const PROBE_FAILURE_THRESHOLD: u16 = 30;
/// Restart policy for a service: crashes are retried, a clean exit is not.
pub(crate) const RESTART_POLICY: &str = "on_failure";
/// Pause between restarts.
pub(crate) const RESTART_BACKOFF_SECONDS: u16 = 2;
/// Restarts before process-compose gives up on a service.
pub(crate) const MAX_RESTARTS: u16 = 5;

/// A double-quoted YAML scalar. Service names are validated identifiers and
/// every path here is generator-built, so the escaping is defensive only —
/// but quoting unconditionally keeps a name like `no` or `on` from being read
/// as a YAML boolean.
fn q(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// A process name that no service can collide with. `service db { … }` is a
/// legal system description, and two entries with the same key would make the
/// emitted YAML mean something other than what the spec says — so the
/// infrastructure processes take `base`, prefixed with `_` until it is free.
/// Derived from the spec like everything else, so it stays deterministic.
fn reserved(base: &str, spec: &SystemSpec) -> String {
    let mut name = base.to_string();
    while spec.services.iter().any(|s| s.name == name) {
        name.insert(0, '_');
    }
    name
}

/// The infrastructure one-shots every service waits for: database
/// provisioning (when the spec declares any `db`) and one workspace build
/// (when it declares any Rust service). Both are exactly what `bin/main` runs
/// before booting the fleet, hoisted into processes so process-compose owns
/// the ordering — and so the TUI shows their output instead of swallowing it.
fn one_shots(spec: &SystemSpec) -> Vec<(String, &'static str, &'static str)> {
    let mut out = Vec::new();
    if spec.services.iter().any(|s| s.db.is_some()) {
        out.push((
            reserved("db", spec),
            "bin/db up",
            "provision the declared databases (idempotent)",
        ));
    }
    if spec.services.iter().any(|s| s.lang == Some(Lang::Rust)) {
        out.push((
            reserved("build", spec),
            "cargo build",
            "compile the rust workspace once, before the services start",
        ));
    }
    out
}

pub(crate) fn process_compose(spec: &SystemSpec) -> Artifact {
    let mut out = String::new();
    let _ = writeln!(out, "{}", header(&spec.name, "#"));
    let _ = writeln!(
        out,
        "# The `{}` fleet as a process-compose project: `bin/main --process-compose`",
        spec.name
    );
    out.push_str("# (or `process-compose -U up` from this directory). Containers optional —\n");
    out.push_str("# these are the same local processes `bin/main` starts, supervised.\n");
    out.push_str("#\n");
    out.push_str("# -U runs the process-compose API over a unix socket; its TCP default is\n");
    out.push_str("# :8080, which a service in this system may want for itself. The socket\n");
    out.push_str("# path is pinned by the .envrc (PC_SOCKET_PATH), so a second terminal can\n");
    out.push_str("# `process-compose -U process list` / `graph` / `down` this fleet.\n");
    let _ = writeln!(out, "version: {}\n", q(VERSION));
    out.push_str("processes:\n");

    let one_shots = one_shots(spec);
    for (name, command, description) in &one_shots {
        let _ = writeln!(out, "  {name}:");
        let _ = writeln!(out, "    description: {}", q(description));
        let _ = writeln!(out, "    command: {}", q(command));
        let _ = writeln!(out, "    working_dir: {}", q("."));
        out.push_str("    availability:\n");
        let _ = writeln!(out, "      restart: {}", q("no"));
    }

    for (index, service) in spec.services.iter().enumerate() {
        let lang = service.lang.expect("validated: service has a lang");
        let port = crate::service_port(service, index);
        // Mirrors `bin/main`: rust services run from the workspace root,
        // python ones from their own package directory (`-u`: unbuffered, so
        // event logs reach the TUI as they happen).
        let (command, working_dir) = match lang {
            Lang::Rust => (format!("cargo run -q -p {}", service.name), ".".to_string()),
            Lang::Python => (
                format!("python3 -u -m {}", service.name),
                service.name.clone(),
            ),
        };
        let _ = writeln!(out, "  {}:", service.name);
        let _ = writeln!(
            out,
            "    description: {}",
            q(&format!(
                "{lang} service on {}",
                crate::service_addr(service, index)
            ))
        );
        let _ = writeln!(out, "    command: {}", q(&command));
        let _ = writeln!(out, "    working_dir: {}", q(&working_dir));
        // Only the infrastructure one-shots are dependencies. The event
        // topology is deliberately *not* a dependency graph: nothing in
        // validation forbids two services consuming each other's events, and
        // a cycle here would deadlock `process-compose up` on a system that
        // `metaarch check` accepts. Readiness carries the health story
        // instead — the transport already retries.
        if !one_shots.is_empty() {
            out.push_str("    depends_on:\n");
            for (name, _, _) in &one_shots {
                let _ = writeln!(out, "      {name}:");
                let _ = writeln!(
                    out,
                    "        condition: {}",
                    q("process_completed_successfully")
                );
            }
        }
        // The `/health` route every generated service serves, which is also
        // what `bin/smoke` waits on — so "ready" means the same thing to the
        // TUI as it does to the smoke test.
        out.push_str("    readiness_probe:\n");
        out.push_str("      http_get:\n");
        let _ = writeln!(out, "        host: {}", q("127.0.0.1"));
        let _ = writeln!(out, "        scheme: {}", q("http"));
        let _ = writeln!(out, "        path: {}", q("/health"));
        let _ = writeln!(out, "        port: {port}");
        let _ = writeln!(
            out,
            "      initial_delay_seconds: {PROBE_INITIAL_DELAY_SECONDS}"
        );
        let _ = writeln!(out, "      period_seconds: {PROBE_PERIOD_SECONDS}");
        let _ = writeln!(out, "      timeout_seconds: {PROBE_TIMEOUT_SECONDS}");
        let _ = writeln!(out, "      success_threshold: {PROBE_SUCCESS_THRESHOLD}");
        let _ = writeln!(out, "      failure_threshold: {PROBE_FAILURE_THRESHOLD}");
        out.push_str("    availability:\n");
        let _ = writeln!(out, "      restart: {}", q(RESTART_POLICY));
        let _ = writeln!(out, "      backoff_seconds: {RESTART_BACKOFF_SECONDS}");
        let _ = writeln!(out, "      max_restarts: {MAX_RESTARTS}");
    }
    Artifact::file("process-compose.yaml", out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use metaarch_spec::{Database, Engine, Service, Span, Table};

    fn zero() -> Span {
        Span { line: 0, col: 0 }
    }

    fn service(name: &str, lang: Lang, port: Option<u16>) -> Service {
        Service {
            name: name.into(),
            lang: Some(lang),
            port,
            db: None,
            emits: Vec::new(),
            consumes: Vec::new(),
            impls: Vec::new(),
            span: zero(),
        }
    }

    fn database() -> Database {
        Database {
            engine: Engine::Postgres,
            tables: vec![Table {
                name: "orders".into(),
                fields: Vec::new(),
                span: zero(),
            }],
            span: zero(),
        }
    }

    /// A two-language system with a database — the shop's shape.
    fn spec() -> SystemSpec {
        let mut orders = service("orders", Lang::Rust, Some(8081));
        orders.db = Some(database());
        SystemSpec {
            name: "shop".into(),
            services: vec![orders, service("notifier", Lang::Python, None)],
        }
    }

    fn yaml(spec: &SystemSpec) -> String {
        process_compose(spec).contents
    }

    #[test]
    fn one_process_per_service_in_declaration_order() {
        let out = yaml(&spec());
        let orders = out.find("\n  orders:").expect("orders process");
        let notifier = out.find("\n  notifier:").expect("notifier process");
        assert!(orders < notifier, "declaration order not preserved:\n{out}");
        assert!(out.contains("command: \"cargo run -q -p orders\""), "{out}");
        assert!(out.contains("command: \"python3 -u -m notifier\""), "{out}");
        assert!(out.contains("working_dir: \"notifier\""), "{out}");
    }

    #[test]
    fn readiness_probes_the_spec_derived_port() {
        let out = yaml(&spec());
        // orders declares 8081; notifier declares nothing and takes the
        // `9000 + index` fallback every other generator addresses it by.
        assert!(out.contains("port: 8081"), "{out}");
        assert!(out.contains("port: 9001"), "{out}");
        assert!(out.contains("path: \"/health\""), "{out}");
    }

    #[test]
    fn services_wait_for_the_infrastructure_one_shots() {
        let out = yaml(&spec());
        assert!(out.contains("command: \"bin/db up\""), "{out}");
        assert!(out.contains("command: \"cargo build\""), "{out}");
        assert_eq!(
            out.matches("condition: \"process_completed_successfully\"")
                .count(),
            4,
            "each of two services waits for both one-shots:\n{out}"
        );
    }

    #[test]
    fn no_db_block_means_no_db_process() {
        let spec = SystemSpec {
            name: "t".into(),
            services: vec![service("api", Lang::Rust, None)],
        };
        let out = yaml(&spec);
        assert!(!out.contains("bin/db up"), "{out}");
        assert!(out.contains("command: \"cargo build\""), "{out}");
        assert_eq!(out.matches("condition:").count(), 1, "{out}");
    }

    #[test]
    fn python_only_systems_get_no_build_process() {
        let spec = SystemSpec {
            name: "t".into(),
            services: vec![service("worker", Lang::Python, None)],
        };
        let out = yaml(&spec);
        assert!(!out.contains("cargo"), "{out}");
        assert!(!out.contains("depends_on"), "{out}");
    }

    /// `service db { … }` is a legal system description; the one-shot has to
    /// move rather than emit a duplicate YAML key.
    #[test]
    fn a_service_named_db_does_not_collide_with_the_db_process() {
        let mut db_service = service("db", Lang::Python, None);
        db_service.db = Some(database());
        let spec = SystemSpec {
            name: "t".into(),
            services: vec![db_service],
        };
        let out = yaml(&spec);
        assert_eq!(out.matches("\n  db:").count(), 1, "duplicate key:\n{out}");
        assert!(out.contains("\n  _db:"), "{out}");
        assert!(out.contains("      _db:\n        condition:"), "{out}");
    }

    #[test]
    fn generation_is_deterministic() {
        assert_eq!(yaml(&spec()), yaml(&spec()));
    }
}
