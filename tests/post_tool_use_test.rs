use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

/// Create a mock fortified-llm-client that returns a fixed JSON response
fn create_mock_flc(dir: &std::path::Path) -> String {
    let fixture = fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/flc-success.json")
    ).unwrap();

    let script_path = dir.join("mock-flc");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::write(&script_path, format!("#!/bin/sh\ncat <<'EOF'\n{fixture}\nEOF\n")).unwrap();
        fs::set_permissions(&script_path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    script_path.to_str().unwrap().to_string()
}

/// Create a mock symref that echoes refs JSON
fn create_mock_symref(dir: &std::path::Path) -> String {
    let script_path = dir.join("mock-symref");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let script = r#"#!/bin/sh
# Read stdin (extraction JSON), output refs
cat <<'EOF'
{
  "refs": {
    "$TC42_REQ_1": {"summary": "OAuth2 login flow", "ref": "$TC42_REQ_1"},
    "$TC42_SUMMARY": {"summary": "Add OAuth2 login", "ref": "$TC42_SUMMARY"}
  },
  "store_path": "/tmp/test/vars.json"
}
EOF
"#;
        fs::write(&script_path, script).unwrap();
        fs::set_permissions(&script_path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    script_path.to_str().unwrap().to_string()
}

fn create_test_registry(security_dir: &std::path::Path) {
    let registry = serde_json::json!({
        "post_tool_use": {
            "mcp__atlassian__getJiraIssue": {
                "config": "config/jira-task.toml",
                "prefix_from": "issueIdOrKey"
            }
        },
        "pre_tool_use": {}
    });
    fs::write(
        security_dir.join("tool-registry.json"),
        serde_json::to_string_pretty(&registry).unwrap(),
    ).unwrap();

    // Create mock config file (just needs to exist)
    fs::create_dir_all(security_dir.join("config")).unwrap();
    fs::write(security_dir.join("config/jira-task.toml"), "# mock").unwrap();
}

#[test]
fn test_post_tool_use_full_flow() {
    let tmp = TempDir::new().unwrap();
    let security_dir = TempDir::new().unwrap();
    let session_dir = TempDir::new().unwrap();

    create_test_registry(security_dir.path());

    let mock_flc = create_mock_flc(tmp.path());
    let mock_symref = create_mock_symref(tmp.path());

    let jira_response = fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/jira-task-response.json")
    ).unwrap();

    let input = serde_json::json!({
        "session_id": "test123",
        "hook_event_name": "PostToolUse",
        "tool_name": "mcp__atlassian__getJiraIssue",
        "tool_input": {"issueIdOrKey": "TC-42"},
        "tool_response": jira_response
    });

    let output = Command::cargo_bin("agent-sentinel")
        .unwrap()
        .args(["hook", "post-tool-use", "--security-dir", security_dir.path().to_str().unwrap()])
        .env("SDLC_SESSION_DIR", session_dir.path())
        .env("FORTIFIED_LLM_CLIENT_BIN", &mock_flc)
        .env("SYMREF_BIN", &mock_symref)
        .write_stdin(serde_json::to_string(&input).unwrap())
        .output()
        .unwrap();

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let response: serde_json::Value = serde_json::from_str(&stdout).unwrap();

    assert_eq!(response["hookSpecificOutput"]["hookEventName"], "PostToolUse");
    assert!(response["hookSpecificOutput"]["updatedMCPToolOutput"]["refs"].is_object());
}

#[test]
fn test_post_tool_use_passthrough_unknown_tool() {
    let security_dir = TempDir::new().unwrap();

    let registry = serde_json::json!({
        "post_tool_use": {},
        "pre_tool_use": {}
    });
    fs::write(
        security_dir.path().join("tool-registry.json"),
        serde_json::to_string_pretty(&registry).unwrap(),
    ).unwrap();

    let input = serde_json::json!({
        "tool_name": "mcp__github__list_repos",
        "tool_input": {},
        "tool_response": "{}"
    });

    Command::cargo_bin("agent-sentinel")
        .unwrap()
        .args(["hook", "post-tool-use", "--security-dir", security_dir.path().to_str().unwrap()])
        .env("SDLC_SESSION_DIR", "/tmp/test")
        .write_stdin(serde_json::to_string(&input).unwrap())
        .assert()
        .success()
        .stdout(predicate::str::is_empty());
}

#[test]
fn test_post_tool_use_fails_closed_without_session() {
    let security_dir = TempDir::new().unwrap();

    let registry = serde_json::json!({
        "post_tool_use": {
            "mcp__atlassian__getJiraIssue": {
                "config": "config/jira-task.toml",
                "prefix_from": "issueIdOrKey"
            }
        },
        "pre_tool_use": {}
    });
    fs::write(
        security_dir.path().join("tool-registry.json"),
        serde_json::to_string_pretty(&registry).unwrap(),
    ).unwrap();

    let input = serde_json::json!({
        "tool_name": "mcp__atlassian__getJiraIssue",
        "tool_input": {"issueIdOrKey": "TC-42"},
        "tool_response": "{}"
    });

    Command::cargo_bin("agent-sentinel")
        .unwrap()
        .args(["hook", "post-tool-use", "--security-dir", security_dir.path().to_str().unwrap()])
        .env_remove("SDLC_SESSION_DIR")
        .write_stdin(serde_json::to_string(&input).unwrap())
        .assert()
        .code(2);
}
