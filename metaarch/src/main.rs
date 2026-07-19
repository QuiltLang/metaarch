//! The metaarch CLI: parse, validate, and generate a full distributed
//! system from a `.arch` description — or a `.arch.quilt` one, loaded
//! through the quilt registry (phase 4d). `bin/main` wraps this.

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
    /// Parse and validate a .arch or .arch.quilt file; print diagnostics.
    Check { file: PathBuf },
    /// Parse a .arch or .arch.quilt file and print the SystemSpec.
    Dump { file: PathBuf },
    /// Validate a .arch or .arch.quilt file, then generate the runnable system.
    Generate {
        file: PathBuf,
        /// Output directory for the generated system.
        #[arg(short, long, default_value = "out")]
        out: PathBuf,
    },
    /// Rewrite .arch files in the canonical style (comments survive).
    Fmt {
        files: Vec<PathBuf>,
        /// Don't write; exit nonzero if any file isn't canonically formatted.
        #[arg(long)]
        check: bool,
    },
}

fn fmt(files: &[PathBuf], check_only: bool) -> anyhow::Result<ExitCode> {
    let mut dirty = false;
    for file in files {
        let name = file
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if name.ends_with(".quilt") {
            // A `.arch.quilt` file is quilt syntax (splices, meta blocks);
            // its layout is the quilt toolchain's business, not ours.
            anyhow::bail!("{}: fmt handles plain .arch files only", file.display());
        }
        let src = std::fs::read_to_string(file)
            .with_context(|| format!("failed to read {}", file.display()))?;
        let formatted = metaarch_parser::fmt::format(&src)
            .map_err(|e| anyhow::anyhow!("{}: {e}", file.display()))?;
        if formatted == src {
            continue;
        }
        dirty = true;
        if check_only {
            println!("would reformat {}", file.display());
        } else {
            std::fs::write(file, &formatted)
                .with_context(|| format!("failed to write {}", file.display()))?;
            println!("reformatted {}", file.display());
        }
    }
    Ok(if dirty && check_only {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

/// A loaded architecture: the spec plus the plain `.arch` text it was derived
/// from — the source itself for `.arch` files, the expanded term's coparse
/// for `.arch.quilt` ones. The plain text is what `generate` snapshots as
/// `system.arch` (the snapshot must reparse with the ordinary parser on the
/// next run).
struct Loaded {
    spec: metaarch_spec::SystemSpec,
    plain: String,
}

fn load(file: &PathBuf) -> anyhow::Result<Loaded> {
    let src = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    let name = file
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    if name.ends_with(".quilt") {
        // Only the quilt registry understands a `.arch.quilt` file, so it
        // parses and expands first (identity — arch is data); the ordinary
        // parser then derives the spec from the coparsed text, staying the
        // semantic authority (types, topology, positioned diagnostics).
        let plain = metaarch_expand::arch_text(name, &src)
            .map_err(|e| anyhow::anyhow!("{}: {e}", file.display()))?;
        let spec = metaarch_parser::parse(&plain)
            .map_err(|e| anyhow::anyhow!("{} (expanded): {e}", file.display()))?;
        return Ok(Loaded { spec, plain });
    }
    let spec =
        metaarch_parser::parse(&src).map_err(|e| anyhow::anyhow!("{}: {e}", file.display()))?;
    // Convergence check (phase 4d): a plain `.arch` file must load to the
    // same spec through the quilt registry. Possible only when every
    // fragment carries its language annotation — quilt resolves an
    // un-annotated quote's language from the file-extension chain, which a
    // plain `.arch` file doesn't provide.
    let annotated = spec
        .services
        .iter()
        .flat_map(|s| &s.impls)
        .all(|r| r.frag_lang.is_some());
    if annotated {
        let plain = metaarch_expand::arch_text(name, &src)
            .map_err(|e| anyhow::anyhow!("{}: quilt registry rejects it: {e}", file.display()))?;
        let respec = metaarch_parser::parse(&plain)
            .map_err(|e| anyhow::anyhow!("{}: registry round-trip broke it: {e}", file.display()))?;
        if respec != spec {
            anyhow::bail!(
                "{}: the quilt registry and the ordinary parser disagree on this file (metaarch bug)",
                file.display()
            );
        }
    }
    Ok(Loaded { spec, plain: src })
}

fn check(file: &PathBuf) -> anyhow::Result<ExitCode> {
    let spec = load(file)?.spec;
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
            let spec = load(&file)?.spec;
            println!("{spec:#?}");
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Generate { file, out } => {
            // Validate first so `generate` never runs on a broken architecture.
            let code = check(&file)?;
            if code != ExitCode::SUCCESS {
                return Ok(code);
            }
            let loaded = load(&file)?;
            let spec = loaded.spec;
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
            artifacts.push(metaarch_codegen::Artifact::snapshot(loaded.plain));

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
        Cmd::Fmt { files, check } => fmt(&files, check),
    }
}
