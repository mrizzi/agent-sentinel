use assert_cmd::Command;
use std::fs;
use tempfile::TempDir;

#[test]
fn test_session_end_copies_transcript() {
    let security_dir = TempDir::new().unwrap();
    let session_dir = TempDir::new().unwrap();
    let transcript = tempfile::NamedTempFile::new().unwrap();

    fs::write(transcript.path(), "test transcript content").unwrap();

    let input = serde_json::json!({
        "tool_name": "",
        "session_id": "test123",
        "transcript_path": transcript.path().to_str().unwrap()
    });

    Command::cargo_bin("agent-sentinel")
        .unwrap()
        .args([
            "hook",
            "session-end",
            "--security-dir",
            security_dir.path().to_str().unwrap(),
        ])
        .env("AGENT_SENTINEL_SESSION_DIR", session_dir.path())
        .write_stdin(serde_json::to_string(&input).unwrap())
        .assert()
        .success();

    let copied = fs::read_to_string(session_dir.path().join("transcript.jsonl")).unwrap();
    assert_eq!(copied, "test transcript content");
}

#[test]
fn test_session_end_graceful_without_session_dir() {
    let security_dir = TempDir::new().unwrap();

    let input = serde_json::json!({
        "tool_name": "",
        "session_id": "test123",
        "transcript_path": "/nonexistent"
    });

    Command::cargo_bin("agent-sentinel")
        .unwrap()
        .args([
            "hook",
            "session-end",
            "--security-dir",
            security_dir.path().to_str().unwrap(),
        ])
        .env_remove("AGENT_SENTINEL_SESSION_DIR")
        .write_stdin(serde_json::to_string(&input).unwrap())
        .assert()
        .success();
}
