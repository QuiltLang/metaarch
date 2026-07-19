//! Migration generator: diff the previous generated spec against the new one
//! and emit `ALTER` steps per `db` block.
//!
//! The previous spec comes from the `system.arch` copy that `generate` leaves
//! in the output root — the `.arch` file stays the only source of truth, and
//! the diff base is just its last generated snapshot, reparsed with the
//! ordinary parser. `migrations` is a pure function of (previous spec, next
//! spec, existing migration counts); the CLI supplies the counts by listing
//! `sql/migrations/` so numbering continues across runs.
//!
//! What diffs to what:
//! - new table → `CREATE TABLE` (shared spelling with the schema generator)
//! - dropped table → `DROP TABLE`
//! - added column → `ALTER TABLE … ADD COLUMN` (NOT NULL left off: existing
//!   rows need a backfill first; both engines reject it anyway)
//! - dropped column → `ALTER TABLE … DROP COLUMN`
//! - anything else (type change, pk change, enum variants, engine change) →
//!   a `-- manual migration required` comment; renames read as drop + add

use std::collections::BTreeMap;
use std::fmt::Write as _;

use metaarch_spec::{Database, Field, SystemSpec};

use crate::sql::{column_def, create_table, engine_name};
use crate::{Artifact, header, ty_arch};

/// `ADD COLUMN` spelling: the schema column def minus NOT NULL, which neither
/// engine accepts on a populated table without a default.
fn add_column_def(field: &Field, engine: metaarch_spec::Engine) -> String {
    column_def(field, engine).replace(" NOT NULL", "")
}

/// The migration steps for one service's db, or `None` if nothing changed.
fn db_steps(prev: &Database, next: &Database) -> Option<String> {
    let mut out = String::new();
    if prev.engine != next.engine {
        let _ = writeln!(
            out,
            "-- manual migration required: engine changed from {} to {}; recreate from sql/schema.sql",
            engine_name(prev.engine),
            engine_name(next.engine),
        );
        return Some(out);
    }
    let engine = next.engine;
    for table in &next.tables {
        let Some(prev_table) = prev.tables.iter().find(|t| t.name == table.name) else {
            out.push_str(&create_table(table, engine));
            continue;
        };
        for field in &table.fields {
            let Some(prev_field) = prev_table.fields.iter().find(|f| f.name == field.name) else {
                if field.pk {
                    let _ = writeln!(
                        out,
                        "-- manual migration required: cannot add PRIMARY KEY column {} to existing table {}",
                        field.name, table.name,
                    );
                } else {
                    let _ = writeln!(
                        out,
                        "ALTER TABLE {} ADD COLUMN {}; -- NOT NULL deferred: backfill existing rows, then add the constraint",
                        table.name,
                        add_column_def(field, engine),
                    );
                }
                continue;
            };
            if prev_field.ty != field.ty || prev_field.pk != field.pk {
                let _ = writeln!(
                    out,
                    "-- manual migration required: column {}.{} changed from {}{} to {}{}",
                    table.name,
                    field.name,
                    ty_arch(&prev_field.ty),
                    if prev_field.pk { " pk" } else { "" },
                    ty_arch(&field.ty),
                    if field.pk { " pk" } else { "" },
                );
            }
        }
        for prev_field in &prev_table.fields {
            if !table.fields.iter().any(|f| f.name == prev_field.name) {
                let _ = writeln!(out, "ALTER TABLE {} DROP COLUMN {};", table.name, prev_field.name);
            }
        }
    }
    for prev_table in &prev.tables {
        if !next.tables.iter().any(|t| t.name == prev_table.name) {
            let _ = writeln!(out, "DROP TABLE {};", prev_table.name);
        }
    }
    if out.is_empty() { None } else { Some(out) }
}

/// Diff two specs and emit one numbered migration per changed `db` block.
/// `existing` maps service name → how many migrations its `sql/migrations/`
/// already holds; new files continue the numbering.
pub fn migrations(
    prev: &SystemSpec,
    next: &SystemSpec,
    existing: &BTreeMap<String, u32>,
) -> Vec<Artifact> {
    let mut out = Vec::new();
    for service in &next.services {
        let Some(db) = &service.db else { continue };
        // A service (or db block) absent from the previous spec needs no
        // migration: its fresh sql/schema.sql is the starting point.
        let Some(prev_db) = prev
            .services
            .iter()
            .find(|s| s.name == service.name)
            .and_then(|s| s.db.as_ref())
        else {
            continue;
        };
        let Some(steps) = db_steps(prev_db, db) else {
            continue;
        };
        let number = existing.get(service.name.as_str()).copied().unwrap_or(0) + 1;
        let mut contents = format!(
            "{}\n-- Migration {number:04} for service `{}` ({}).\n\n",
            header(&next.name, "--"),
            service.name,
            engine_name(db.engine),
        );
        contents.push_str(&steps);
        out.push(Artifact::file(
            format!("{}/sql/migrations/{number:04}.sql", service.name),
            contents,
        ));
    }
    out
}
