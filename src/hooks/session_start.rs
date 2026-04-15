use crate::claude::{sessions_base_dir, HookInput};
use anyhow::Context;
use chrono::Utc;
use std::fs;
use std::path::Path;

pub fn run(_security_dir: &Path) -> anyhow::Result<()> {
    let input = HookInput::from_stdin()?;

    let env_file = std::env::var("CLAUDE_ENV_FILE").context("CLAUDE_ENV_FILE not available")?;

    let session_id = input.session_id.unwrap_or_default();
    let cwd = input.cwd.unwrap_or_default();
    let short_id = &session_id[..session_id.len().min(8)];

    let base_dir = sessions_base_dir();
    let timestamp = timestamp_for_dir();
    let session_dir = format!("{base_dir}/{timestamp}-{short_id}");

    fs::create_dir_all(format!("{session_dir}/evaluations"))
        .context("Failed to create evaluations dir")?;
    fs::create_dir_all(format!("{session_dir}/output")).context("Failed to create output dir")?;

    // Write session-meta.json
    let meta = serde_json::json!({
        "session_id": session_id,
        "started_at": iso_timestamp(),
        "user": whoami(),
        "cwd": cwd,
    });
    fs::write(
        format!("{session_dir}/session-meta.json"),
        serde_json::to_string_pretty(&meta)?,
    )?;

    // Export session dir via CLAUDE_ENV_FILE (available to Bash commands)
    let mut env_content = fs::read_to_string(&env_file).unwrap_or_default();
    env_content.push_str(&format!(
        "export AGENT_SENTINEL_SESSION_DIR='{session_dir}'\n"
    ));
    fs::write(&env_file, env_content)?;

    // Also write to a well-known file so other hooks (PostToolUse, PreToolUse,
    // SessionEnd) can discover the session dir. These hooks run as separate
    // processes and don't inherit CLAUDE_ENV_FILE variables.
    let sentinel_state = format!("{base_dir}/current");
    fs::write(&sentinel_state, &session_dir)
        .with_context(|| format!("Failed to write session state file: {sentinel_state}"))?;

    Ok(())
}

fn timestamp_for_dir() -> String {
    Utc::now().format("%Y%m%d-%H%M%S").to_string()
}

fn iso_timestamp() -> String {
    Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn whoami() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_else(|_| "unknown".to_string())
}
