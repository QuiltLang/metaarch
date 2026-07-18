//! DDL generator: one `sql/schema.sql` per `db` block.
//!
//! Plain text rather than a quilt metaprogram — quilt has no SQL grammar yet,
//! and codegen.md keeps SQL-as-text for phase 1. Both engines are spelled
//! from the same tables; the differences live in `sql_ty` and the enum CHECK.

use std::fmt::Write as _;

use metaarch_spec::{Database, Service, SystemSpec, Ty};

use crate::{Artifact, header, sql_ty};

pub(crate) fn schema(spec: &SystemSpec, service: &Service, db: &Database) -> Artifact {
    let mut out = String::new();
    let _ = writeln!(out, "{}", header(&spec.name, "--"));
    let _ = writeln!(
        out,
        "-- Schema for service `{}` ({engine}).",
        service.name,
        engine = match db.engine {
            metaarch_spec::Engine::Postgres => "postgres",
            metaarch_spec::Engine::Sqlite => "sqlite",
        }
    );
    for table in &db.tables {
        let _ = writeln!(out, "\nCREATE TABLE {} (", table.name);
        let n = table.fields.len();
        for (i, field) in table.fields.iter().enumerate() {
            let mut column = format!("  {} {}", field.name, sql_ty(&field.ty, db.engine));
            if field.pk {
                column.push_str(" PRIMARY KEY");
            } else {
                column.push_str(" NOT NULL");
            }
            // The closed type set maps `enum` onto TEXT plus a CHECK, the
            // spelling both engines accept.
            if let Ty::Enum(variants) = &field.ty {
                let list = variants
                    .iter()
                    .map(|v| format!("'{v}'"))
                    .collect::<Vec<_>>()
                    .join(", ");
                let _ = write!(column, " CHECK ({} IN ({list}))", field.name);
            }
            if i + 1 < n {
                column.push(',');
            }
            let _ = writeln!(out, "{column}");
        }
        let _ = writeln!(out, ");");
    }
    Artifact::file(format!("{}/sql/schema.sql", service.name), out)
}
