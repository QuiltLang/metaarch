//! The metaarch CLI: parse, validate, and generate a full distributed
//! system from a `.arch` description. `bin/main` wraps this.

use std::path::PathBuf;
use std::process::ExitCode;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use anyhow::Context;
use clap::{Parser, Subcommand};
use metaarch_spec::validate::{has_errors, validate};

#[derive(Parser)]
#[command(
    name = "metaarch",
    about = "A distributed system compiler built on quilt"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Parse and validate a .arch file; print diagnostics.
    Check { file: PathBuf },
    /// Parse a .arch file and print the SystemSpec.
    Dump { file: PathBuf },
    /// Validate a .arch file, then generate the runnable system.
    Generate {
        file: PathBuf,
        /// Output directory for the generated system.
        #[arg(short, long, default_value = "out")]
        out: PathBuf,
    },
}

fn load(file: &PathBuf) -> anyhow::Result<metaarch_spec::SystemSpec> {
    let src = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    metaarch_parser::parse(&src).map_err(|e| anyhow::anyhow!("{}: {e}", file.display()))
}

fn check(file: &PathBuf) -> anyhow::Result<ExitCode> {
    let spec = load(file)?;
    let mut diags = validate(&spec);
    // Fragment syntax lives with the generators (they own the real
    // grammars); a malformed `impl` body fails `check`, not `generate`.
    diags.extend(metaarch_codegen::check_fragments(&spec));
    for diag in &diags {
        eprintln!("{diag}");
    }
    if has_errors(&diags) {
        Ok(ExitCode::FAILURE)
    } else {
        println!(
            "ok: system `{}` — {} services, {} valid",
            spec.name,
            spec.services.len(),
            if diags.is_empty() {
                "fully"
            } else {
                "with warnings"
            }
        );
        Ok(ExitCode::SUCCESS)
    }
}

/// How many migrations each service's `sql/migrations/` already holds, so
/// new ones continue the numbering across `generate` runs.
fn migration_counts(
    root: &std::path::Path,
    spec: &metaarch_spec::SystemSpec,
) -> std::collections::BTreeMap<String, u32> {
    let mut counts = std::collections::BTreeMap::new();
    for service in &spec.services {
        let dir = root.join(&service.name).join("sql/migrations");
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let n = entries
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|x| x == "sql"))
            .count() as u32;
        if n > 0 {
            counts.insert(service.name.clone(), n);
        }
    }
    counts
}

fn main() -> anyhow::Result<ExitCode> {
    match Cli::parse().cmd {
        Cmd::Check { file } => check(&file),
        Cmd::Dump { file } => {
            let spec = load(&file)?;
            println!("{spec:#?}");
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Generate { file, out } => {
            // Validate first so `generate` never runs on a broken architecture.
            let code = check(&file)?;
            if code != ExitCode::SUCCESS {
                return Ok(code);
            }
            let spec = load(&file)?;
            let root = out.join(&spec.name);

            // Migrations diff against the `system.arch` snapshot the previous
            // `generate` left in the output root (the source of truth is still
            // the `.arch` file — the snapshot is just its last generated
            // state). First generate: no snapshot, no migrations.
            let mut artifacts = metaarch_codegen::generate(&spec);
            let snapshot = root.join("system.arch");
            if let Ok(prev_src) = std::fs::read_to_string(&snapshot) {
                match metaarch_parser::parse(&prev_src) {
                    Ok(prev) => {
                        let existing = migration_counts(&root, &spec);
                        artifacts.extend(metaarch_codegen::migrations(&prev, &spec, &existing));
                    }
                    Err(e) => eprintln!(
                        "warning: {} does not parse ({e}); skipping migrations",
                        snapshot.display()
                    ),
                }
            }
            artifacts.push(metaarch_codegen::Artifact::snapshot(std::fs::read_to_string(
                &file,
            )?));

            for artifact in artifacts {
                let path = root.join(&artifact.path);
                if let Some(dir) = path.parent() {
                    std::fs::create_dir_all(dir)
                        .with_context(|| format!("failed to create {}", dir.display()))?;
                }
                std::fs::write(&path, &artifact.contents)
                    .with_context(|| format!("failed to write {}", path.display()))?;
                #[cfg(unix)]
                if artifact.executable {
                    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))?;
                }
                println!("  wrote {}", path.display());
            }
            println!(
                "generated system `{}` — boot it with `cd {} && bin/main`",
                spec.name,
                root.display()
            );
            Ok(ExitCode::SUCCESS)
        }
    }
}
