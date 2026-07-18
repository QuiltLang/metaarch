//! The metaarch CLI: parse, validate, and (eventually) generate a full
//! distributed system from a `.arch` description. `bin/main` wraps this.

use std::path::PathBuf;
use std::process::ExitCode;

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
    /// Generate the system into an output directory (not implemented yet;
    /// see docs/wiki/plan.md phase 1).
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
            // Validate first so `generate` never runs on a broken architecture,
            // then hand off to metaarch-codegen (phase 1, not implemented yet).
            let code = check(&file)?;
            if code != ExitCode::SUCCESS {
                return Ok(code);
            }
            anyhow::bail!(
                "`generate` is not implemented yet (docs/wiki/plan.md, phase 1); \
                 output would be written to {}",
                out.display()
            );
        }
    }
}
