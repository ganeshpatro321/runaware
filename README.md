# RunAware

RunAware gives AI coding agents awareness of what is happening in a developer's local runtime environment.

It captures local process output, identifies runtime sources, redacts obvious secrets, extracts errors and warnings, groups stack traces, tracks checkpoints, and exposes active runtime context to coding agents through MCP and CLI commands.

RunAware is local-first. Runtime data is stored on your machine in SQLite.

## Why

AI coding agents can edit files, but they often cannot see what happened after the code ran. Developers end up copying logs from terminals into chat.

RunAware closes that loop:

```text
developer runs local services
-> RunAware captures safe runtime signals
-> agent queries RunAware
-> agent can debug with current runtime evidence
```

RunAware is not a production observability platform. It is local runtime context for AI-assisted development.

## Project Status

RunAware is early-stage software. The core CLI, shell capture, active-run runtime model, and MCP server are usable, but APIs and storage behavior may change before 1.0.

## Current Features

- Runtime source tracking for services such as `frontend`, `api`, `worker`, and `tests`
- Working-directory tracking per source
- PTY-backed command capture that mirrors output back to your terminal
- Shell integration for zsh, bash, and fish
- Explicit capture mode for any command
- SQLite-backed local runtime store
- Secret redaction for common tokens, passwords, credentials, database URLs, cookies, and bearer tokens
- Error and warning extraction
- Multi-line error block grouping for stack traces and tracebacks
- Runtime timeline through timestamped events
- Search over active runtime logs
- Checkpoint creation and diff summaries
- Simple cross-source correlation hints
- Agent-friendly runtime summaries
- Stdio MCP server for coding agents
- JSON output for CLI automation

## Runtime Model

RunAware is designed to keep agents focused on the current process state.

- Queries return data only from currently active runs.
- When a source starts a new run, previous runtime data for that source is deleted.
- Stopped, stale, and unknown sources can still appear in `runaware sources`, but their logs/errors are not returned by `logs`, `errors`, `summary`, `search`, or MCP query tools.

Source status values:

- `active`: latest captured run has a PID and the PID is alive
- `stopped`: latest captured run exited and wrote an exit code
- `stale`: latest captured run did not write an exit timestamp, but its PID is dead
- `unknown`: older captured run does not have PID metadata

## Install

### Homebrew

The first Homebrew install path will be a project tap:

```bash
brew install ganeshpatro321/tap/runaware
```

Plain `brew install runaware` is the goal, but it only works after RunAware is accepted into Homebrew core. Until then, Homebrew requires the tap-qualified command above.

### macOS and Linux Install Script

After a GitHub release is published, macOS and Linux users can install the latest binary without installing Rust:

```bash
curl -fsSL https://raw.githubusercontent.com/ganeshpatro321/runaware/main/scripts/install.sh | sh
```

The script installs `runaware` to:

```text
~/.local/bin/runaware
```

Install a specific version:

```bash
RUNAWARE_VERSION=v0.1.0 curl -fsSL https://raw.githubusercontent.com/ganeshpatro321/runaware/main/scripts/install.sh | sh
```

Install somewhere else:

```bash
RUNAWARE_INSTALL_DIR=/usr/local/bin curl -fsSL https://raw.githubusercontent.com/ganeshpatro321/runaware/main/scripts/install.sh | sh
```

### Windows PowerShell Install Script

After a GitHub release is published, Windows users can install without Rust:

```powershell
irm https://raw.githubusercontent.com/ganeshpatro321/runaware/main/scripts/install.ps1 | iex
```

The script installs `runaware.exe` to:

```text
%USERPROFILE%\.runaware\bin\runaware.exe
```

Add that directory to `PATH` if needed.

PowerShell shell-profile integration is not implemented yet. On Windows, use explicit capture for now:

```powershell
runaware capture --source api -- pnpm run dev
```

### Direct Binary Downloads

GitHub Releases publish prebuilt archives:

```text
runaware-aarch64-apple-darwin.tar.gz
runaware-x86_64-apple-darwin.tar.gz
runaware-x86_64-unknown-linux-gnu.tar.gz
runaware-x86_64-pc-windows-msvc.zip
```

Each archive includes the `runaware` binary and README. Release assets also include `.sha256` checksum files.

