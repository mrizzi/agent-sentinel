use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

#[test]
fn test_session_start_creates_session_dir() {
    let security_dir = TempDir::new().unwrap();
    let env_file = tempfile::NamedTempFile::new().unwrap();

    let input = serde_json::json!({
        "session_id": "test123abc",
        "hook_event_name": "SessionStart",
        "tool_name": "",
        "cwd": "/tmp"
    });

    Command::cargo_bin("agent-sentinel")
        .unwrap()
        .args(["hook", "session-start", "--security-dir", security_dir.path().to_str().unwrap()])
        .env("SDLC_TARGET_ISSUE", "TC-42")
        .env("CLAUDE_ENV_FILE", env_file.path())
        .write_stdin(serde_json::to_string(&input).unwrap())
        .assert()
        .success();

    // Check CLAUDE_ENV_FILE was written
    let env_content = fs::read_to_string(env_file.path()).unwrap();
    assert!(env_content.contains("SDLC_SESSION_DIR="));

    // Extract session dir and verify contents
    let session_dir = env_content
        .lines()
        .find(|l| l.starts_with("SDLC_SESSION_DIR="))
        .unwrap()
        .strip_prefix("SDLC_SESSION_DIR=")
        .unwrap();

    assert!(std::path::Path::new(session_dir).join("scope.json").exists());
    assert!(std::path::Path::new(session_dir).join("session-meta.json").exists());

    let scope: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(std::path::Path::new(session_dir).join("scope.json")).unwrap()
    ).unwrap();
    assert_eq!(scope["issue_key"], "TC-42");
}

#[test]
fn test_session_start_fails_without_target_issue() {
    let security_dir = TempDir::new().unwrap();
    let env_file = tempfile::NamedTempFile::new().unwrap();

    let input = serde_json::json!({
        "session_id": "test123",
        "hook_event_name": "SessionStart",
        "tool_name": "",
        "cwd": "/tmp"
    });

    Command::cargo_bin("agent-sentinel")
        .unwrap()
        .args(["hook", "session-start", "--security-dir", security_dir.path().to_str().unwrap()])
        .env_remove("SDLC_TARGET_ISSUE")
        .env("CLAUDE_ENV_FILE", env_file.path())
        .write_stdin(serde_json::to_string(&input).unwrap())
        .assert()
        .code(2)
        .stderr(predicate::str::contains("SDLC_TARGET_ISSUE"));
}
