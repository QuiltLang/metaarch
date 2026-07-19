//! DDL generator: one `sql/schema.sql` per `db` block.
//!
//! Plain text rather than a quilt metaprogram — quilt has no SQL grammar yet,
//! and codegen.md keeps SQL-as-text for phase 1. Both engines are spelled
//! from the same tables; the differences live in `sql_ty` and the enum CHECK.
//! `column_def` and `create_table` are shared with the migrations generator,
//! so a column added by an `ALTER` is spelled exactly as it would be in a
//! fresh schema.

use std::fmt::Write as _;

use metaarch_spec::{Database, Engine, Field, Service, SystemSpec, Table, Ty};

use crate::{Artifact, header, sql_ty};

/// One column definition as it appears in `CREATE TABLE` (no trailing comma).
pub(crate) fn column_def(field: &Field, engine: Engine) -> String {
    let mut column = format!("{} {}", field.name, sql_ty(&field.ty, engine));
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
    column
}

/// A full `CREATE TABLE` statement for one table.
pub(crate) fn create_table(table: &Table, engine: Engine) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "CREATE TABLE {} (", table.name);
    let n = table.fields.len();
    for (i, field) in table.fields.iter().enumerate() {
        let comma = if i + 1 < n { "," } else { "" };
        let _ = writeln!(out, "  {}{comma}", column_def(field, engine));
    }
    out.push_str(");\n");
    out
}

pub(crate) fn engine_name(engine: Engine) -> &'static str {
    match engine {
        Engine::Postgres => "postgres",
        Engine::Sqlite => "sqlite",
    }
}

pub(crate) fn schema(spec: &SystemSpec, service: &Service, db: &Database) -> Artifact {
    let mut out = String::new();
    let _ = writeln!(out, "{}", header(&spec.name, "--"));
    let _ = writeln!(
        out,
        "-- Schema for service `{}` ({}).",
        service.name,
        engine_name(db.engine)
    );
    for table in &db.tables {
        out.push('\n');
        out.push_str(&create_table(table, db.engine));
    }
    Artifact::file(format!("{}/sql/schema.sql", service.name), out)
}
