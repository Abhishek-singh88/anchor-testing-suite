# Anchor Testing Suite

Anchor Testing Suite is a CLI-first workflow for Anchor/Solana programs that provides:

- PDA discovery from generated IDLs
- Automated mutation-style transaction checks on LiteSVM
- JSON report output for local runs and CI artifact storage

This repository contains:

- An example Anchor program (`programs/anchor-testing-suite/src/lib.rs`)
- A CLI binary (`programs/anchor-testing-suite/src/bin/pda-scanner.rs`)
- LiteSVM smoke test (`programs/anchor-testing-suite/tests/litesvm_test.rs`)
- CI workflow (`.github/workflows/anchor-suite-ci.yml`)

## Problem It Solves

Anchor developers often spend significant time writing repetitive baseline tests and debugging PDA/account issues manually.

This project reduces that effort by:

- reading IDL + deploy artifacts directly
- generating mutation checks automatically
- running those checks in a local VM (LiteSVM)
- producing a structured `report.json` for evidence and debugging

## High-Level Workflow

1. Build your Anchor program (`anchor build`)
2. Run `scan` to inspect PDAs from IDL
3. Run `test` to generate and execute mutation cases
4. Inspect `target/anchor-suite/report.json`

## Repository Structure

- `programs/anchor-testing-suite/src/lib.rs`:
  Example Anchor program (vault/profile-style logic)
- `programs/anchor-testing-suite/src/bin/pda-scanner.rs`:
  CLI implementation (`scan`, `test`)
- `programs/anchor-testing-suite/tests/litesvm_test.rs`:
  LiteSVM smoke test
- `.github/workflows/anchor-suite-ci.yml`:
  GitHub Actions CI (build + test + artifact upload)

## Prerequisites

For local development in this repo:

- Rust/Cargo
- Solana CLI
- Anchor CLI (`0.32.1` recommended in this project)

For testing any Anchor project with the installed CLI:

- Valid Anchor project with `anchor build` output:
  - `target/idl/*.json`
  - `target/deploy/*.so`

## Install CLI

### Option A: Install from GitHub

```bash
cargo install --git https://github.com/Abhishek-singh88/anchor-testing-suite.git \
  anchor-testing-suite \
  --features cli \
  --bin pda-scanner
```

### Option B: Install from local clone

```bash
cargo install --path programs/anchor-testing-suite \
  --features cli \
  --bin pda-scanner
```

### Verify Install

```bash
which pda-scanner
pda-scanner --version
```

Expected version format:

```text
anchor-suite 0.1.0
```

## Command Reference

### 1. Scan PDAs

```bash
pda-scanner scan --project-dir /path/to/anchor-project
```

What it does:

- Reads `target/idl/*.json`
- Extracts PDA metadata from `instructions[].accounts[].pda.seeds`
- Prints discovered PDA accounts and seed definitions

### 2. Run Automated Tests

```bash
pda-scanner test --project-dir /path/to/anchor-project
```

What it does:

- Preflight checks for IDL/deploy directories
- Loads IDLs and matching `.so` binaries
- Generates mutation cases from instruction/account metadata
- Executes cases in LiteSVM
- Writes report to `target/anchor-suite/report.json`

## Local Usage in This Repo

```bash
anchor build
cargo run -p anchor-testing-suite --features cli --bin pda-scanner scan
cargo run -p anchor-testing-suite --features cli --bin pda-scanner test
```

## User Workflow 

Assume user created project `abc`.

```bash
cd abc
anchor build
pda-scanner scan --project-dir .
pda-scanner test --project-dir .
cat target/anchor-suite/report.json
```

Important:

- `scan` is fully IDL-driven
- `test` is now IDL-driven and project-agnostic for supported argument/account patterns

## Report Output

Path:

- `target/anchor-suite/report.json`

Main sections:

- `summary`:
  counts for generated/executed/passed/failed
- `checks`:
  preflight and pipeline status checks
- `optional_smoke`:
  smoke test output if local smoke test file exists
- `generated_cases`:
  generated mutation case definitions
- `executed_cases`:
  per-case results (`actual_success`, `passed`, `error`)

## CI Workflow

Workflow file:

- `.github/workflows/anchor-suite-ci.yml`

Pipeline steps:

1. Checkout
2. Setup Rust + cargo cache
3. Install Solana CLI
4. Install AVM + Anchor CLI
5. `anchor build`
6. `cargo run ... pda-scanner test`
7. Upload artifact: `anchor-suite-report` (`target/anchor-suite/report.json`)

## Troubleshooting

### `pda-scanner: command not found`

Ensure cargo bin path is in shell PATH:

```bash
export PATH="$HOME/.cargo/bin:$PATH"
```

### `No IDL directory found`

Run:

```bash
anchor build
```

inside target project before running `scan`/`test`.

### Install conflict / old binary

```bash
cargo install --force --git https://github.com/Abhishek-singh88/anchor-testing-suite.git \
  anchor-testing-suite \
  --features cli \
  --bin pda-scanner
```

### CI failure at Solana install

If `solana: command not found` appears in CI, ensure workflow step exports Solana path in-step and appends to `$GITHUB_PATH`.

## Limitations (Current Scope)

- Dynamic arg encoding currently supports common primitive/array patterns; very complex IDL arg types may be skipped/fail-fast.
- Mutation strategy focuses on protocol-level negative checks (wrong program, wrong PDA, truncated data) rather than full semantic fuzzing.
- This is a capstone MVP focused on practical, automatable baseline coverage.

## Release / Evidence

Tag used for capstone freeze:

- `v0.1-capstone`

Suggested submission evidence:

- Green GitHub Actions run screenshot
- Downloaded artifact `anchor-suite-report.zip`
- Local `scan`/`test` terminal outputs
- `report.json` excerpt
