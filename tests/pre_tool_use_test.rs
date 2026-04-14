use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

#[test]
fn test_pre_tool_use_deref() {
    let security_dir = TempDir::new().unwrap();
    let session_dir = TempDir::new().unwrap();

    // Create tool registry
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

    // Create vars.json with test variables (symref is now a library — no mock needed)
    let vars = serde_json::json!({
        "$TC42_REQ_1": "OAuth2 login flow"
    });
    fs::write(
        session_dir.path().join("vars.json"),
        serde_json::to_string_pretty(&vars).unwrap(),
    )
    .unwrap();

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
    let updated = &response["hookSpecificOutput"]["updatedInput"];
    assert_eq!(updated["issueKey"], "TC-42");
    assert_eq!(updated["description"], "Implementing OAuth2 login flow");
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

#[test]
fn test_pre_tool_use_deref_failure_passthrough() {
    let security_dir = TempDir::new().unwrap();
    let session_dir = TempDir::new().unwrap();

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

    // Write malformed JSON to vars.json — symref::deref() will fail to parse it
    fs::write(session_dir.path().join("vars.json"), "NOT VALID JSON").unwrap();

    let input = serde_json::json!({
        "tool_name": "mcp__atlassian__editJiraIssue",
        "tool_input": {
            "issueKey": "TC-42",
            "description": "Implementing $TC42_REQ_1"
        }
    });

    // Should passthrough: exit 0, no stdout output
    Command::cargo_bin("agent-sentinel")
        .unwrap()
        .args([
            "hook",
            "pre-tool-use",
            "--security-dir",
            security_dir.path().to_str().unwrap(),
        ])
        .env("AGENT_SENTINEL_SESSION_DIR", session_dir.path())
        .write_stdin(serde_json::to_string(&input).unwrap())
        .assert()
        .success()
        .stdout(predicate::str::is_empty());
}
