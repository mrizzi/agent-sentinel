use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

/// The extraction JSON that FLC returns, embedded in the mock OpenAI chat completion
/// response's `content` field.
const EXTRACTION_JSON: &str = r#"{"summary":"Add OAuth2 login","requirements":[{"id":"REQ_1","text":"OAuth2 login flow","priority":"high"}],"acceptance_criteria":[{"id":"AC_1","description":"Users can authenticate via OAuth2"}],"status":"To Do"}"#;

/// Build a canned OpenAI-compatible chat completion response body.
fn openai_chat_completion_body() -> String {
    serde_json::json!({
        "id": "test-response",
        "object": "chat.completion",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": EXTRACTION_JSON
            },
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 100,
            "completion_tokens": 50,
            "total_tokens": 150
        }
    })
    .to_string()
}

/// Create a valid FLC config TOML file that points to the given mock server URL.
///
/// The `api_url` includes the `/v1/chat/completions` path because:
///   - The OpenAI provider posts directly to `api_url` as-is
///   - Provider auto-detection keys off `/v1/chat/completions` in the URL
fn create_test_config(config_dir: &Path, mock_server_url: &str) -> std::path::PathBuf {
    let config = format!(
        r#"api_url = "{url}/v1/chat/completions"
model = "test-model"
system_prompt = "Extract structured data. Return valid JSON."
temperature = 0.0
max_tokens = 2000
timeout_secs = 10
response_format = "json-object"
api_key = "test-key"
"#,
        url = mock_server_url
    );
    let path = config_dir.join("jira-task.toml");
    fs::write(&path, config).unwrap();
    path
}

fn create_test_registry(security_dir: &Path, config_filename: &str) {
    let registry = serde_json::json!({
        "post_tool_use": {
            "mcp__atlassian__getJiraIssue": {
                "config": format!("config/{config_filename}"),
                "prefix_from": "issueIdOrKey"
            }
        },
        "pre_tool_use": {}
    });
    fs::write(
        security_dir.join("tool-registry.json"),
        serde_json::to_string_pretty(&registry).unwrap(),
    )
    .unwrap();

    fs::create_dir_all(security_dir.join("config")).unwrap();
}

#[test]
fn test_post_tool_use_full_flow() {
    let security_dir = TempDir::new().unwrap();
    let session_dir = TempDir::new().unwrap();

    // Set up mockito server
    let mut server = mockito::Server::new();
    let mock = server
        .mock("POST", "/v1/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(openai_chat_completion_body())
        .create();

    // Create registry first (creates config/ dir), then write FLC config
    create_test_registry(security_dir.path(), "jira-task.toml");
    create_test_config(&security_dir.path().join("config"), &server.url());

    let jira_response = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/jira-task-response.json"),
    )
    .unwrap();

    let input = serde_json::json!({
        "session_id": "test123",
        "hook_event_name": "PostToolUse",
        "tool_name": "mcp__atlassian__getJiraIssue",
        "tool_input": {"issueIdOrKey": "TC-42"},
        "tool_response": jira_response
    });

    let output = Command::cargo_bin("agent-sentinel")
        .unwrap()
        .args([
            "hook",
            "post-tool-use",
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
        "PostToolUse"
    );
    let mcp_output = &response["hookSpecificOutput"]["updatedMCPToolOutput"];
    assert!(
        mcp_output.is_array(),
        "updatedMCPToolOutput must be content blocks array"
    );
    assert_eq!(mcp_output[0]["type"], "text");
    let text = mcp_output[0]["text"].as_str().unwrap();
    let text_json: serde_json::Value = serde_json::from_str(text).unwrap();
    let refs = text_json["refs"]
        .as_object()
        .expect("refs must be a JSON object");
    assert!(
        refs.contains_key("$TC42_REQ_1"),
        "refs should contain $TC42_REQ_1, got: {refs:?}"
    );
    assert!(
        refs.contains_key("$TC42_AC_1"),
        "refs should contain $TC42_AC_1, got: {refs:?}"
    );

    // Verify vars.json was created with expected content
    let vars_path = session_dir.path().join("vars.json");
    assert!(vars_path.exists(), "symref should have created vars.json");
    let vars: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&vars_path).unwrap()).unwrap();
    assert!(
        vars["$TC42_REQ_1"].is_object(),
        "vars.json should contain $TC42_REQ_1"
    );
    assert!(
        vars["$TC42_AC_1"].is_object(),
        "vars.json should contain $TC42_AC_1"
    );

    mock.assert();
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
    )
    .unwrap();

    let input = serde_json::json!({
        "tool_name": "mcp__github__list_repos",
        "tool_input": {},
        "tool_response": "{}"
    });

    Command::cargo_bin("agent-sentinel")
        .unwrap()
        .args([
            "hook",
            "post-tool-use",
            "--security-dir",
            security_dir.path().to_str().unwrap(),
        ])
        .env("AGENT_SENTINEL_SESSION_DIR", "/tmp/test")
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
    )
    .unwrap();

    let input = serde_json::json!({
        "tool_name": "mcp__atlassian__getJiraIssue",
        "tool_input": {"issueIdOrKey": "TC-42"},
        "tool_response": "{}"
    });

    Command::cargo_bin("agent-sentinel")
        .unwrap()
        .args([
            "hook",
            "post-tool-use",
            "--security-dir",
            security_dir.path().to_str().unwrap(),
        ])
        .env_remove("AGENT_SENTINEL_SESSION_DIR")
        .write_stdin(serde_json::to_string(&input).unwrap())
        .assert()
        .code(2);
}

