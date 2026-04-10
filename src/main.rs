use clap::{Parser, Subcommand};
use std::process::ExitCode;

mod claude;

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
                eprintln!("session-start: not yet implemented (security_dir={security_dir})");
                Ok(())
            }
            HookCommands::PostToolUse { security_dir } => {
                eprintln!("post-tool-use: not yet implemented (security_dir={security_dir})");
                Ok(())
            }
            HookCommands::PreToolUse { security_dir } => {
                eprintln!("pre-tool-use: not yet implemented (security_dir={security_dir})");
                Ok(())
            }
            HookCommands::SessionEnd { security_dir } => {
                eprintln!("session-end: not yet implemented (security_dir={security_dir})");
                Ok(())
            }
        },
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err::<_, anyhow::Error>(_) => ExitCode::from(2),
    }
}