### Install From Source

Developers can install from source with Cargo:

```bash
git clone git@github.com:ganeshpatro321/runaware.git
cd runaware
cargo install --path .
```

Cargo installs the binary to:

```text
~/.cargo/bin/runaware
```

Check the installation:

```bash
runaware doctor
```

## Requirements For Source Builds

- Rust toolchain with Cargo
- macOS or Linux-like environment
- A shell supported by the integration: zsh, bash, or fish

Install Rust from:

```text
https://rustup.rs
```

Check Rust:

```bash
rustc --version
cargo --version
```

## Development Build

```bash
cargo build
cargo test
cargo run -- doctor
```

## Capture Modes

RunAware captures commands in two ways.

### 1. Shell Integration

This lets you keep typing normal commands such as:

```bash
pnpm run dev
npm run dev
pytest
docker compose up
```

Load the integration for the current zsh session:

```bash
eval "$(runaware shell init zsh)"
```

Load it for bash:

```bash
eval "$(runaware shell init bash)"
```

Load it for fish:

```fish
runaware shell init fish | source
```

Supported shell-wrapped commands currently include:

```text
npm, pnpm, yarn, bun, node, python, python3, pytest, go, cargo, docker compose, docker logs
```

### Persistent zsh Setup

To load RunAware automatically in new zsh terminals, add this to `~/.zshrc`:

```bash
# RunAware runtime capture
if command -v runaware >/dev/null 2>&1; then
  eval "$(runaware shell init zsh)"
fi
```

Then restart the terminal or run:

```bash
source ~/.zshrc
```

Important: some terminal launchers or IDEs start zsh with `--no_rcs`. Those shells do not load `~/.zshrc`, so RunAware shell functions will not be installed. In that terminal, verify with:

```bash
type pnpm
```

If it does not say `pnpm is a shell function`, load RunAware manually:

```bash
eval "$(runaware shell init zsh)"
```

or use explicit capture.

### 2. Explicit Capture

Use explicit capture when shell integration is not loaded, when running from an IDE, or when you want to force a source name:

```bash
runaware capture --source api -- pnpm run dev
runaware capture --source frontend -- npm run dev
runaware capture --source tests -- cargo test
```

RunAware mirrors the command output to your terminal while storing redacted runtime context locally.

## Source Naming

For package-manager commands such as `pnpm`, `npm`, `yarn`, and `bun`, RunAware infers the source from:

```text
package.json name
-> current directory name
```

Example:

```bash
cd apps/db-api-server
pnpm run dev
```

May become:

```text
source = db-api-server
last_command = pnpm run dev
last_cwd = /path/to/apps/db-api-server
```

Override source naming with:

```bash
RUNAWARE_SOURCE=api pnpm run dev
```

or:

```bash
runaware capture --source api -- pnpm run dev
```

## CLI Usage

List sources:

```bash
runaware sources
runaware sources --active
runaware sources --stopped
runaware sources --json
```

Inspect active runtime logs:

```bash
runaware logs --since 10m
runaware logs --source api --since 10m
```

Inspect active errors:

```bash
runaware errors --since 10m
runaware errors --source api --since 10m
```

Summarize active runtime state:

```bash
runaware summary --since 10m
runaware summary --source api --since 10m
runaware summary --json
```

Search active runtime logs:

```bash
runaware search ECONNREFUSED --since 30m
runaware search "500" --source frontend --since 30m
```

Create checkpoints:

```bash
runaware checkpoint "before refactor"
runaware diff "before refactor"
```

Show logs around an active error:

```bash
runaware errors --json
runaware context <error-id> --seconds 10
```

Clear data:

```bash
runaware remove-source token-service
runaware clear --source token-service
runaware clear
runaware clear --checkpoints
```

## MCP Integration

RunAware exposes a stdio MCP server:

```bash
runaware mcp
```

You usually do not run this manually. Your coding agent starts it from MCP configuration.

### Codex

Add RunAware to Codex:

```bash
codex mcp add runaware /Users/you/.cargo/bin/runaware mcp
```

Or edit `~/.codex/config.toml`:

```toml
[mcp_servers.runaware]
command = "/Users/you/.cargo/bin/runaware"
args = ["mcp"]
```

