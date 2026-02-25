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
use std::collections::{BTreeSet, HashMap};
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
    let idl_dir = project_root.join("target").join("idl");
    let deploy_dir = project_root.join("target").join("deploy");

    let mut checks = Vec::new();
    println!("Running anchor-suite test");
    println!("{:-^60}", " Preflight ");

    if idl_dir.exists() {
        println!("PASS  idl directory found: {}", idl_dir.display());
        checks.push(CheckResult::pass("idl_dir_exists", format!("{}", idl_dir.display())));
    } else {
        println!("FAIL  missing idl directory: {}", idl_dir.display());
        checks.push(CheckResult::fail(
            "idl_dir_exists",
            format!("{}", idl_dir.display()),
            "Run `anchor build` first".to_string(),
        ));
        write_min_report(project_root, &checks)?;
        bail!("Test suite failed");
    }

    if deploy_dir.exists() {
        println!("PASS  deploy directory found: {}", deploy_dir.display());
        checks.push(CheckResult::pass(
            "deploy_dir_exists",
            format!("{}", deploy_dir.display()),
        ));
    } else {
        println!("FAIL  missing deploy directory: {}", deploy_dir.display());
        checks.push(CheckResult::fail(
            "deploy_dir_exists",
            format!("{}", deploy_dir.display()),
            "Run `anchor build` first".to_string(),
        ));
        write_min_report(project_root, &checks)?;
        bail!("Test suite failed");
    }

    let programs = load_program_specs(&idl_dir, &deploy_dir)?;
    if programs.is_empty() {
        checks.push(CheckResult::fail(
            "program_specs_loaded",
            "No testable IDL program specs found".to_string(),
            "Ensure IDL has instructions and matching .so exists in target/deploy".to_string(),
        ));
        write_min_report(project_root, &checks)?;
        bail!("No testable programs found");
    }

    checks.push(CheckResult::pass(
        "program_specs_loaded",
        format!("loaded {} program specs", programs.len()),
    ));

    let smoke = maybe_run_local_smoke(project_root)?;
    if let Some(smoke_result) = &smoke {
        if smoke_result.ok {
            checks.push(CheckResult::pass("optional_smoke_test", smoke_result.detail.clone()));
        } else {
            checks.push(CheckResult::fail(
                "optional_smoke_test",
                smoke_result.detail.clone(),
                "Inspect stderr/stdout in report".to_string(),
            ));
        }
    }

    let generated = generate_edge_cases(&programs);
    println!("{:-^60}", " Generated Cases ");
    println!("generated_edge_cases: {}", generated.len());
    checks.push(CheckResult::pass(
        "edge_case_generation",
        format!("generated {} idl-driven cases", generated.len()),
    ));

    println!("{:-^60}", " Case Execution ");
    let executed = execute_edge_cases(&programs, &generated)?;
    let case_passed = executed.iter().filter(|c| c.passed).count();
    let case_failed = executed.len().saturating_sub(case_passed);
    println!("executed_cases: {}", executed.len());
    println!("case_passed: {}", case_passed);
    println!("case_failed: {}", case_failed);

    if case_failed == 0 {
        checks.push(CheckResult::pass(
            "generated_case_execution",
            format!("all {} generated cases matched expectations", executed.len()),
        ));
    } else {
        checks.push(CheckResult::fail(
            "generated_case_execution",
            format!("{} of {} generated cases did not match expectations", case_failed, executed.len()),
            "Inspect `executed_cases` in report.json".to_string(),
        ));
    }

    let report_path = write_report(project_root, &checks, &generated, &executed, &smoke)?;
    println!("report: {}", report_path.display());

    println!("{:-^60}", " Summary ");
    println!("checks_failed: {}", checks.iter().filter(|c| !c.ok).count());
    println!("case_passed: {}", case_passed);
    println!("case_failed: {}", case_failed);

    if checks.iter().any(|c| !c.ok) || case_failed > 0 {
        bail!("Test suite failed");
    }

    Ok(())
}

#[derive(Debug)]
struct SmokeResult {
    ok: bool,
    detail: String,
    stdout: String,
    stderr: String,
}

fn maybe_run_local_smoke(project_root: &Path) -> Result<Option<SmokeResult>> {
    let smoke_test = project_root
        .join("programs")
        .join("anchor-testing-suite")
        .join("tests")
        .join("litesvm_test.rs");

    if !smoke_test.exists() {
        return Ok(None);
    }

    println!("{:-^60}", " Optional Smoke Test ");
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
        .context("Failed to execute optional smoke test")?;

    let ok = output.status.success();
    let detail = "cargo test -p anchor-testing-suite --test litesvm_test -- --nocapture".to_string();
    if ok {
        println!("PASS  {}", detail);
    } else {
        println!("FAIL  {}", detail);
    }

    Ok(Some(SmokeResult {
        ok,
        detail,
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
    }))
}

