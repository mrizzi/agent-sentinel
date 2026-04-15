use crate::claude::{sessions_base_dir, HookInput};
use anyhow::Context;
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn run(_security_dir: &Path) -> anyhow::Result<()> {
    let input = HookInput::from_stdin()?;

    let env_file = std::env::var("CLAUDE_ENV_FILE").context("CLAUDE_ENV_FILE not available")?;

    let session_id = input.session_id.unwrap_or_default();
    let cwd = input.cwd.unwrap_or_default();
    let short_id = &session_id[..session_id.len().min(8)];

    let base_dir = sessions_base_dir();
    let timestamp = chrono_free_timestamp();
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

fn chrono_free_timestamp() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| {
            let total_seconds = d.as_secs();
            let days = total_seconds / 86400;
            let seconds_today = total_seconds % 86400;
            let hours = seconds_today / 3600;
            let minutes = (seconds_today % 3600) / 60;
            let seconds = seconds_today % 60;

            let (year, month, day) = days_to_ymd(days);
            format!(
                "{:04}{:02}{:02}-{:02}{:02}{:02}",
                year, month, day, hours, minutes, seconds
            )
        })
        .unwrap_or_else(|| "00000000-000000".to_string())
}

fn iso_timestamp() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| {
            let total_seconds = d.as_secs();
            let days = total_seconds / 86400;
            let seconds_today = total_seconds % 86400;
            let hours = seconds_today / 3600;
            let minutes = (seconds_today % 3600) / 60;
            let seconds = seconds_today % 60;

            let (year, month, day) = days_to_ymd(days);
            format!(
                "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
                year, month, day, hours, minutes, seconds
            )
        })
        .unwrap_or_else(|| "1970-01-01T00:00:00Z".to_string())
}

/// Convert days since Unix epoch to (year, month, day).
/// Correct for dates from 1970 to 2099.
fn days_to_ymd(days_since_epoch: u64) -> (u32, u32, u32) {
    // Days in each month for non-leap and leap years
    const DAYS_IN_MONTH: [[u32; 12]; 2] = [
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31], // non-leap
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31], // leap
    ];

    let mut days = days_since_epoch as u32;
    let mut year = 1970;

    // Iterate through years
    loop {
        let is_leap = (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0);
        let days_in_year = if is_leap { 366 } else { 365 };

        if days >= days_in_year {
            days -= days_in_year;
            year += 1;
        } else {
            break;
        }
    }

    // Find month and day
    let is_leap = (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0);
    let month_days = &DAYS_IN_MONTH[if is_leap { 1 } else { 0 }];

    let mut month = 1;
    for &days_in_month in month_days {
        if days >= days_in_month {
            days -= days_in_month;
            month += 1;
        } else {
            break;
        }
    }

    let day = days + 1; // days is 0-indexed, calendar days are 1-indexed

    (year, month, day)
}

fn whoami() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_else(|_| "unknown".to_string())
}
