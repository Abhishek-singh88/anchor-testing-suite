use anchor_lang::prelude::*;
use litesvm::LiteSVM;
use std::path::PathBuf;
use std::str::FromStr;

#[test]
fn test_litesvm_setup() {
    // Initialize LiteSVM (fast in-memory Solana VM)
    let mut svm = LiteSVM::new();
    
    // Load compiled program from workspace target/deploy.
    let workspace_so = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/deploy/anchor_testing_suite.so");
    let local_so = PathBuf::from("target/deploy/anchor_testing_suite.so");
    let program_path = if workspace_so.exists() { workspace_so } else { local_so };
    let program_bytes = std::fs::read(&program_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", program_path.display(), e));
    
    // Deploy program to SVM
    let program_id = Pubkey::from_str("Fg6PaFpoGXkYsidMpWTK6W2BeZ7FEfcYkg476zPFsLnS").unwrap();
    svm.add_program(program_id.to_bytes(), &program_bytes).unwrap();
    
    // Create a test payer pubkey
    let payer = Pubkey::new_unique();
    
    println!("LiteSVM ready! Program deployed: {}", program_id);
    println!("Test payer: {}", payer);
}
