use serde::{Deserialize, Serialize};

/// Claude Code hook input (stdin JSON for all hook types)
#[derive(Debug, Deserialize)]
pub struct HookInput {
    pub session_id: Option<String>,
    pub hook_event_name: Option<String>,
    pub tool_name: String,
    pub tool_input: Option<serde_json::Value>,
    pub tool_response: Option<String>,
    pub transcript_path: Option<String>,
    pub cwd: Option<String>,
}

/// Claude Code hook output (stdout JSON)
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookOutput {
    pub hook_specific_output: HookSpecificOutput,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookSpecificOutput {
    pub hook_event_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "updatedMCPToolOutput")]
    pub updated_mcp_tool_output: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_input: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub additional_context: Option<String>,
}

impl HookOutput {
    pub fn post_tool_use(updated_output: serde_json::Value) -> Self {
        Self {
            hook_specific_output: HookSpecificOutput {
                hook_event_name: "PostToolUse".to_string(),
                updated_mcp_tool_output: Some(updated_output),
                updated_input: None,
                additional_context: None,
            },
        }
    }

    pub fn pre_tool_use(updated_input: serde_json::Value) -> Self {
        Self {
            hook_specific_output: HookSpecificOutput {
                hook_event_name: "PreToolUse".to_string(),
                updated_mcp_tool_output: None,
                updated_input: Some(updated_input),
                additional_context: None,
            },
        }
    }

    pub fn extraction_failed(tool_name: &str, detail: &str) -> Self {
        Self::post_tool_use(serde_json::json!({
            "error": "extraction_failed",
            "message": "Could not safely extract content. Manual input required.",
            "original_tool": tool_name,
            "detail": detail
        }))
    }
}

impl HookInput {
    /// Read hook input from stdin
    pub fn from_stdin() -> anyhow::Result<Self> {
        let input = std::io::read_to_string(std::io::stdin())?;
        let parsed: Self = serde_json::from_str(&input)?;
        Ok(parsed)
    }

    /// Get a field from tool_input by name
    pub fn tool_input_field(&self, field: &str) -> Option<String> {
        self.tool_input
            .as_ref()
            .and_then(|v| v.get(field))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_post_tool_use_input() {
        let json = r#"{
            "session_id": "abc123",
            "hook_event_name": "PostToolUse",
            "tool_name": "mcp__atlassian__getJiraIssue",
            "tool_input": {"issueIdOrKey": "TC-42"},
            "tool_response": "{\"key\":\"TC-42\"}"
        }"#;
        let input: HookInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.tool_name, "mcp__atlassian__getJiraIssue");
        assert_eq!(input.tool_response.unwrap(), "{\"key\":\"TC-42\"}");
    }

    #[test]
    fn test_parse_pre_tool_use_input() {
        let json = r#"{
            "session_id": "abc123",
            "hook_event_name": "PreToolUse",
            "tool_name": "mcp__atlassian__editJiraIssue",
            "tool_input": {"issueKey": "TC-42", "description": "$TC42_REQ_1"}
        }"#;
        let input: HookInput = serde_json::from_str(json).unwrap();
        assert!(input.tool_response.is_none());
    }

    #[test]
    fn test_serialize_post_tool_use_output() {
        let output = HookOutput::post_tool_use(serde_json::json!({
            "issue_key": "TC-42",
            "refs": {"$TC42_REQ_1": {"summary": "OAuth2", "ref": "$TC42_REQ_1"}}
        }));
        let json = serde_json::to_string(&output).unwrap();
        assert!(json.contains("updatedMCPToolOutput"));
        assert!(json.contains("PostToolUse"));
    }

    #[test]
    fn test_serialize_pre_tool_use_output() {
        let output = HookOutput::pre_tool_use(serde_json::json!({
            "issueKey": "TC-42",
            "description": "OAuth2 login flow"
        }));
        let json = serde_json::to_string(&output).unwrap();
        assert!(json.contains("updatedInput"));
        assert!(json.contains("PreToolUse"));
    }

    #[test]
    fn test_serialize_extraction_failed() {
        let output = HookOutput::extraction_failed("mcp__atlassian__getJiraIssue", "timeout");
        let json = serde_json::to_string(&output).unwrap();
        assert!(json.contains("extraction_failed"));
        assert!(json.contains("Manual input required"));
    }
}
