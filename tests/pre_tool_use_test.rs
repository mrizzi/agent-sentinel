use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

fn create_mock_symref_deref(dir: &std::path::Path) -> String {
    let script_path = dir.join("mock-symref");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let script = r#"#!/bin/sh
# Read stdin, do simple substitution for testing
sed 's/\$TC42_REQ_1/OAuth2 login flow/g'
"#;
        fs::write(&script_path, script).unwrap();
        fs::set_permissions(&script_path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    script_path.to_str().unwrap().to_string()
}

#[test]
fn test_pre_tool_use_deref() {
    let tmp = TempDir::new().unwrap();
    let security_dir = TempDir::new().unwrap();
    let session_dir = TempDir::new().unwrap();

    // Create vars.json so session dir looks valid
    fs::write(session_dir.path().join("vars.json"), "{}").unwrap();

    let registry = serde_json::json!({
        "post_tool_use": {},
        "pre_tool_use": {
            "mcp__atlassian__editJiraIssue": {}
        }
    });
    fs::write(
        security_dir.path().join("tool-registry.json"),
        serde_json::to_string_pretty(&registry).unwrap(),
    )
    .unwrap();

    let mock_symref = create_mock_symref_deref(tmp.path());

    let input = serde_json::json!({
        "tool_name": "mcp__atlassian__editJiraIssue",
        "tool_input": {
            "issueKey": "TC-42",
            "description": "Implementing $TC42_REQ_1"
        }
    });

    let output = Command::cargo_bin("agent-sentinel")
        .unwrap()
        .args([
            "hook",
            "pre-tool-use",
            "--security-dir",
            security_dir.path().to_str().unwrap(),
        ])
        .env("AGENT_SENTINEL_SESSION_DIR", session_dir.path())
        .env("SYMREF_BIN", &mock_symref)
        .write_stdin(serde_json::to_string(&input).unwrap())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let response: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(
        response["hookSpecificOutput"]["hookEventName"],
        "PreToolUse"
    );
    assert!(response["hookSpecificOutput"]["updatedInput"].is_object());
}

#[test]
fn test_pre_tool_use_passthrough_unknown_tool() {
    let security_dir = TempDir::new().unwrap();

    let registry = serde_json::json!({
        "post_tool_use": {},
        "pre_tool_use": {}
    });
    fs::write(
        security_dir.path().join("tool-registry.json"),
        serde_json::to_string_pretty(&registry).unwrap(),
    )
    .unwrap();

    let input = serde_json::json!({
        "tool_name": "mcp__github__create_pr",
        "tool_input": {}
    });

    Command::cargo_bin("agent-sentinel")
        .unwrap()
        .args([
            "hook",
            "pre-tool-use",
            "--security-dir",
            security_dir.path().to_str().unwrap(),
        ])
        .write_stdin(serde_json::to_string(&input).unwrap())
        .assert()
        .success()
        .stdout(predicate::str::is_empty());
}
