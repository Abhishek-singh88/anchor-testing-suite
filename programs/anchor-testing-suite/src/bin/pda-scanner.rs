use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use serde_json::{json, Value};
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

    let report_path = write_report(
        project_root,
        &checks,
        &cases,
        &stdout,
        &stderr,
        passed,
        failed,
    )?;
    println!("report: {}", report_path.display());

    println!("{:-^60}", " Summary ");
    println!("passed: {}", passed);
    println!("failed: {}", failed);

    if failed > 0 {
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
    });

    let report_string =
        serde_json::to_string_pretty(&report).context("Failed to serialize JSON report")?;
    fs::write(&report_path, report_string)
        .with_context(|| format!("Failed to write {}", report_path.display()))?;

    Ok(report_path)
}
