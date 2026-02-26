use anyhow::Result;
use clap::{Parser, Subcommand};

#[path = "pda_scanner/cases.rs"]
mod cases;
#[path = "pda_scanner/report.rs"]
mod report;
#[path = "pda_scanner/runner.rs"]
mod runner;
#[path = "pda_scanner/scan.rs"]
mod scan;
#[path = "pda_scanner/specs.rs"]
mod specs;
#[path = "pda_scanner/types.rs"]
mod types;

#[derive(Parser)]
#[command(name = "anchor-suite")]
#[command(about = "Anchor testing suite CLI")]
#[command(version = env!("CARGO_PKG_VERSION"))]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

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
