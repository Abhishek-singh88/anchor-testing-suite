use crate::types::{
    EdgeCase, ExecutedCase, Expectation, InstructionSpec, Mutation, ProgramSpec, SeedSpec,
};
use anyhow::{Context, Result};
use litesvm::LiteSVM;
use serde_json::Value;
use solana_address::Address;
use solana_instruction::{account_meta::AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_message::Message;
use solana_signer::Signer;
use solana_transaction::Transaction;
use std::collections::HashMap;
use std::fs;

pub fn generate_edge_cases(programs: &[ProgramSpec]) -> Vec<EdgeCase> {
    let mut cases = Vec::new();

    for p in programs {
        for ix in &p.instructions {
            cases.push(EdgeCase {
                id: format!("{}_{}_base", p.idl_file, ix.name),
                idl_file: p.idl_file.clone(),
                program_id: p.program_id,
                instruction: ix.clone(),
                mutation: Mutation::None,
                expectation: Expectation::Any,
            });

            cases.push(EdgeCase {
                id: format!("{}_{}_wrong_program", p.idl_file, ix.name),
                idl_file: p.idl_file.clone(),
                program_id: p.program_id,
                instruction: ix.clone(),
                mutation: Mutation::WrongProgramId,
                expectation: Expectation::MustFail,
            });

            cases.push(EdgeCase {
                id: format!("{}_{}_truncate_data", p.idl_file, ix.name),
                idl_file: p.idl_file.clone(),
                program_id: p.program_id,
                instruction: ix.clone(),
                mutation: Mutation::TruncateData,
                expectation: Expectation::MustFail,
            });

            for acc in &ix.accounts {
                if !acc.pda_seeds.is_empty() {
                    cases.push(EdgeCase {
                        id: format!("{}_{}_wrong_pda_{}", p.idl_file, ix.name, acc.name),
                        idl_file: p.idl_file.clone(),
                        program_id: p.program_id,
                        instruction: ix.clone(),
                        mutation: Mutation::WrongPda {
                            account: acc.name.clone(),
                        },
                        expectation: Expectation::MustFail,
                    });
                }
            }
        }
    }

    cases
}

pub fn execute_edge_cases(programs: &[ProgramSpec], cases: &[EdgeCase]) -> Result<Vec<ExecutedCase>> {
    let mut program_bytes = HashMap::new();
    for p in programs {
        let bytes = fs::read(&p.deploy_so)
            .with_context(|| format!("Failed to read {}", p.deploy_so.display()))?;
        program_bytes.insert(p.program_id, bytes);
    }

    let mut out = Vec::with_capacity(cases.len());
    for case in cases {
        let bytes = match program_bytes.get(&case.program_id) {
            Some(v) => v,
            None => continue,
        };
        let run = run_case(bytes, case);
        let (actual_success, error) = match run {
            Ok(()) => (true, None),
            Err(e) => (false, Some(e)),
        };

        let (expected_success, passed) = match case.expectation {
            Expectation::Any => (None, true),
            Expectation::MustFail => (Some(false), !actual_success),
        };

        out.push(ExecutedCase {
            id: case.id.clone(),
            idl_file: case.idl_file.clone(),
            instruction: case.instruction.name.clone(),
            mutation: match &case.mutation {
                Mutation::None => "none".to_string(),
                Mutation::WrongProgramId => "wrong_program_id".to_string(),
                Mutation::TruncateData => "truncate_data".to_string(),
                Mutation::WrongPda { account } => format!("wrong_pda:{}", account),
            },
            expected_success,
            actual_success,
            passed,
            error,
        });
    }

    Ok(out)
}

fn run_case(program_bytes: &[u8], case: &EdgeCase) -> std::result::Result<(), String> {
    let mut svm = LiteSVM::new();
    svm.add_program(case.program_id, program_bytes)
        .map_err(|e| format!("add_program failed: {e:?}"))?;

    let payer = Keypair::new();
    svm.airdrop(&payer.pubkey(), 10_000_000_000)
        .map_err(|e| format!("airdrop failed: {e:?}"))?;

    let (account_metas, signer_keys) = build_accounts(case, &payer)?;
    let mut data = encode_instruction_data(&case.instruction)?;

    if matches!(case.mutation, Mutation::TruncateData) && !data.is_empty() {
        data.pop();
    }

    let program_id = match case.mutation {
        Mutation::WrongProgramId => Address::from(Keypair::new().pubkey().to_bytes()),
        _ => case.program_id,
    };

    let ix = Instruction {
        program_id,
        accounts: account_metas,
        data,
    };

    send_ix(&mut svm, &payer, &signer_keys, ix)
}

fn build_accounts(
    case: &EdgeCase,
    payer: &Keypair,
) -> std::result::Result<(Vec<AccountMeta>, Vec<Keypair>), String> {
    let mut signer_by_name: HashMap<String, Keypair> = HashMap::new();
    let mut pubkey_by_name: HashMap<String, Address> = HashMap::new();

    for acc in &case.instruction.accounts {
        if acc.signer {
            if pubkey_by_name.is_empty() {
                pubkey_by_name.insert(acc.name.clone(), payer.pubkey());
            } else {
                let kp = Keypair::new();
                pubkey_by_name.insert(acc.name.clone(), kp.pubkey());
                signer_by_name.insert(acc.name.clone(), kp);
            }
        } else {
            pubkey_by_name.insert(acc.name.clone(), Keypair::new().pubkey());
        }
    }

    for acc in &case.instruction.accounts {
        if acc.pda_seeds.is_empty() {
            continue;
        }

        let mut seeds: Vec<Vec<u8>> = Vec::new();
        let mut resolvable = true;
        for seed in &acc.pda_seeds {
            match seed {
                SeedSpec::Const(bytes) => seeds.push(bytes.clone()),
                SeedSpec::Account(path) => match pubkey_by_name.get(path) {
                    Some(pk) => seeds.push(pk.to_bytes().to_vec()),
                    None => {
                        resolvable = false;
                        break;
                    }
                },
            }
        }

        if !resolvable {
            continue;
        }

        let seed_slices = seeds.iter().map(Vec::as_slice).collect::<Vec<_>>();
        let program_pubkey =
            anchor_lang::prelude::Pubkey::new_from_array(case.program_id.to_bytes());
        let (pda, _) =
            anchor_lang::prelude::Pubkey::find_program_address(&seed_slices, &program_pubkey);
        pubkey_by_name.insert(acc.name.clone(), Address::from(pda.to_bytes()));
    }

    if let Mutation::WrongPda { account } = &case.mutation {
        if pubkey_by_name.contains_key(account) {
            pubkey_by_name.insert(account.clone(), Keypair::new().pubkey());
        }
    }

    let mut metas = Vec::new();
    let mut extra_signers = Vec::new();

    for acc in &case.instruction.accounts {
        let key = pubkey_by_name
            .get(&acc.name)
            .copied()
            .unwrap_or_else(|| Keypair::new().pubkey());

        let meta = if acc.writable {
            AccountMeta::new(key, acc.signer)
        } else {
            AccountMeta::new_readonly(key, acc.signer)
        };
        metas.push(meta);

        if let Some(kp) = signer_by_name.remove(&acc.name) {
            extra_signers.push(kp);
        }
    }

    Ok((metas, extra_signers))
}

fn encode_instruction_data(ix: &InstructionSpec) -> std::result::Result<Vec<u8>, String> {
    let mut out = ix.discriminator.clone();
    for arg in &ix.args {
        let bytes = encode_arg_zero(&arg.ty)
            .map_err(|e| format!("arg {} type not supported: {}", arg.name, e))?;
        out.extend(bytes);
    }
    Ok(out)
}

fn encode_arg_zero(ty: &Value) -> std::result::Result<Vec<u8>, &'static str> {
    if let Some(s) = ty.as_str() {
        return match s {
            "bool" => Ok(vec![0]),
            "u8" | "i8" => Ok(vec![0]),
            "u16" | "i16" => Ok(vec![0; 2]),
            "u32" | "i32" => Ok(vec![0; 4]),
            "u64" | "i64" => Ok(vec![0; 8]),
            "u128" | "i128" => Ok(vec![0; 16]),
            "pubkey" => Ok(vec![0; 32]),
            _ => Err("primitive not supported"),
        };
    }

    if let Some(obj) = ty.as_object() {
        if let Some(arr_ty) = obj.get("array") {
            let inner = arr_ty
                .as_array()
                .and_then(|a| if a.len() == 2 { Some((&a[0], &a[1])) } else { None })
                .ok_or("invalid array type")?;
            let inner_bytes = encode_arg_zero(inner.0)?;
            let len = inner.1.as_u64().ok_or("invalid array len")? as usize;
            return Ok(inner_bytes.into_iter().cycle().take(len).collect());
        }
    }

    Err("complex arg type not supported")
}

fn send_ix(
    svm: &mut LiteSVM,
    payer: &Keypair,
    extra_signers: &[Keypair],
    ix: Instruction,
) -> std::result::Result<(), String> {
    let blockhash = svm.latest_blockhash();
    let msg = Message::new_with_blockhash(&[ix], Some(&payer.pubkey()), &blockhash);

    let mut signers: Vec<&Keypair> = Vec::with_capacity(1 + extra_signers.len());
    signers.push(payer);
    for s in extra_signers {
        signers.push(s);
    }

    let tx = Transaction::new(&signers, msg, blockhash);
    svm.send_transaction(tx)
        .map(|_| ())
        .map_err(|e| format!("transaction failed: {e:?}"))
}
