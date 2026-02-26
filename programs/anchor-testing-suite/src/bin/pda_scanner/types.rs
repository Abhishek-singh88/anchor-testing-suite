use serde_json::Value;
use solana_address::Address;
use std::path::PathBuf;

#[derive(Debug)]
pub struct SmokeResult {
    pub ok: bool,
    pub detail: String,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug)]
pub struct ProgramSpec {
    pub idl_file: String,
    pub program_id: Address,
    pub deploy_so: PathBuf,
    pub instructions: Vec<InstructionSpec>,
}

#[derive(Debug, Clone)]
pub struct InstructionSpec {
    pub name: String,
    pub discriminator: Vec<u8>,
    pub accounts: Vec<AccountSpec>,
    pub args: Vec<ArgSpec>,
}

#[derive(Debug, Clone)]
pub struct AccountSpec {
    pub name: String,
    pub signer: bool,
    pub writable: bool,
    pub pda_seeds: Vec<SeedSpec>,
}

#[derive(Debug, Clone)]
pub enum SeedSpec {
    Const(Vec<u8>),
    Account(String),
}

#[derive(Debug, Clone)]
pub struct ArgSpec {
    pub name: String,
    pub ty: Value,
}

#[derive(Debug, Clone)]
pub struct EdgeCase {
    pub id: String,
    pub idl_file: String,
    pub program_id: Address,
    pub instruction: InstructionSpec,
    pub mutation: Mutation,
    pub expectation: Expectation,
}

#[derive(Debug, Clone)]
pub enum Mutation {
    None,
    WrongProgramId,
    TruncateData,
    WrongPda { account: String },
}

#[derive(Debug, Clone, Copy)]
pub enum Expectation {
    MustFail,
    Any,
}

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

#[derive(Debug)]
pub struct CheckResult {
    pub name: &'static str,
    pub ok: bool,
    pub detail: String,
    pub hint: Option<String>,
}

impl CheckResult {
    pub fn pass(name: &'static str, detail: String) -> Self {
        Self {
            name,
            ok: true,
            detail,
            hint: None,
        }
    }

    pub fn fail(name: &'static str, detail: String, hint: String) -> Self {
        Self {
            name,
            ok: false,
            detail,
            hint: Some(hint),
        }
    }
}