Verify:

```bash
codex mcp list
```

Restart Codex or start a new Codex session after adding the server.

Example prompts:

```text
Use RunAware to list active runtime sources.
Use RunAware to summarize active runtime errors.
Check RunAware for errors in db-api-server.
```

### MCP Tools

RunAware exposes these MCP tools:

- `runaware_list_sources`
- `runaware_latest_errors`
- `runaware_summarize_runtime`
- `runaware_search_logs`
- `runaware_logs_around`
- `runaware_create_checkpoint`
- `runaware_diff_since_checkpoint`

The MCP query tools return active-run data only. Stopped, stale, and unknown runs are not exposed as runtime evidence.

### Manual MCP Smoke Test

```bash
printf '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}\n' | runaware mcp
```

## Local Data

Default data directory:

```text
~/.runaware
```

Default database:

```text
~/.runaware/runaware.db
```

Use a separate data directory for testing:

```bash
RUNAWARE_HOME=/tmp/runaware-test runaware summary
```

## Safety

RunAware stores redacted log messages and redacted command metadata. It does not expose raw environment dumps.

The terminal still receives the original command output because RunAware mirrors local process output back to your terminal.

Current redaction covers common forms of:

- bearer tokens
- API keys
- passwords
- cookies
- database URLs
- OpenAI-style keys
- GitHub personal access tokens
- AWS access key IDs
- secret query parameters

Treat this as a safety layer, not a perfect DLP system.

## Contributing

Contributions are welcome. Start with:

```bash
cargo fmt --check
cargo test --locked
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup, project principles, and good first contribution areas.

## Security

Please do not report security vulnerabilities in public issues.

See [SECURITY.md](SECURITY.md) for the security policy and private reporting instructions.

## Code of Conduct

This project has a [Code of Conduct](CODE_OF_CONDUCT.md). Participation in project spaces means agreeing to follow it.

## License

RunAware is licensed under the [MIT License](LICENSE).

## Troubleshooting

### My service is running, but RunAware does not show it as active

The process probably did not start through RunAware.

Check whether shell integration is loaded:

```bash
type pnpm
type npm
```

Expected:

```text
pnpm is a shell function
```

If not:

```bash
eval "$(runaware shell init zsh)"
```

Then stop and restart the service.

### My terminal uses zsh but still does not capture commands

Check whether the shell was started with `--no_rcs`:

```bash
ps -o pid,ppid,command -p $$
```

If you see `zsh --no_rcs`, your `~/.zshrc` was not loaded. Use:

```bash
eval "$(runaware shell init zsh)"
```

or:

```bash
runaware capture --source my-service -- pnpm run dev
```

### I restarted a service and old logs disappeared

That is expected. RunAware deletes previous runtime data for a source when a new run starts. This keeps AI agents from confusing old failures with the current run.

### I stopped a service and logs disappeared from queries

That is expected. Logs/errors/summaries/search results are returned only for active runs.

### MCP shows in `codex mcp list` but not in a visible app UI

That can be fine. Start a new Codex session and ask the agent to use RunAware. MCP servers are typically loaded at session startup.

### I want to inspect the database

```bash
sqlite3 ~/.runaware/runaware.db
```

## Roadmap

- PID/process attach mode
- Direct Docker log adapter
- Log file tailing for IDE-run services
- Browser console capture
- IDE diagnostics adapters
- Better structured JSON-log parsing
- Stronger cross-service correlation
- More robust MCP protocol behavior
- Optional UI/dashboard

## Maintainer Release Process

Run tests:

```bash
cargo test --locked
```

Create and push a version tag:

```bash
git tag v0.1.0
git push origin v0.1.0
```

The release workflow builds:

```text
macOS Apple Silicon
macOS Intel
Linux x86_64
Windows x86_64
```

and uploads archives plus `.sha256` checksum files to GitHub Releases.

For Homebrew, publish the formula from:

```text
packaging/homebrew/Formula/runaware.rb
```

to a tap repository such as:

```text
github.com/ganeshpatro321/homebrew-tap
```

Replace the formula's checksum placeholders with the release checksum values, then users can install with:

```bash
brew install ganeshpatro321/tap/runaware
```
