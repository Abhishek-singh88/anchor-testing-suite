use anyhow::Result;              // nice error handling library
use clap::{Parser, Subcommand};  // library for CLI tools

// Case generation + execution pipeline.
#[path = "pda_scanner/cases.rs"]
mod cases;
// Report writer for JSON output.
#[path = "pda_scanner/report.rs"]
mod report;
// Orchestration for the `test` command.
#[path = "pda_scanner/runner.rs"]
mod runner;
// PDA discovery from IDL.
#[path = "pda_scanner/scan.rs"]
mod scan;
// IDL + deploy artifact parsing.
#[path = "pda_scanner/specs.rs"]
mod specs;
// Shared data types across modules.
#[path = "pda_scanner/types.rs"]
mod types;

// CLI definition for `pda-scanner`.
#[derive(Parser)]
#[command(name = "anchor-suite")]
#[command(about = "Anchor testing suite CLI")]
#[command(version = env!("CARGO_PKG_VERSION"))]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

// Subcommands supported by the CLI.
#[derive(Subcommand)]
enum Commands {
    Scan {
        #[arg(short, long)]
        project_dir: Option<String>,
    },
    Test {
        #[arg(short, long)]
        project_dir: Option<String>,
    },
}

// Entry point: route subcommands to the correct module.
fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Scan { project_dir } => {
            let dir = project_dir.unwrap_or_else(|| ".".to_string());
            scan::scan_pdas(&dir)?;
        }
        Commands::Test { project_dir } => {
            let dir = project_dir.unwrap_or_else(|| ".".to_string());
            runner::run_tests(&dir)?;
        }
    }
    Ok(())
}
