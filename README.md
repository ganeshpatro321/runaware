# RunAware

RunAware is a local runtime awareness layer for AI coding agents.

It captures named local process output, redacts obvious secrets, extracts errors and warnings, groups stack traces, tracks checkpoints, summarizes recent runtime state, and exposes the result to agents through MCP and JSON-friendly CLI commands.

## Features

- Runtime source tracking for named services such as `frontend`, `api`, `worker`, and `tests`
- Latest working directory tracking per source, so agents can identify which project/service produced a signal
- PTY-backed command capture that mirrors output back to the terminal
- Shell integration for zsh, bash, and fish so developers can run normal commands
- Explicit capture fallback with `runaware capture --source <name> -- <command>`
- SQLite-backed local runtime history
- Secret redaction for common tokens, passwords, credentials, database URLs, cookies, and bearer tokens
- Error and warning extraction from noisy logs
- Multi-line error block grouping for stack traces and tracebacks
- Runtime timeline through timestamped events
- Searchable recent logs with SQLite full-text search
- Checkpoint creation and diff summaries
- Simple cross-source correlation hints for related failures
- Agent-friendly runtime summaries
- Stdio MCP server for coding agents
- JSON output for CLI automation

## Current Limitations

- RunAware captures commands it starts or commands routed through shell integration.
- It cannot recover past stdout/stderr from a process that was already started outside RunAware.
- PID attach, direct Docker log discovery, WebStorm/IDE log adapters, browser console capture, and a UI dashboard are not implemented yet.
- Cross-source correlation is currently heuristic and time-window based.

## Install

```bash
cargo install --path .
```

This installs the `runaware` binary into Cargo's bin directory, usually:

```text
~/.cargo/bin/runaware
```

Make sure Cargo's bin directory is on your `PATH`:

```bash
export PATH="$HOME/.cargo/bin:$PATH"
```

Check the installation:

```bash
runaware doctor
```

## Development Build

```bash
cargo build
```

For local development:

```bash
cargo run -- doctor
```

## Shell Integration

RunAware's shell integration lets you keep typing normal commands like `npm run dev`, while the shell routes supported commands through RunAware capture.

Load it for the current zsh session:

```bash
eval "$(runaware shell init zsh)"
```

Load it for the current bash session:

```bash
eval "$(runaware shell init bash)"
```

Load it for the current fish session:

```fish
runaware shell init fish | source
```

### Persistent zsh Setup

To make RunAware load automatically in every new zsh terminal, add this block to `~/.zshrc`:

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

After that, supported commands are captured automatically in new zsh sessions.

## Capture Commands Explicitly

```bash
runaware capture --source api -- npm run dev
runaware capture --source frontend -- pnpm dev
runaware capture --source tests -- cargo test
```

RunAware mirrors command output to the terminal while storing redacted runtime context locally.

Then run common commands normally:

```bash
npm run dev
pnpm dev
pytest
docker compose up
```

Override source naming when inference is not enough:

```bash
RUNAWARE_SOURCE=api npm run dev
```

Supported shell-wrapped commands currently include:

```text
npm, pnpm, yarn, bun, node, python, python3, pytest, go, cargo, docker compose, docker logs
```

## Query Runtime State

```bash
runaware sources
runaware sources --active
runaware sources --stopped
runaware logs --since 10m
runaware errors --since 10m
runaware summary --since 10m
runaware search ECONNREFUSED --since 30m
```

Runtime query commands only return logs, errors, search results, and summaries for currently active runs. When a source starts a new run, previous runtime logs for that source are deleted so agents do not confuse old failures with the current process. Stopped, stale, and unknown sources remain visible in `runaware sources`, but their logs and errors are not returned by query commands.

Source status values:

- `active`: latest captured run has a PID and that PID is still alive
- `stopped`: latest captured run exited cleanly and wrote an exit code
- `stale`: latest captured run did not write an exit timestamp, but its PID is no longer alive
- `unknown`: older captured run does not have PID metadata

## Clearing Data

Remove one stale source and its captured runs/logs/errors:

```bash
runaware remove-source token-service
```

Equivalent:

```bash
runaware clear --source token-service
```

Clear all runtime data while keeping checkpoints:

```bash
runaware clear
```

Clear all runtime data and checkpoints:

```bash
runaware clear --checkpoints
```

## Checkpoints

```bash
runaware checkpoint "before refactor"
runaware diff "before refactor"
```

## Error Context

```bash
runaware errors --json
runaware context <error-id> --seconds 10
```

## MCP

RunAware exposes a stdio MCP server:

```bash
runaware mcp
```

Agent-facing tools:

- `runaware_list_sources`
- `runaware_latest_errors`
- `runaware_summarize_runtime`
- `runaware_search_logs`
- `runaware_logs_around`
- `runaware_create_checkpoint`
- `runaware_diff_since_checkpoint`

Example Codex-style MCP command configuration:

```toml
[mcp_servers.runaware]
command = "runaware"
args = ["mcp"]
```

RunAware's MCP surface is intentionally read-mostly. The initial write operation is checkpoint creation.

## Local Data

By default, RunAware stores data at:

```text
~/.runaware/runaware.db
```

For isolated testing:

```bash
RUNAWARE_HOME=/tmp/runaware-test runaware summary
```

## Safety

RunAware stores redacted log messages and redacted command metadata. It does not expose raw environment dumps. The terminal still receives the original command output because RunAware mirrors the user's normal process output back to their local terminal.

## Typical Workflow

```bash
# one-time install
cargo install --path .

# one-time zsh setup: add the persistent block above to ~/.zshrc

# start a new terminal
RUNAWARE_SOURCE=api npm run dev

# start another terminal
RUNAWARE_SOURCE=frontend pnpm dev

# inspect runtime state
runaware summary --since 10m
runaware errors --since 10m
```