#[test]
fn test_post_tool_use_with_object_tool_response() {
    let security_dir = TempDir::new().unwrap();
    let session_dir = TempDir::new().unwrap();

    // Set up mockito server
    let mut server = mockito::Server::new();
    let mock = server
        .mock("POST", "/v1/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(openai_chat_completion_body())
        .create();

    // Create FLC config pointing to mock server
    let config = format!(
        r#"api_url = "{url}/v1/chat/completions"
model = "test-model"
system_prompt = "Extract structured data. Return valid JSON."
temperature = 0.0
max_tokens = 2000
timeout_secs = 10
response_format = "json-object"
api_key = "test-key"
"#,
        url = server.url()
    );
    fs::create_dir_all(security_dir.path().join("config")).unwrap();
    fs::write(security_dir.path().join("config/github-issue.toml"), config).unwrap();

    let registry = serde_json::json!({
        "post_tool_use": {
            "mcp__github__issue_read": {
                "config": "config/github-issue.toml",
                "prefix_from": "issue_number"
            }
        },
        "pre_tool_use": {}
    });
    fs::write(
        security_dir.path().join("tool-registry.json"),
        serde_json::to_string_pretty(&registry).unwrap(),
    )
    .unwrap();

    let input = serde_json::json!({
        "session_id": "test456",
        "hook_event_name": "PostToolUse",
        "tool_name": "mcp__github__issue_read",
        "tool_input": {"method": "get", "owner": "mrizzi", "repo": "test", "issue_number": 1},
        "tool_response": {
            "title": "Add OAuth2 login",
            "body": "## Requirements\n1. OAuth2 login flow\n2. Session persistence",
            "state": "open",
            "labels": ["enhancement"]
        }
    });

    let output = Command::cargo_bin("agent-sentinel")
        .unwrap()
        .args([
            "hook",
            "post-tool-use",
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
        "PostToolUse"
    );
    let mcp_output = &response["hookSpecificOutput"]["updatedMCPToolOutput"];
    assert!(
        mcp_output.is_array(),
        "updatedMCPToolOutput must be content blocks array"
    );
    assert_eq!(mcp_output[0]["type"], "text");
    let text = mcp_output[0]["text"].as_str().unwrap();
    let text_json: serde_json::Value = serde_json::from_str(text).unwrap();
    let refs = text_json["refs"]
        .as_object()
        .expect("refs must be a JSON object");
    assert!(
        refs.contains_key("$1_REQ_1"),
        "refs should contain $1_REQ_1 (issue_number=1), got: {refs:?}"
    );
    assert!(
        refs.contains_key("$1_AC_1"),
        "refs should contain $1_AC_1, got: {refs:?}"
    );

    mock.assert();
}

#[test]
fn test_post_tool_use_symref_store_failure_fallback() {
    let security_dir = TempDir::new().unwrap();
    let session_dir = TempDir::new().unwrap();

    // Set up mockito server
    let mut server = mockito::Server::new();
    let mock = server
        .mock("POST", "/v1/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(openai_chat_completion_body())
        .create();

    // Create registry first (creates config/ dir), then write FLC config
    create_test_registry(security_dir.path(), "jira-task.toml");
    create_test_config(&security_dir.path().join("config"), &server.url());

    // Make session dir read-only so symref::store() fails writing vars.json
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(session_dir.path(), fs::Permissions::from_mode(0o555)).unwrap();
    }

    let input = serde_json::json!({
        "session_id": "test-fail",
        "hook_event_name": "PostToolUse",
        "tool_name": "mcp__atlassian__getJiraIssue",
        "tool_input": {"issueIdOrKey": "TC-42"},
        "tool_response": "{\"key\": \"TC-42\"}"
    });

    let output = Command::cargo_bin("agent-sentinel")
        .unwrap()
        .args([
            "hook",
            "post-tool-use",
            "--security-dir",
            security_dir.path().to_str().unwrap(),
        ])
        .env("AGENT_SENTINEL_SESSION_DIR", session_dir.path())
        .write_stdin(serde_json::to_string(&input).unwrap())
        .output()
        .unwrap();

    // Restore permissions for cleanup
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(session_dir.path(), fs::Permissions::from_mode(0o755)).unwrap();
    }

    assert!(
        output.status.success(),
        "Hook should succeed with fallback, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let response: serde_json::Value = serde_json::from_str(&stdout).unwrap();

    // Should still return valid PostToolUse output with the FLC extraction (no refs)
    assert_eq!(
        response["hookSpecificOutput"]["hookEventName"],
        "PostToolUse"
    );
    let mcp_output = &response["hookSpecificOutput"]["updatedMCPToolOutput"];
    assert!(mcp_output.is_array());

    // The output should contain the FLC extraction content, not refs
    let text = mcp_output[0]["text"].as_str().unwrap();
    assert!(
        text.contains("OAuth2 login"),
        "Fallback should return FLC extraction content"
    );

    mock.assert();
}
