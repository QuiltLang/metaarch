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
    let diags = validate(&spec);
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
            for artifact in metaarch_codegen::generate(&spec) {
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
