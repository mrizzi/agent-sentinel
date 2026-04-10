use crate::claude::HookInput;
use anyhow::Context;
use std::fs;
use std::path::Path;

pub fn run(_security_dir: &Path) -> anyhow::Result<()> {
    let input = HookInput::from_stdin()?;

    let target_issue = std::env::var("SDLC_TARGET_ISSUE")
        .context("SDLC_TARGET_ISSUE environment variable not set")?;

    let env_file = std::env::var("CLAUDE_ENV_FILE")
        .context("CLAUDE_ENV_FILE not available")?;

    let session_id = input.session_id.unwrap_or_default();
    let cwd = input.cwd.unwrap_or_default();
    let short_id = &session_id[..session_id.len().min(8)];

    let timestamp = chrono_free_timestamp();
    let session_dir = format!("/tmp/sdlc-sessions/{timestamp}-{short_id}");

    fs::create_dir_all(format!("{session_dir}/evaluations"))
        .context("Failed to create evaluations dir")?;
    fs::create_dir_all(format!("{session_dir}/output"))
        .context("Failed to create output dir")?;

    // Write scope.json
    let scope = serde_json::json!({ "issue_key": target_issue });
    fs::write(
        format!("{session_dir}/scope.json"),
        serde_json::to_string_pretty(&scope)?,
    )?;

    // Write session-meta.json
    let meta = serde_json::json!({
        "session_id": session_id,
        "issue_key": target_issue,
        "started_at": iso_timestamp(),
        "user": whoami(),
        "cwd": cwd,
    });
    fs::write(
        format!("{session_dir}/session-meta.json"),
        serde_json::to_string_pretty(&meta)?,
    )?;

    // Export session dir
    let mut env_content = fs::read_to_string(&env_file).unwrap_or_default();
    env_content.push_str(&format!("SDLC_SESSION_DIR={session_dir}\n"));
    fs::write(&env_file, env_content)?;

    Ok(())
}

fn chrono_free_timestamp() -> String {
    std::process::Command::new("date")
        .args(["+%Y%m%d-%H%M%S"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| "00000000-000000".to_string())
}

fn iso_timestamp() -> String {
    std::process::Command::new("date")
        .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

fn whoami() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_else(|_| "unknown".to_string())
}
