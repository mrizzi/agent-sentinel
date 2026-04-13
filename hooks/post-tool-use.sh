#!/usr/bin/env bash
# PostToolUse hook: Dual LLM flow for MCP read interception
#
# Flow:
#   1. Map tool_name → config file
#   2. fortified-llm-client: extract structured data from untrusted content
#   3. symref store: save extraction as $VAR references
#   4. Return updatedMCPToolOutput with summaries + $VAR refs
#
# Reads: stdin JSON from Claude Code (tool_name, tool_input, tool_response)
# Requires: AGENT_SENTINEL_SESSION_DIR (set by SessionStart hook)
set -euo pipefail

# SECURITY: Convert any unexpected error to a blocking failure (exit 2).
# Without this, a failed hook causes Claude Code to show the raw untrusted
# tool response to the privileged LLM, bypassing the quarantine entirely.
trap 'echo "FATAL: PostToolUse hook failed unexpectedly at line $LINENO" >&2; exit 2' ERR

# --- Path resolution ---

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
AGENT_SENTINEL_DIR=$(dirname "$SCRIPT_DIR")

# Binary resolution: env var > PATH
FORTIFIED_LLM_CLIENT="${FORTIFIED_LLM_CLIENT_BIN:-$(command -v fortified-llm-client 2>/dev/null)}"
SYMREF="${SYMREF_BIN:-$(command -v symref 2>/dev/null)}"

# --- Read hook input ---

HOOK_INPUT=$(cat)
TOOL_NAME=$(printf '%s' "$HOOK_INPUT" | jq -r '.tool_name // empty')
TOOL_INPUT=$(printf '%s' "$HOOK_INPUT" | jq -c '.tool_input // {}')
TOOL_RESPONSE=$(printf '%s' "$HOOK_INPUT" | jq -r '.tool_response // empty')

# --- Tool name → config mapping ---

CONFIG=""
case "$TOOL_NAME" in
  mcp__atlassian__getJiraIssue)
    CONFIG="config/jira-task.toml"
    ;;
  # Future interceptions:
  # mcp__plugin_figma_figma__get_design_context) CONFIG="config/figma-design.toml" ;;
  # mcp__plugin_figma_figma__get_screenshot)     CONFIG="config/figma-design.toml" ;;
  *)
    # No interception configured — passthrough
    exit 0
    ;;
esac

# --- Validate prerequisites ---

if [[ -z "${AGENT_SENTINEL_SESSION_DIR:-}" ]]; then
  echo "ERROR: AGENT_SENTINEL_SESSION_DIR not set. Cannot quarantine without session." >&2
  exit 2
fi

if [[ -z "$TOOL_RESPONSE" ]]; then
  echo "ERROR: Empty tool_response for $TOOL_NAME." >&2
  exit 2
fi

CONFIG_PATH="$AGENT_SENTINEL_DIR/$CONFIG"
if [[ ! -f "$CONFIG_PATH" ]]; then
  echo "ERROR: Config file not found: $CONFIG_PATH" >&2
  exit 2
fi

if [[ ! -x "$FORTIFIED_LLM_CLIENT" ]]; then
  echo "ERROR: fortified-llm-client not found or not executable: $FORTIFIED_LLM_CLIENT" >&2
  echo "Set FORTIFIED_LLM_CLIENT_BIN or add to PATH" >&2
  exit 2
fi

if [[ ! -x "$SYMREF" ]]; then
  echo "ERROR: symref not found or not executable: $SYMREF" >&2
  echo "Set SYMREF_BIN or add to PATH" >&2
  exit 2
fi

# --- Derive issue key prefix for symref ---

# Extract issue key from tool_input (e.g. "TC-42") and convert to prefix (e.g. "TC42")
ISSUE_KEY=$(printf '%s' "$TOOL_INPUT" | jq -r '.issueIdOrKey // .issueKey // empty')
if [[ -z "$ISSUE_KEY" ]]; then
  ISSUE_KEY="UNKNOWN"
fi
PREFIX=$(printf '%s' "$ISSUE_KEY" | tr -d '-' | tr '[:lower:]' '[:upper:]')

