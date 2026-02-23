use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

#[derive(Parser)]
#[command(name = "anchor-suite")]
#[command(about = "Automated testing suite for Anchor programs")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Scan Anchor project for PDAs
    Scan {
        /// Path to Anchor.toml (default: current directory)
        #[arg(short, long)]
        project_dir: Option<String>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Scan { project_dir } => {
            let project_dir = project_dir.unwrap_or_else(|| ".".to_string());
            scan_pdas(&project_dir)?;
        }
    }
    Ok(())
}

fn scan_pdas(project_dir: &str) -> Result<()> {
    let idl_dir = Path::new(project_dir).join("target").join("idl");
    
    if !idl_dir.exists() {
        anyhow::bail!("No IDL found. Run 'anchor build' first.");
    }

    let mut pdas = vec![];
    let mut seen = BTreeSet::new();

    for entry in fs::read_dir(idl_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("json") {
            let idl_content = fs::read_to_string(&path)?;
            let idl: serde_json::Value = serde_json::from_str(&idl_content)?;
            
            let program_id = idl["address"]
                .as_str()
                .or_else(|| idl["metadata"]["address"].as_str())
                .context("No program ID in IDL")?;

            // Anchor IDL v0.1 stores PDA seed metadata on instruction accounts:
            // instructions[].accounts[].pda.seeds
            if let Some(instructions) = idl["instructions"].as_array() {
                for ix in instructions {
                    if let Some(accounts) = ix["accounts"].as_array() {
                        for account in accounts {
                            if let Some(seeds) = account["pda"]["seeds"].as_array() {
                                let account_name = account["name"]
                                    .as_str()
                                    .unwrap_or("<unknown>")
                                    .to_string();
                                let key = format!("{}:{}", program_id, account_name);
                                if seen.insert(key) {
                                    pdas.push(PdaInfo {
                                        program_id: program_id.to_string(),
                                        account_name,
                                        seeds: serde_json::Value::Array(seeds.clone()),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    println!("Found {} PDAs:", pdas.len());
    println!("{:-^60}", " PDAs ");
    for pda in pdas {
        println!("Program: {} | Account: {} | Seeds: {:?}", 
                pda.program_id, pda.account_name, pda.seeds);
    }

    Ok(())
}

#[derive(Debug)]
struct PdaInfo {
    program_id: String,
    account_name: String,
    seeds: serde_json::Value,
}
