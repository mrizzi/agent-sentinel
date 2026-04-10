use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Deserialize)]
pub struct ToolRegistry {
    #[serde(default)]
    pub post_tool_use: HashMap<String, PostToolUseEntry>,
    #[serde(default)]
    pub pre_tool_use: HashMap<String, PreToolUseEntry>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct PostToolUseEntry {
    pub config: String,
    pub prefix_from: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct PreToolUseEntry {}

impl ToolRegistry {
    /// Load registry from a directory containing tool-registry.json
    pub fn load(security_dir: &Path) -> Result<Self> {
        let path = security_dir.join("tool-registry.json");
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        let registry: Self = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse {}", path.display()))?;
        Ok(registry)
    }

    pub fn lookup_post_tool_use(&self, tool_name: &str) -> Option<&PostToolUseEntry> {
        self.post_tool_use.get(tool_name)
    }

    pub fn is_pre_tool_use_intercepted(&self, tool_name: &str) -> bool {
        self.pre_tool_use.contains_key(tool_name)
    }
}

/// Derive a symref prefix from an issue key: "TC-42" → "TC42"
pub fn derive_prefix(issue_key: &str) -> String {
    issue_key
        .chars()
        .filter(|c| *c != '-')
        .collect::<String>()
        .to_uppercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/tool-registry.json")
    }

    #[test]
    fn test_load_registry() {
        let registry = ToolRegistry::load(fixture_path().parent().unwrap()).unwrap();
        assert_eq!(registry.post_tool_use.len(), 2);
        assert_eq!(registry.pre_tool_use.len(), 4);
    }

    #[test]
    fn test_lookup_post_tool_use_hit() {
        let registry = ToolRegistry::load(fixture_path().parent().unwrap()).unwrap();
        let entry = registry.lookup_post_tool_use("mcp__atlassian__getJiraIssue");
        assert!(entry.is_some());
        let entry = entry.unwrap();
        assert_eq!(entry.config, "config/jira-task.toml");
        assert_eq!(entry.prefix_from.as_deref(), Some("issueIdOrKey"));
    }

    #[test]
    fn test_lookup_post_tool_use_miss() {
        let registry = ToolRegistry::load(fixture_path().parent().unwrap()).unwrap();
        let entry = registry.lookup_post_tool_use("mcp__github__list_repos");
        assert!(entry.is_none());
    }

    #[test]
    fn test_lookup_pre_tool_use_hit() {
        let registry = ToolRegistry::load(fixture_path().parent().unwrap()).unwrap();
        assert!(registry.is_pre_tool_use_intercepted("mcp__atlassian__editJiraIssue"));
    }

    #[test]
    fn test_lookup_pre_tool_use_miss() {
        let registry = ToolRegistry::load(fixture_path().parent().unwrap()).unwrap();
        assert!(!registry.is_pre_tool_use_intercepted("mcp__github__create_pr"));
    }

    #[test]
    fn test_derive_prefix() {
        assert_eq!(derive_prefix("TC-42"), "TC42");
        assert_eq!(derive_prefix("PROJ-123-4"), "PROJ1234");
        assert_eq!(derive_prefix("abc"), "ABC");
    }
}
