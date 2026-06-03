# Changelog

All notable changes to RunAware will be documented in this file.

This project follows semantic versioning after the initial public release.

## 0.1.1 - 2026-06-03

- Forward interrupts to captured processes so `Ctrl+C` frees server ports.
- Mark interrupted captured runs as stopped with exit code 130.
- Update GitHub Actions dependencies.
- Document Claude Code MCP setup.

## 0.1.0 - 2026-06-02

- Initial Rust CLI
- Shell integration for zsh, bash, and fish
- PTY-backed command capture
- SQLite local runtime store
- Runtime source tracking
- Active-run-only query behavior
- Secret redaction
- Error and warning extraction
- Error block grouping
- Runtime summaries
- Search over active logs
- Checkpoints
- Stdio MCP server
- GitHub Actions CI and release packaging
- Install scripts for macOS, Linux, and Windows
