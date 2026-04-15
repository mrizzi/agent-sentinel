use clap::{Parser, Subcommand};
use std::process::ExitCode;

mod claude;
mod hooks;
mod registry;

#[derive(Parser)]
#[command(name = "agent-sentinel")]
#[command(about = "Security hook engine for the Dual LLM pattern")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Hook subcommands for Claude Code integration
    Hook {
        #[command(subcommand)]
        hook: HookCommands,
    },
}

#[derive(Subcommand)]
enum HookCommands {
    /// SessionStart: create session directory and export env
    SessionStart {
        /// Path to the security configuration directory
        #[arg(long)]
        security_dir: String,
    },
    /// PostToolUse: Dual LLM quarantine flow
    PostToolUse {
        /// Path to the security configuration directory
        #[arg(long)]
        security_dir: String,
    },
    /// PreToolUse: symbolic dereferencing
    PreToolUse {
        /// Path to the security configuration directory
        #[arg(long)]
        security_dir: String,
    },
    /// SessionEnd: collect transcript
    SessionEnd {
        /// Path to the security configuration directory
        #[arg(long)]
        security_dir: String,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Hook { hook } => match hook {
            HookCommands::SessionStart { security_dir } => {
                hooks::session_start::run(std::path::Path::new(&security_dir))
            }
            HookCommands::PostToolUse { security_dir } => {
                hooks::post_tool_use::run(std::path::Path::new(&security_dir))
            }
            HookCommands::PreToolUse { security_dir } => {
                hooks::pre_tool_use::run(std::path::Path::new(&security_dir))
            }
            HookCommands::SessionEnd { security_dir } => {
                hooks::session_end::run(std::path::Path::new(&security_dir))
            }
        },
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("FATAL: {e:#}");
            ExitCode::from(2)
        }
    }
}
