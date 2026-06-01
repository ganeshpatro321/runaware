mod capture;
mod cli;
mod detect;
mod mcp;
mod redact;
mod shell;
mod store;
mod summary;
mod time;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Commands, ShellCommands};
use store::Store;

fn main() -> Result<()> {
    let cli = Cli::parse();
    let store = Store::open_default()?;

    match cli.command {
        Commands::Capture {
            source,
            command,
            no_pty,
        } => {
            let code = capture::capture_command(&store, source, command, !no_pty)?;
            std::process::exit(code);
        }
        Commands::Shell { command } => match command {
            ShellCommands::Init { shell } => {
                println!("{}", shell::init_script(shell));
            }
        },
        Commands::Sources {
            active,
            stopped,
            json,
        } => {
            let mut sources = store.list_sources()?;
            if active {
                sources.retain(|source| source.active);
            }
            if stopped {
                sources.retain(|source| source.status == "stopped");
            }
            if json {
                println!("{}", serde_json::to_string_pretty(&sources)?);
            } else if sources.is_empty() {
                println!("No runtime sources have been captured yet.");
            } else {
                for source in sources {
                    println!(
                        "{}\tstatus={}\tactive_runs={}\tlast_seen={}\truns={}\tcwd={}\tlast_command={}",
                        source.name,
                        source.status,
                        source.active_run_count,
                        source.last_seen_at,
                        source.run_count,
                        source.last_cwd.unwrap_or_else(|| "-".to_string()),
                        source.last_command.unwrap_or_else(|| "-".to_string())
                    );
                }
            }
        }
        Commands::Logs {
            source,
            since,
            limit,
            json,
        } => {
            let since = time::parse_since(&since)?;
            let logs = store.logs_since(since, source.as_deref(), limit, true)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&logs)?);
            } else {
                for event in logs {
                    println!(
                        "{} [{}] {} {}",
                        event.ts, event.source, event.level, event.message
                    );
                }
            }
        }
        Commands::Errors {
            source,
            since,
            limit,
            json,
        } => {
            let since = time::parse_since(&since)?;
            let errors = store.error_blocks_since(since, source.as_deref(), limit, true)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&errors)?);
            } else if errors.is_empty() {
                println!("No errors or warnings found.");
            } else {
                for error in errors {
                    println!(
                        "{} [{}] {} {}",
                        error.start_ts, error.source, error.severity, error.title
                    );
                    println!("{}", indent(&error.body));
                }
            }
        }
        Commands::Summary {
            source,
            since,
            json,
        } => {
            let since = time::parse_since(&since)?;
            let report = summary::summarize(&store, since, source.as_deref(), true)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("{}", summary::render_text(&report));
            }
        }
        Commands::Search {
            query,
            source,
            since,
            limit,
            json,
        } => {
            let since = time::parse_since(&since)?;
            let results = store.search_logs(&query, since, source.as_deref(), limit, true)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&results)?);
            } else {
                for event in results {
                    println!(
                        "{} [{}] {} {}",
                        event.ts, event.source, event.level, event.message
                    );
                }
            }
        }
        Commands::Context {
            error_id,
            seconds,
            limit,
            json,
        } => {
            let events = store.logs_around_error(&error_id, seconds, limit)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&events)?);
            } else {
                for event in events {
                    println!(
                        "{} [{}] {} {}",
                        event.ts, event.source, event.level, event.message
                    );
                }
            }
        }
        Commands::Clear {
            source,
            checkpoints,
        } => {
            if let Some(source) = source {
                let removed = store.remove_source(&source)?;
                println!("Removed source '{source}' and {removed} run(s).");
            } else {
                store.clear_all_runtime_data(checkpoints)?;
                if checkpoints {
                    println!("Cleared all runtime data and checkpoints.");
                } else {
                    println!("Cleared all runtime data. Checkpoints were kept.");
                }
            }
        }
        Commands::RemoveSource { source } => {
            let removed = store.remove_source(&source)?;
            println!("Removed source '{source}' and {removed} run(s).");
        }
        Commands::Checkpoint { name, json } => {
            let checkpoint = store.create_checkpoint(&name)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&checkpoint)?);
            } else {
                println!(
                    "Created checkpoint '{}' at {}",
                    checkpoint.name, checkpoint.ts
                );
            }
        }
        Commands::Diff {
            checkpoint,
            source,
            json,
        } => {
            let checkpoint = store.find_checkpoint(&checkpoint)?;
            let since =
                chrono::DateTime::parse_from_rfc3339(&checkpoint.ts)?.with_timezone(&chrono::Utc);
            let report = summary::summarize(&store, since, source.as_deref(), false)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("Since checkpoint '{}' ({})", checkpoint.name, checkpoint.ts);
                println!("{}", summary::render_text(&report));
            }
        }
        Commands::Mcp => {
            mcp::serve_stdio(store)?;
        }
        Commands::Doctor => {
            println!("RunAware data directory: {}", Store::data_dir()?.display());
            println!("RunAware database: {}", Store::default_path()?.display());
            println!("SQLite schema is ready.");
            println!("Shell integration: run `eval \"$(runaware shell init zsh)\"`");
            println!("MCP server command: runaware mcp");
        }
    }

    Ok(())
}

fn indent(value: &str) -> String {
    value
        .lines()
        .map(|line| format!("  {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}
