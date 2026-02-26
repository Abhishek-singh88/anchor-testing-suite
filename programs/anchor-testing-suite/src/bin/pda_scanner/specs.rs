use crate::types::{AccountSpec, ArgSpec, InstructionSpec, ProgramSpec, SeedSpec};
use anyhow::{bail, Context, Result};
use serde_json::Value;
use solana_address::Address;
use std::fs;
use std::path::{Path, PathBuf};

pub fn load_program_specs(idl_dir: &Path, deploy_dir: &Path) -> Result<Vec<ProgramSpec>> {
    let deploy_sos: Vec<PathBuf> = fs::read_dir(deploy_dir)?
        .filter_map(|e| e.ok().map(|x| x.path()))
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("so"))
        .collect();

    let mut programs = Vec::new();
    for entry in fs::read_dir(idl_dir)? {
        let path = entry?.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }

        let idl_content = fs::read_to_string(&path)?;
        let idl: Value = serde_json::from_str(&idl_content)
            .with_context(|| format!("Invalid JSON in {}", path.display()))?;

        let program_id_str = match idl["address"]
            .as_str()
            .or_else(|| idl["metadata"]["address"].as_str())
        {
            Some(v) => v,
            None => continue,
        };
        let program_id: Address = match program_id_str.parse() {
            Ok(v) => v,
            Err(_) => continue,
        };

        let idl_file = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("<unknown>")
            .to_string();

        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let meta_name = idl["metadata"]["name"].as_str().unwrap_or("");
        let deploy_so = resolve_so_file(deploy_dir, &deploy_sos, stem, meta_name)?;

        let mut instructions = Vec::new();
        if let Some(ixs) = idl["instructions"].as_array() {
            for ix in ixs {
                if let Some(spec) = parse_instruction(ix) {
                    instructions.push(spec);
                }
            }
        }

        if !instructions.is_empty() {
            programs.push(ProgramSpec {
                idl_file,
                program_id,
                deploy_so,
                instructions,
            });
        }
    }

    Ok(programs)
}

fn resolve_so_file(
    deploy_dir: &Path,
    sos: &[PathBuf],
    stem: &str,
    meta_name: &str,
) -> Result<PathBuf> {
    let norm = |s: &str| s.replace('-', "_");
    let candidates = [format!("{}.so", norm(stem)), format!("{}.so", norm(meta_name))];

    for c in &candidates {
        let p = deploy_dir.join(c);
        if p.exists() {
            return Ok(p);
        }
    }

    if sos.len() == 1 {
        return Ok(sos[0].clone());
    }

    bail!(
        "Could not resolve matching .so in {} for idl stem={} meta_name={}",
        deploy_dir.display(),
        stem,
        meta_name
    )
}

fn parse_instruction(ix: &Value) -> Option<InstructionSpec> {
    let name = ix["name"].as_str()?.to_string();
    let discriminator = ix["discriminator"]
        .as_array()?
        .iter()
        .filter_map(|v| v.as_u64().map(|n| n as u8))
        .collect::<Vec<_>>();
    if discriminator.len() != 8 {
        return None;
    }

    let mut accounts = Vec::new();
    if let Some(accs) = ix["accounts"].as_array() {
        for a in accs {
            let name = a["name"].as_str().unwrap_or("unknown").to_string();
            let signer = a["signer"].as_bool().unwrap_or(false);
            let writable = a["writable"].as_bool().unwrap_or(false);

            let mut pda_seeds = Vec::new();
            if let Some(seeds) = a["pda"]["seeds"].as_array() {
                for s in seeds {
                    match s["kind"].as_str().unwrap_or("") {
                        "const" => {
                            if let Some(bytes) = s["value"].as_array() {
                                let b = bytes
                                    .iter()
                                    .filter_map(|v| v.as_u64().map(|n| n as u8))
                                    .collect::<Vec<_>>();
                                pda_seeds.push(SeedSpec::Const(b));
                            }
                        }
                        "account" => {
                            if let Some(path) = s["path"].as_str() {
                                pda_seeds.push(SeedSpec::Account(path.to_string()));
                            }
                        }
                        _ => {}
                    }
                }
            }

            accounts.push(AccountSpec {
                name,
                signer,
                writable,
                pda_seeds,
            });
        }
    }

    let mut args = Vec::new();
    if let Some(a) = ix["args"].as_array() {
        for arg in a {
            let name = arg["name"].as_str().unwrap_or("arg").to_string();
            let ty = arg["type"].clone();
            args.push(ArgSpec { name, ty });
        }
    }

    Some(InstructionSpec {
        name,
        discriminator,
        accounts,
        args,
    })
}
