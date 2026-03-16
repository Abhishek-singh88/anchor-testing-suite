use serde_json::Value;
use solana_address::Address;
use std::path::PathBuf;

// Result summary for the optional smoke test run.
#[derive(Debug)]
pub struct SmokeResult {
    pub ok: bool,
    pub detail: String,
    pub stdout: String,
    pub stderr: String,
}

// Parsed representation of one program + its instructions from IDL.
#[derive(Debug)]
pub struct ProgramSpec {
    pub idl_file: String,
    pub program_id: Address,
    pub deploy_so: PathBuf,
    pub instructions: Vec<InstructionSpec>,
}

// Parsed instruction schema from the IDL.
#[derive(Debug, Clone)]
pub struct InstructionSpec {
    pub name: String,
    pub discriminator: Vec<u8>,
    pub accounts: Vec<AccountSpec>,
    pub args: Vec<ArgSpec>,
}

// Account metadata for an instruction (signer, writable, PDA seeds).
#[derive(Debug, Clone)]
pub struct AccountSpec {
    pub name: String,
    pub signer: bool,
    pub writable: bool,
    pub pda_seeds: Vec<SeedSpec>,
}

// Supported PDA seed kinds extracted from the IDL.
#[derive(Debug, Clone)]
pub enum SeedSpec {
    Const(Vec<u8>),
    Account(String),
}

// Instruction argument schema (type is raw IDL JSON for flexible parsing).
#[derive(Debug, Clone)]
pub struct ArgSpec {
    pub name: String,
    pub ty: Value,
}

// One generated mutation case to execute.
#[derive(Debug, Clone)]
pub struct EdgeCase {
    pub id: String,
    pub idl_file: String,
    pub program_id: Address,
    pub instruction: InstructionSpec,
    pub mutation: Mutation,
    pub expectation: Expectation,
}

// Mutation category applied to a base instruction.
#[derive(Debug, Clone)]
pub enum Mutation {
    None,
    WrongProgramId,
    TruncateData,
    WrongPda { account: String },
}

// Expected outcome for a case.
#[derive(Debug, Clone, Copy)]
pub enum Expectation {
    MustFail,
    Any,
}

// Result of executing one mutation case.
#[derive(Debug)]
pub struct ExecutedCase {
    pub id: String,
    pub idl_file: String,
    pub instruction: String,
    pub mutation: String,
    pub expected_success: Option<bool>,
    pub actual_success: bool,
    pub passed: bool,
    pub error: Option<String>,
}

// Preflight and pipeline check results (used in report).
#[derive(Debug)]
pub struct CheckResult {
    pub name: &'static str,
    pub ok: bool,
    pub detail: String,
    pub hint: Option<String>,
}

impl CheckResult {
    // Convenience constructor for passing checks.
    pub fn pass(name: &'static str, detail: String) -> Self {
        Self {
            name,
            ok: true,
            detail,
            hint: None,
        }
    }

    // Convenience constructor for failing checks.
    pub fn fail(name: &'static str, detail: String, hint: String) -> Self {
        Self {
            name,
            ok: false,
            detail,
            hint: Some(hint),
        }
    }
}
