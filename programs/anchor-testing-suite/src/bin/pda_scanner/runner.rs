use crate::cases::{execute_edge_cases, generate_edge_cases};
use crate::report::{write_min_report, write_report};
use crate::specs::load_program_specs;
use crate::types::{CheckResult, SmokeResult};
use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::Command;

pub fn run_tests(project_dir: &str) -> Result<()> {
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
