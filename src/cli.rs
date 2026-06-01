use clap::{Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(name = "runaware")]
#[command(about = "Local runtime awareness for AI coding agents")]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Capture a command's runtime output while mirroring it to the terminal.
    Capture {
        /// Runtime source name, such as frontend, api, worker, tests, or auto.
        #[arg(short, long, default_value = "auto")]
        source: String,

        /// Use plain pipes instead of a pseudo-terminal.
        #[arg(long)]
        no_pty: bool,

        /// Command and arguments to run. Use `--` before the command.
        #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<String>,
    },

    /// Print shell integration scripts.
    Shell {
        #[command(subcommand)]
        command: ShellCommands,
    },

    /// List known runtime sources.
    Sources {
        /// Show only sources whose latest captured run is still active.
        #[arg(long)]
        active: bool,

        /// Show only sources whose latest captured run has stopped.
        #[arg(long)]
        stopped: bool,

        /// Emit JSON.
        #[arg(long)]
        json: bool,
    },

    /// Show recent log events.
    Logs {
        #[arg(short, long)]
        source: Option<String>,

        #[arg(long, default_value = "10m")]
        since: String,

        #[arg(short, long, default_value_t = 100)]
        limit: usize,

        #[arg(long)]
        json: bool,
    },

    /// Show extracted errors and warnings.
    Errors {
        #[arg(short, long)]
        source: Option<String>,

        #[arg(long, default_value = "10m")]
        since: String,

        #[arg(short, long, default_value_t = 20)]
        limit: usize,

        #[arg(long)]
        json: bool,
    },

    /// Summarize current runtime state for an agent or developer.
    Summary {
        #[arg(short, long)]
        source: Option<String>,

        #[arg(long, default_value = "10m")]
        since: String,

        #[arg(long)]
        json: bool,
    },

    /// Search recent runtime history.
    Search {
        query: String,

        #[arg(short, long)]
        source: Option<String>,

        #[arg(long, default_value = "30m")]
        since: String,

        #[arg(short, long, default_value_t = 50)]
        limit: usize,

        #[arg(long)]
        json: bool,
    },

    /// Show logs around a specific extracted error id.
    Context {
        error_id: String,

        #[arg(long, default_value_t = 10)]
        seconds: i64,

        #[arg(short, long, default_value_t = 100)]
        limit: usize,

        #[arg(long)]
        json: bool,
    },

    /// Clear captured runtime data.
    Clear {
        /// Clear only one source. Without this, all runtime data is cleared.
        #[arg(short, long)]
        source: Option<String>,

        /// Also remove checkpoints when clearing all runtime data.
        #[arg(long)]
        checkpoints: bool,
    },

    /// Remove one runtime source and all of its captured data.
    RemoveSource { source: String },

    /// Create a named debugging checkpoint.
    Checkpoint {
        name: String,

        #[arg(long)]
        json: bool,
    },

    /// Summarize errors and logs since a checkpoint id or name.
    Diff {
        checkpoint: String,

        #[arg(short, long)]
        source: Option<String>,

        #[arg(long)]
        json: bool,
    },

    /// Serve RunAware tools over MCP stdio.
    Mcp,

    /// Print local configuration and setup status.
    Doctor,
}

#[derive(Debug, Subcommand)]
pub enum ShellCommands {
    /// Print shell integration for the selected shell.
    Init { shell: ShellKind },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ShellKind {
    Zsh,
    Bash,
    Fish,
}
