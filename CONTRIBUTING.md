# Contributing to RunAware

Thanks for your interest in contributing.

RunAware is early-stage software. The current priority is making local runtime capture reliable, safe, and useful for AI coding agents.

## Development Setup

Install Rust:

```bash
rustc --version
cargo --version
```

Clone and test:

```bash
git clone git@github.com:ganeshpatro321/runaware.git
cd runaware
cargo test --locked
cargo run -- doctor
```

Install the local binary while developing:

```bash
cargo install --path .
```

## Before Opening a PR

Run:

```bash
cargo fmt --check
cargo test --locked
```

If you changed user-facing behavior, update `README.md`.

If you changed release packaging, update files under `packaging/` or `.github/workflows/`.

## Design Principles

- Keep RunAware local-first by default.
- Do not expose raw secrets or environment dumps.
- Prefer active runtime context over historical log dumps.
- Make agent-facing output concise and structured.
- Preserve normal developer workflows where possible.

## Good First Contribution Areas

- Better redaction rules
- Framework-specific error detection
- Shell integration improvements
- Docker log integration
- Log file tailing
- Windows PowerShell integration
- Documentation fixes

## Reporting Bugs

Please include:

- OS and shell
- RunAware version
- command used to start the service
- whether shell integration or explicit capture was used
- output from `runaware sources --json`
- a redacted log snippet if relevant
