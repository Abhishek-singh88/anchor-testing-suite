use crate::types::{CheckResult, EdgeCase, ExecutedCase, SmokeResult};
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

pub fn write_min_report(project_root: &Path, checks: &[CheckResult]) -> Result<()> {
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

pub fn write_report(
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
