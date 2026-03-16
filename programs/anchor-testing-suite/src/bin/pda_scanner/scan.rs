use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

// Lightweight PDA summary used for display output.
#[derive(Debug)]
struct PdaInfo {
    program_id: String,
    account_name: String,
    seeds: Value,
}

// Scan IDL files and print PDA accounts derived from instruction metadata.
pub fn scan_pdas(project_dir: &str) -> Result<()> {
    // Anchor generates IDL JSON at `target/idl`.
    let idl_dir = Path::new(project_dir).join("target").join("idl");
    if !idl_dir.exists() {
        bail!("No IDL directory found at {}. Run `anchor build` first.", idl_dir.display());
    }

    // Deduplicate by `program_id:account_name`.
    let mut pdas = Vec::new();
    let mut seen = BTreeSet::new();

    for entry in fs::read_dir(&idl_dir)? {
        let path = entry?.path();
        // Only process IDL JSON files.
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }

        // Parse IDL.
        let idl_content = fs::read_to_string(&path)
            .with_context(|| format!("Failed reading {}", path.display()))?;
        let idl: Value = serde_json::from_str(&idl_content)
            .with_context(|| format!("Invalid JSON in {}", path.display()))?;

        // Program id is stored either at root or metadata in some IDLs.
        let program_id = idl["address"]
            .as_str()
            .or_else(|| idl["metadata"]["address"].as_str())
            .unwrap_or("<unknown_program>");

        // PDA metadata is stored per-instruction account entry.
        if let Some(instructions) = idl["instructions"].as_array() {
            for instruction in instructions {
                if let Some(accounts) = instruction["accounts"].as_array() {
                    for account in accounts {
                        if let Some(seeds) = account["pda"]["seeds"].as_array() {
                            // Collect PDA definition from seeds.
                            let account_name = account["name"].as_str().unwrap_or("<unknown_account>");
                            let key = format!("{}:{}", program_id, account_name);
                            if seen.insert(key) {
                                pdas.push(PdaInfo {
                                    program_id: program_id.to_string(),
                                    account_name: account_name.to_string(),
                                    seeds: Value::Array(seeds.clone()),
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    // Print results.
    println!("Found {} PDAs:", pdas.len());
    println!("{:-^60}", " PDAs ");
    for pda in pdas {
        println!(
            "Program: {} | Account: {} | Seeds: {:?}",
            pda.program_id, pda.account_name, pda.seeds
        );
    }

    Ok(())
}