#[derive(Debug)]
struct ProgramSpec {
    idl_file: String,
    program_id: Address,
    deploy_so: PathBuf,
    instructions: Vec<InstructionSpec>,
}

#[derive(Debug, Clone)]
struct InstructionSpec {
    name: String,
    discriminator: Vec<u8>,
    accounts: Vec<AccountSpec>,
    args: Vec<ArgSpec>,
}

#[derive(Debug, Clone)]
struct AccountSpec {
    name: String,
    signer: bool,
    writable: bool,
    pda_seeds: Vec<SeedSpec>,
}

#[derive(Debug, Clone)]
enum SeedSpec {
    Const(Vec<u8>),
    Account(String),
}

#[derive(Debug, Clone)]
struct ArgSpec {
    name: String,
    ty: Value,
}

fn load_program_specs(idl_dir: &Path, deploy_dir: &Path) -> Result<Vec<ProgramSpec>> {
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

#[derive(Debug, Clone)]
struct EdgeCase {
    id: String,
    idl_file: String,
    program_id: Address,
    instruction: InstructionSpec,
    mutation: Mutation,
    expectation: Expectation,
}

#[derive(Debug, Clone)]
enum Mutation {
    None,
    WrongProgramId,
    TruncateData,
    WrongPda { account: String },
}

#[derive(Debug, Clone, Copy)]
enum Expectation {
    MustFail,
    Any,
}

fn generate_edge_cases(programs: &[ProgramSpec]) -> Vec<EdgeCase> {
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

#[derive(Debug)]
struct ExecutedCase {
    id: String,
    idl_file: String,
    instruction: String,
    mutation: String,
    expected_success: Option<bool>,
    actual_success: bool,
    passed: bool,
    error: Option<String>,
}

fn execute_edge_cases(programs: &[ProgramSpec], cases: &[EdgeCase]) -> Result<Vec<ExecutedCase>> {
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

    // Resolve PDA accounts from known account seeds.
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
        let program_pubkey = anchor_lang::prelude::Pubkey::new_from_array(case.program_id.to_bytes());
        let (pda, _) = anchor_lang::prelude::Pubkey::find_program_address(&seed_slices, &program_pubkey);
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

fn write_min_report(project_root: &Path, checks: &[CheckResult]) -> Result<()> {
    let report_dir = project_root.join("target").join("anchor-suite");
    fs::create_dir_all(&report_dir)?;
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

    let report = json!({
        "tool": "anchor-suite",
        "step": 4,
        "checks": checks_json
    });
    fs::write(&report_path, serde_json::to_string_pretty(&report)?)?;
    Ok(())
}

fn write_report(
    project_root: &Path,
    checks: &[CheckResult],
    generated: &[EdgeCase],
    executed: &[ExecutedCase],
    smoke: &Option<SmokeResult>,
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

    let generated_json: Vec<Value> = generated
        .iter()
        .map(|c| {
            json!({
                "id": c.id,
                "idl_file": c.idl_file,
                "program_id": c.program_id.to_string(),
                "instruction": c.instruction.name,
                "mutation": format!("{:?}", c.mutation),
                "expectation": format!("{:?}", c.expectation)
            })
        })
        .collect();

    let executed_json: Vec<Value> = executed
        .iter()
        .map(|c| {
            json!({
                "id": c.id,
                "idl_file": c.idl_file,
                "instruction": c.instruction,
                "mutation": c.mutation,
                "expected_success": c.expected_success,
                "actual_success": c.actual_success,
                "passed": c.passed,
                "error": c.error
            })
        })
        .collect();

    let summary = json!({
        "checks_failed": checks.iter().filter(|c| !c.ok).count(),
        "generated_edge_cases": generated.len(),
        "executed_cases": executed.len(),
        "case_passed": executed.iter().filter(|c| c.passed).count(),
        "case_failed": executed.iter().filter(|c| !c.passed).count()
    });

    let smoke_json = match smoke {
        Some(s) => json!({
            "ok": s.ok,
            "detail": s.detail,
            "stdout": s.stdout,
            "stderr": s.stderr
        }),
        None => json!(null),
    };

    let report = json!({
        "tool": "anchor-suite",
        "step": 4,
        "summary": summary,
        "checks": checks_json,
        "optional_smoke": smoke_json,
        "generated_cases": generated_json,
        "executed_cases": executed_json
    });

    fs::write(&report_path, serde_json::to_string_pretty(&report)?)
        .with_context(|| format!("Failed to write {}", report_path.display()))?;

    Ok(report_path)
}
