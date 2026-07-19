//! Seed data generator: one `sql/seed.sql` per `db` block, three
//! deterministic rows per table. Values are a pure function of the field's
//! type, name, and row number, so `generate` stays byte-identical run to run.
//! Plain text, like the schema DDL.

use std::fmt::Write as _;

use metaarch_spec::{Database, Engine, Field, Service, SystemSpec, Ty};

use crate::{Artifact, header};

use crate::sql::engine_name;

const ROWS: u32 = 3;

/// The SQL literal seeded into a column for row `row` (1-based).
fn sql_literal(field: &Field, engine: Engine, row: u32) -> String {
    match &field.ty {
        Ty::Uuid => format!("'00000000-0000-0000-0000-{:012}'", row),
        Ty::Int => row.to_string(),
        Ty::Money => format!("{row}00"),
        Ty::Text => format!("'{}_{row}'", field.name),
        Ty::Bool => match (engine, row % 2 == 1) {
            // sqlite stores bools as INTEGER; spell the literal accordingly.
            (Engine::Sqlite, b) => u32::from(b).to_string(),
            (Engine::Postgres, true) => "TRUE".into(),
            (Engine::Postgres, false) => "FALSE".into(),
        },
        Ty::Timestamp => format!("'2026-01-{:02}T00:00:00Z'", row),
        Ty::Enum(variants) => {
            // Cycle the variants so every seed file exercises more than one.
            let v = &variants[(row as usize - 1) % variants.len()];
            format!("'{v}'")
        }
    }
}

pub(crate) fn seed(spec: &SystemSpec, service: &Service, db: &Database) -> Artifact {
    let mut out = String::new();
    let _ = writeln!(out, "{}", header(&spec.name, "--"));
    let _ = writeln!(
        out,
        "-- Seed data for service `{}` ({}). Apply after sql/schema.sql.",
        service.name,
        engine_name(db.engine)
    );
    for table in &db.tables {
        out.push('\n');
        let columns = table
            .fields
            .iter()
            .map(|f| f.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        for row in 1..=ROWS {
            let values = table
                .fields
                .iter()
                .map(|f| sql_literal(f, db.engine, row))
                .collect::<Vec<_>>()
                .join(", ");
            let _ = writeln!(out, "INSERT INTO {} ({columns}) VALUES ({values});", table.name);
        }
    }
    Artifact::file(format!("{}/sql/seed.sql", service.name), out)
}
