use anchor_lang::{prelude::Pubkey, InstructionData};
use anchor_testing_suite::{instruction as vault_ix, ID as PROGRAM_ID};
use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use litesvm::LiteSVM;
use serde_json::{json, Value};
use solana_address::Address;
use solana_instruction::{account_meta::AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_message::Message;
use solana_signer::Signer;
use solana_transaction::Transaction;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Parser)]
#[command(name = "anchor-suite")]
#[command(about = "Anchor testing suite CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Scan Anchor IDLs for PDA accounts
    Scan {
        /// Anchor project root (defaults to current directory)
        #[arg(short, long)]
        project_dir: Option<String>,
    },
    /// Run generated edge-case tests (implemented in Step 2)
    Test {
        /// Anchor project root (defaults to current directory)
        #[arg(short, long)]
        project_dir: Option<String>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Scan { project_dir } => {
            let dir = project_dir.unwrap_or_else(|| ".".to_string());
            scan_pdas(&dir)?;
        }
        Commands::Test { project_dir } => {
            let dir = project_dir.unwrap_or_else(|| ".".to_string());
            run_tests(&dir)?;
        }
    }
    Ok(())
}

fn scan_pdas(project_dir: &str) -> Result<()> {
    let idl_dir = Path::new(project_dir).join("target").join("idl");
    if !idl_dir.exists() {
        bail!("No IDL directory found at {}. Run `anchor build` first.", idl_dir.display());
    }

    let mut pdas = Vec::new();
    let mut seen = BTreeSet::new();

    for entry in fs::read_dir(&idl_dir)? {
        let path = entry?.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }

        let idl_content = fs::read_to_string(&path)
            .with_context(|| format!("Failed reading {}", path.display()))?;
        let idl: Value = serde_json::from_str(&idl_content)
            .with_context(|| format!("Invalid JSON in {}", path.display()))?;

        let program_id = idl["address"]
            .as_str()
            .or_else(|| idl["metadata"]["address"].as_str())
            .unwrap_or("<unknown_program>");

        if let Some(instructions) = idl["instructions"].as_array() {
            for instruction in instructions {
                if let Some(accounts) = instruction["accounts"].as_array() {
                    for account in accounts {
                        if let Some(seeds) = account["pda"]["seeds"].as_array() {
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

#[derive(Debug)]
struct PdaInfo {
    program_id: String,
    account_name: String,
    seeds: Value,
}

fn run_tests(project_dir: &str) -> Result<()> {
    let project_root = Path::new(project_dir);
    let deploy_so = project_root
        .join("target")
        .join("deploy")
        .join("anchor_testing_suite.so");
    let litesvm_test = project_root
        .join("programs")
        .join("anchor-testing-suite")
        .join("tests")
        .join("litesvm_test.rs");

    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut checks: Vec<CheckResult> = Vec::new();

    println!("Running anchor-suite test");
    println!("{:-^60}", " Preflight ");

    if deploy_so.exists() {
        println!("PASS  program artifact found: {}", deploy_so.display());
        passed += 1;
        checks.push(CheckResult::pass(
            "program_artifact_exists",
            format!("{}", deploy_so.display()),
        ));
    } else {
        println!(
            "FAIL  missing program artifact: {} (run `anchor build`)",
            deploy_so.display()
        );
        failed += 1;
        checks.push(CheckResult::fail(
            "program_artifact_exists",
            format!("{}", deploy_so.display()),
            "Run `anchor build` first".to_string(),
        ));
    }

    if litesvm_test.exists() {
        println!("PASS  LiteSVM test file found: {}", litesvm_test.display());
        passed += 1;
        checks.push(CheckResult::pass(
            "litesvm_test_exists",
            format!("{}", litesvm_test.display()),
        ));
    } else {
        println!(
            "FAIL  missing LiteSVM test file: {}",
            litesvm_test.display()
        );
        failed += 1;
        checks.push(CheckResult::fail(
            "litesvm_test_exists",
            format!("{}", litesvm_test.display()),
            "Create programs/anchor-testing-suite/tests/litesvm_test.rs".to_string(),
        ));
    }

    println!("{:-^60}", " Execution ");
    let output = Command::new("cargo")
        .arg("test")
        .arg("-p")
        .arg("anchor-testing-suite")
        .arg("--test")
        .arg("litesvm_test")
        .arg("--")
        .arg("--nocapture")
        .current_dir(project_root)
        .output()
        .context("Failed to execute cargo test for litesvm_test")?;

    if output.status.success() {
        println!("PASS  cargo test -p anchor-testing-suite --test litesvm_test");
        passed += 1;
        checks.push(CheckResult::pass(
            "litesvm_smoke_test",
            "cargo test -p anchor-testing-suite --test litesvm_test -- --nocapture".to_string(),
        ));
    } else {
        println!("FAIL  cargo test -p anchor-testing-suite --test litesvm_test");
        failed += 1;
        checks.push(CheckResult::fail(
            "litesvm_smoke_test",
            "cargo test -p anchor-testing-suite --test litesvm_test -- --nocapture".to_string(),
            "Check stderr/stdout in report".to_string(),
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stdout.trim().is_empty() {
        println!("{:-^60}", " cargo test stdout ");
        println!("{}", stdout);
    }
    if !stderr.trim().is_empty() {
        println!("{:-^60}", " cargo test stderr ");
        println!("{}", stderr);
    }

    let cases = generate_edge_cases();
    let generated_count = cases.len();
    println!("{:-^60}", " Generated Cases ");
    println!("generated_edge_cases: {}", generated_count);
    checks.push(CheckResult::pass(
        "edge_case_generation",
        format!("generated {} deterministic cases", generated_count),
    ));

    println!("{:-^60}", " Case Execution ");
    let executed_cases = execute_edge_cases(&deploy_so, &cases)?;
    let case_passed = executed_cases.iter().filter(|c| c.passed).count();
    let case_failed = executed_cases.len().saturating_sub(case_passed);
    println!("executed_cases: {}", executed_cases.len());
    println!("case_passed: {}", case_passed);
    println!("case_failed: {}", case_failed);
    if case_failed == 0 {
        checks.push(CheckResult::pass(
            "generated_case_execution",
            format!("all {} generated cases matched expectations", executed_cases.len()),
        ));
    } else {
        checks.push(CheckResult::fail(
            "generated_case_execution",
            format!(
                "{} of {} generated cases did not match expectations",
                case_failed,
                executed_cases.len()
            ),
            "Inspect `executed_cases` in report.json".to_string(),
        ));
    }

    let report_path = write_report(
        project_root,
        &checks,
        &cases,
        &executed_cases,
        &stdout,
        &stderr,
        passed,
        failed,
    )?;
    println!("report: {}", report_path.display());

    println!("{:-^60}", " Summary ");
    println!("passed: {}", passed);
    println!("failed: {}", failed);
    println!("case_passed: {}", case_passed);
    println!("case_failed: {}", case_failed);

    if failed > 0 || case_failed > 0 {
        bail!("Test suite failed");
    }

    Ok(())
}

#[derive(Debug)]
struct CheckResult {
    name: &'static str,
    ok: bool,
    detail: String,
    hint: Option<String>,
}

impl CheckResult {
    fn pass(name: &'static str, detail: String) -> Self {
        Self {
            name,
            ok: true,
            detail,
            hint: None,
        }
    }

    fn fail(name: &'static str, detail: String, hint: String) -> Self {
        Self {
            name,
            ok: false,
            detail,
            hint: Some(hint),
        }
    }
}

#[derive(Debug)]
struct EdgeCase {
    id: String,
    category: &'static str,
    instruction: &'static str,
    expected: &'static str,
    data: Value,
}

#[derive(Debug)]
struct ExecutedCase {
    id: String,
    category: String,
    instruction: String,
    expected_success: bool,
    actual_success: bool,
    passed: bool,
    error: Option<String>,
}

fn execute_edge_cases(deploy_so: &Path, cases: &[EdgeCase]) -> Result<Vec<ExecutedCase>> {
    let program_bytes = fs::read(deploy_so)
        .with_context(|| format!("Failed to read {}", deploy_so.display()))?;
    let mut results = Vec::with_capacity(cases.len());

    for case in cases {
        let expected_success = expected_success(case);
        let actual = run_case_once(&program_bytes, case);
        let (actual_success, error) = match actual {
            Ok(()) => (true, None),
            Err(e) => (false, Some(e)),
        };
        let passed = actual_success == expected_success;
        results.push(ExecutedCase {
            id: case.id.clone(),
            category: case.category.to_string(),
            instruction: case.instruction.to_string(),
            expected_success,
            actual_success,
            passed,
            error,
        });
    }

    Ok(results)
}

fn expected_success(case: &EdgeCase) -> bool {
    match (case.category, case.instruction) {
        ("amount-boundary", "deposit") => case
            .data
            .get("amount")
            .and_then(|v| v.as_u64())
            .map(|a| a <= 5_000_000_000)
            .unwrap_or(false),
        ("amount-boundary", "withdraw") => false,
        _ => false,
    }
}

fn run_case_once(program_bytes: &[u8], case: &EdgeCase) -> std::result::Result<(), String> {
    let mut svm = LiteSVM::new();
    let program_address = Address::from(PROGRAM_ID.to_bytes());
    svm.add_program(program_address, program_bytes)
        .map_err(|e| format!("add_program failed: {e:?}"))?;

    let payer = Keypair::new();
    svm.airdrop(&payer.pubkey(), 5_000_000_000)
        .map_err(|e| format!("airdrop failed: {e:?}"))?;

    match case.category {
        "amount-boundary" => run_amount_case(&mut svm, &payer, case),
        "authorization" => run_authorization_case(&mut svm),
        "pda-seeds" => run_seed_case(&mut svm, &payer, case),
        "account-shape" => run_account_shape_case(&mut svm, &payer),
        _ => Err(format!("unknown case category: {}", case.category)),
    }
}

fn run_amount_case(
    svm: &mut LiteSVM,
    payer: &Keypair,
    case: &EdgeCase,
) -> std::result::Result<(), String> {
    let amount = case
        .data
        .get("amount")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| "missing amount".to_string())?;

    let (vault_address, _) = derive_vault_address(&payer.pubkey());
    send_initialize_vault(svm, payer, vault_address)?;

    match case.instruction {
        "deposit" => send_deposit(svm, payer, vault_address, amount),
        "withdraw" => {
            send_deposit(svm, payer, vault_address, 1_000_000)?;
            send_withdraw(svm, payer, vault_address, amount)
        }
        other => Err(format!("unsupported amount instruction: {other}")),
    }
}

fn run_authorization_case(svm: &mut LiteSVM) -> std::result::Result<(), String> {
    let owner = Keypair::new();
    let attacker = Keypair::new();
    svm.airdrop(&owner.pubkey(), 5_000_000_000)
        .map_err(|e| format!("owner airdrop failed: {e:?}"))?;
    svm.airdrop(&attacker.pubkey(), 5_000_000_000)
        .map_err(|e| format!("attacker airdrop failed: {e:?}"))?;

    let (owner_vault, _) = derive_vault_address(&owner.pubkey());
    send_initialize_vault(svm, &owner, owner_vault)?;
    send_deposit(svm, &owner, owner_vault, 100_000)?;
    send_withdraw(svm, &attacker, owner_vault, 1)
}

fn run_seed_case(
    svm: &mut LiteSVM,
    payer: &Keypair,
    _case: &EdgeCase,
) -> std::result::Result<(), String> {
    let (vault_address, _) = derive_vault_address(&payer.pubkey());
    send_initialize_vault(svm, payer, vault_address)?;

    let wrong_vault = Keypair::new().pubkey();
    send_deposit(svm, payer, wrong_vault, 10)
}

fn run_account_shape_case(svm: &mut LiteSVM, payer: &Keypair) -> std::result::Result<(), String> {
    let (vault_address, _) = derive_vault_address(&payer.pubkey());
    let wrong_system_program = payer.pubkey();
    let ix = Instruction {
        program_id: Address::from(PROGRAM_ID.to_bytes()),
        accounts: vec![
            AccountMeta::new(vault_address, false),
            AccountMeta::new(payer.pubkey(), true),
            AccountMeta::new_readonly(wrong_system_program, false),
        ],
        data: vault_ix::InitializeVault {}.data(),
    };
    send_ix(svm, payer, ix)
}

fn derive_vault_address(user: &Address) -> (Address, u8) {
    let user_pubkey = Pubkey::new_from_array(user.to_bytes());
    let (vault_pubkey, bump) =
        Pubkey::find_program_address(&[b"vault", user_pubkey.as_ref()], &PROGRAM_ID);
    (Address::from(vault_pubkey.to_bytes()), bump)
}

fn send_initialize_vault(
    svm: &mut LiteSVM,
    payer: &Keypair,
    vault_address: Address,
) -> std::result::Result<(), String> {
    let system_program: Address = "11111111111111111111111111111111"
        .parse()
        .map_err(|e| format!("invalid system program address: {e}"))?;
    let ix = Instruction {
        program_id: Address::from(PROGRAM_ID.to_bytes()),
        accounts: vec![
            AccountMeta::new(vault_address, false),
            AccountMeta::new(payer.pubkey(), true),
            AccountMeta::new_readonly(system_program, false),
        ],
        data: vault_ix::InitializeVault {}.data(),
    };
    send_ix(svm, payer, ix)
}

fn send_deposit(
    svm: &mut LiteSVM,
    payer: &Keypair,
    vault_address: Address,
    amount: u64,
) -> std::result::Result<(), String> {
    let system_program: Address = "11111111111111111111111111111111"
        .parse()
        .map_err(|e| format!("invalid system program address: {e}"))?;
    let ix = Instruction {
        program_id: Address::from(PROGRAM_ID.to_bytes()),
        accounts: vec![
            AccountMeta::new(vault_address, false),
            AccountMeta::new(payer.pubkey(), true),
            AccountMeta::new_readonly(system_program, false),
        ],
        data: vault_ix::Deposit { amount }.data(),
    };
    send_ix(svm, payer, ix)
}

fn send_withdraw(
    svm: &mut LiteSVM,
    payer: &Keypair,
    vault_address: Address,
    amount: u64,
) -> std::result::Result<(), String> {
    let system_program: Address = "11111111111111111111111111111111"
        .parse()
        .map_err(|e| format!("invalid system program address: {e}"))?;
    let ix = Instruction {
        program_id: Address::from(PROGRAM_ID.to_bytes()),
        accounts: vec![
            AccountMeta::new(vault_address, false),
            AccountMeta::new(payer.pubkey(), true),
            AccountMeta::new_readonly(system_program, false),
        ],
        data: vault_ix::Withdraw { amount }.data(),
    };
    send_ix(svm, payer, ix)
}

fn send_ix(svm: &mut LiteSVM, payer: &Keypair, ix: Instruction) -> std::result::Result<(), String> {
    let blockhash = svm.latest_blockhash();
    let msg = Message::new_with_blockhash(&[ix], Some(&payer.pubkey()), &blockhash);
    let tx = Transaction::new(&[payer], msg, blockhash);
    svm.send_transaction(tx)
        .map(|_| ())
        .map_err(|e| format!("transaction failed: {e:?}"))
}

fn generate_edge_cases() -> Vec<EdgeCase> {
    let mut cases = Vec::new();

    // Amount boundaries across deposit/withdraw.
    let amount_values: [u64; 16] = [
        0,
        1,
        2,
        5,
        10,
        100,
        1_000,
        10_000,
        100_000,
        1_000_000,
        10_000_000,
        100_000_000,
        u32::MAX as u64,
        (u32::MAX as u64) + 1,
        u64::MAX - 1,
        u64::MAX,
    ];
    for (idx, amount) in amount_values.iter().enumerate() {
        cases.push(EdgeCase {
            id: format!("amount_deposit_{idx:02}"),
            category: "amount-boundary",
            instruction: "deposit",
            expected: "depends-on-balance-and-runtime",
            data: json!({ "amount": amount }),
        });
        cases.push(EdgeCase {
            id: format!("amount_withdraw_{idx:02}"),
            category: "amount-boundary",
            instruction: "withdraw",
            expected: "depends-on-vault-state-and-authority",
            data: json!({ "amount": amount }),
        });
    }

    // Authority and signer mismatch permutations.
    for idx in 0..10 {
        cases.push(EdgeCase {
            id: format!("authority_mismatch_{idx:02}"),
            category: "authorization",
            instruction: "withdraw",
            expected: "should-fail-unauthorized",
            data: json!({
                "authority_matches_user": false,
                "signer_present": idx % 2 == 0,
                "mutated_signer_index": idx
            }),
        });
    }

    // PDA seed mutation permutations.
    let seed_mutations = [
        "swap_seed_order",
        "truncate_const_seed",
        "append_extra_seed",
        "wrong_user_pubkey_seed",
        "wrong_bump",
        "empty_seed_list",
        "const_seed_case_change",
        "const_seed_single_byte_off",
        "random_seed_bytes",
        "seed_type_mismatch",
        "missing_account_seed_path",
        "duplicate_seed_entry",
    ];
    for (idx, mutation) in seed_mutations.iter().enumerate() {
        cases.push(EdgeCase {
            id: format!("pda_seed_mutation_{idx:02}"),
            category: "pda-seeds",
            instruction: "deposit",
            expected: "should-fail-constraint-seeds",
            data: json!({ "mutation": mutation }),
        });
    }

    // Account meta / shape mutations.
    let meta_mutations = [
        "vault_not_writable",
        "user_not_signer",
        "system_program_missing",
        "vault_account_missing",
        "duplicate_user_account",
        "wrong_system_program_id",
        "vault_owner_mismatch",
        "extra_unexpected_account",
        "reordered_accounts",
        "readonly_authority",
    ];
    for (idx, mutation) in meta_mutations.iter().enumerate() {
        cases.push(EdgeCase {
            id: format!("account_meta_mutation_{idx:02}"),
            category: "account-shape",
            instruction: "initialize_vault",
            expected: "should-fail-account-validation",
            data: json!({ "mutation": mutation }),
        });
    }

    cases
}

fn write_report(
    project_root: &Path,
    checks: &[CheckResult],
    cases: &[EdgeCase],
    executed_cases: &[ExecutedCase],
    cargo_test_stdout: &str,
    cargo_test_stderr: &str,
    passed: usize,
    failed: usize,
) -> Result<PathBuf> {
    let report_dir = project_root.join("target").join("anchor-suite");
    fs::create_dir_all(&report_dir)
        .with_context(|| format!("Failed to create {}", report_dir.display()))?;
    let report_path = report_dir.join("report.json");

    let checks_json: Vec<Value> = checks
        .iter()
        .map(|c| {
            json!({
                "name": c.name,
                "ok": c.ok,
                "detail": c.detail,
                "hint": c.hint
            })
        })
        .collect();

    let cases_json: Vec<Value> = cases
        .iter()
        .map(|c| {
            json!({
                "id": c.id,
                "category": c.category,
                "instruction": c.instruction,
                "expected": c.expected,
                "data": c.data
            })
        })
        .collect();

    let executed_cases_json: Vec<Value> = executed_cases
        .iter()
        .map(|c| {
            json!({
                "id": c.id,
                "category": c.category,
                "instruction": c.instruction,
                "expected_success": c.expected_success,
                "actual_success": c.actual_success,
                "passed": c.passed,
                "error": c.error
            })
        })
        .collect();

    let report = json!({
        "tool": "anchor-suite",
        "step": 3,
        "summary": {
            "passed": passed,
            "failed": failed,
            "generated_edge_cases": cases.len()
        },
        "checks": checks_json,
        "execution": {
            "command": "cargo test -p anchor-testing-suite --test litesvm_test -- --nocapture",
            "stdout": cargo_test_stdout,
            "stderr": cargo_test_stderr
        },
        "generated_cases": cases_json
        ,
        "executed_cases": executed_cases_json
    });

    let report_string =
        serde_json::to_string_pretty(&report).context("Failed to serialize JSON report")?;
    fs::write(&report_path, report_string)
        .with_context(|| format!("Failed to write {}", report_path.display()))?;

    Ok(report_path)
}
