use assert_cmd::Command;
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
        .args([
            "hook",
            "session-start",
            "--security-dir",
            security_dir.path().to_str().unwrap(),
        ])
        .env("CLAUDE_ENV_FILE", env_file.path())
        .write_stdin(serde_json::to_string(&input).unwrap())
        .assert()
        .success();

    // Check CLAUDE_ENV_FILE was written
    let env_content = fs::read_to_string(env_file.path()).unwrap();
    assert!(env_content.contains("AGENT_SENTINEL_SESSION_DIR="));

    // Extract session dir and verify contents
    let session_dir = env_content
        .lines()
        .find(|l| l.contains("AGENT_SENTINEL_SESSION_DIR="))
        .unwrap()
        .split('=')
        .nth(1)
        .unwrap()
        .trim_matches('\'');

    assert!(std::path::Path::new(session_dir)
        .join("session-meta.json")
        .exists());
    assert!(std::path::Path::new(session_dir)
        .join("evaluations")
        .is_dir());
    assert!(std::path::Path::new(session_dir)
        .join("output")
        .is_dir());
}
