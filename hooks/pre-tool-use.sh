#!/usr/bin/env bash
# PreToolUse hook: symbolic dereferencing for MCP write tools
#
# Flow:
#   1. Filter: only intercept Jira write tools
#   2. symref deref: substitute $VAR references with stored values
#   3. Return updatedInput with concrete values
#   4. (Future: mcp-guard scope check)
#
# Reads: stdin JSON from Claude Code (tool_name, tool_input)
# Requires: AGENT_SENTINEL_SESSION_DIR (set by SessionStart hook)
set -euo pipefail

# --- Path resolution ---

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
SYMREF="${SYMREF_BIN:-$(command -v symref 2>/dev/null)}"

# --- Read hook input ---

HOOK_INPUT=$(cat)
TOOL_NAME=$(printf '%s' "$HOOK_INPUT" | jq -r '.tool_name // empty')
TOOL_INPUT=$(printf '%s' "$HOOK_INPUT" | jq -c '.tool_input // {}')

# --- Tool name filter: only intercept Jira write tools ---

case "$TOOL_NAME" in
  mcp__atlassian__createJiraIssue|\
  mcp__atlassian__editJiraIssue|\
  mcp__atlassian__addCommentToJiraIssue|\
  mcp__atlassian__transitionJiraIssue)
    # Intercept — continue to dereferencing
    ;;
  *)
    # Not a write tool we intercept — passthrough
    exit 0
    ;;
esac

# --- Validate prerequisites ---

if [[ -z "${AGENT_SENTINEL_SESSION_DIR:-}" ]]; then
  echo "WARN: AGENT_SENTINEL_SESSION_DIR not set. Passthrough without dereferencing." >&2
  exit 0
fi

if [[ ! -f "$AGENT_SENTINEL_SESSION_DIR/vars.json" ]]; then
  echo "WARN: No vars.json in session dir. Passthrough without dereferencing." >&2
  exit 0
fi

# --- Invoke symref deref ---

DEREF_OUTPUT=""
DEREF_EXIT=0
DEREF_OUTPUT=$(printf '%s' "$TOOL_INPUT" | "$SYMREF" deref \
  --session "$AGENT_SENTINEL_SESSION_DIR" \
  2>/dev/null) || DEREF_EXIT=$?

if [[ $DEREF_EXIT -ne 0 ]]; then
  echo "WARN: symref deref failed (exit $DEREF_EXIT). Passthrough with original input." >&2
  exit 0
fi

# --- Future: mcp-guard scope check ---
# mcp-guard --scope "$AGENT_SENTINEL_SESSION_DIR/scope.json" \
#   --tool-name "$TOOL_NAME" \
#   --input <(echo "$DEREF_OUTPUT")
# If mcp-guard blocks: exit 2

# --- Return updatedInput ---

jq -n \
  --argjson updated_input "$DEREF_OUTPUT" \
  '{
    hookSpecificOutput: {
      hookEventName: "PreToolUse",
      updatedInput: $updated_input
    }
  }'