# --- Invoke fortified-llm-client ---

# Write tool_response to temp file to avoid ARG_MAX limits
TEMP_INPUT=$(mktemp) || { echo "ERROR: Failed to create temp file" >&2; exit 2; }
trap 'rm -f "$TEMP_INPUT"' EXIT
printf '%s' "$TOOL_RESPONSE" > "$TEMP_INPUT"

FLC_OUTPUT=""
FLC_EXIT=0
FLC_OUTPUT=$("$FORTIFIED_LLM_CLIENT" \
  --config-file "$CONFIG_PATH" \
  --user-file "$TEMP_INPUT" \
  --quiet \
  2>/dev/null) || FLC_EXIT=$?

if [[ $FLC_EXIT -ne 0 ]]; then
  # Extraction failed — return fallback error to privileged LLM
  FLC_ERROR=""
  if [[ -n "$FLC_OUTPUT" ]]; then
    FLC_ERROR=$(printf '%s' "$FLC_OUTPUT" | jq -r '.error.message // "Unknown error"' 2>/dev/null || echo "Unknown error")
  fi
  ERROR_JSON=$(jq -n --arg tool "$TOOL_NAME" --arg error "$FLC_ERROR" \
    '{"error":"extraction_failed","message":"Could not safely extract content. Manual input required.","original_tool":$tool,"detail":$error}')
  jq -n --arg text "$ERROR_JSON" \
    '{hookSpecificOutput:{hookEventName:"PostToolUse",updatedMCPToolOutput:[{type:"text",text:$text}]}}'
  exit 0
fi

# --- Extract validated response ---

FLC_STATUS=$(printf '%s' "$FLC_OUTPUT" | jq -r '.status // empty')
if [[ "$FLC_STATUS" != "success" ]]; then
  FLC_ERROR=$(printf '%s' "$FLC_OUTPUT" | jq -r '.error.message // "Extraction returned non-success status"')
  ERROR_JSON=$(jq -n --arg tool "$TOOL_NAME" --arg error "$FLC_ERROR" \
    '{"error":"extraction_failed","message":"Could not safely extract content. Manual input required.","original_tool":$tool,"detail":$error}')
  jq -n --arg text "$ERROR_JSON" \
    '{hookSpecificOutput:{hookEventName:"PostToolUse",updatedMCPToolOutput:[{type:"text",text:$text}]}}'
  exit 0
fi

EXTRACTION=$(printf '%s' "$FLC_OUTPUT" | jq -c '.response')

# --- Invoke symref store ---

SYMREF_OUTPUT=""
SYMREF_EXIT=0
SYMREF_OUTPUT=$(printf '%s' "$EXTRACTION" | "$SYMREF" store \
  --session "$AGENT_SENTINEL_SESSION_DIR" \
  --prefix "$PREFIX" \
  2>/dev/null) || SYMREF_EXIT=$?

if [[ $SYMREF_EXIT -ne 0 ]]; then
  echo "WARN: symref store failed (exit $SYMREF_EXIT). Returning extraction without refs." >&2
  # Fall back to returning raw extraction as updatedMCPToolOutput
  FALLBACK_JSON=$(printf '%s' "$EXTRACTION" | jq -c '. + {_warning: "symref store failed; no $VAR refs assigned"}')
  jq -n --arg text "$FALLBACK_JSON" \
    '{hookSpecificOutput:{hookEventName:"PostToolUse",updatedMCPToolOutput:[{type:"text",text:$text}]}}'
  exit 0
fi

# --- Format updatedMCPToolOutput from symref refs ---

REFS=$(printf '%s' "$SYMREF_OUTPUT" | jq -c '.refs')

RESULT_JSON=$(jq -n --argjson refs "$REFS" --arg issue_key "$ISSUE_KEY" \
  '{"issue_key":$issue_key,"refs":$refs}')
jq -n --arg text "$RESULT_JSON" \
  '{hookSpecificOutput:{hookEventName:"PostToolUse",updatedMCPToolOutput:[{type:"text",text:$text}]}}'
