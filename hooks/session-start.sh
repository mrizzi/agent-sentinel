#!/usr/bin/env bash
# SessionStart hook: create session directory and export AGENT_SENTINEL_SESSION_DIR
#
# Reads: stdin JSON from Claude Code (session_id, cwd)
# Exports: AGENT_SENTINEL_SESSION_DIR via CLAUDE_ENV_FILE
set -euo pipefail

HOOK_INPUT=$(cat)
SESSION_ID=$(printf '%s' "$HOOK_INPUT" | jq -r '.session_id // empty')
CWD=$(printf '%s' "$HOOK_INPUT" | jq -r '.cwd // empty')

# Validate CLAUDE_ENV_FILE is available
if [[ -z "${CLAUDE_ENV_FILE:-}" ]]; then
  echo "ERROR: CLAUDE_ENV_FILE not available. Cannot export session directory." >&2
  exit 2
fi

# Create timestamped session directory
SESSION_DIR="/tmp/agent-sentinel-sessions/$(date +%Y%m%d-%H%M%S)-${SESSION_ID:0:8}"
mkdir -p "$SESSION_DIR/evaluations" "$SESSION_DIR/output"

# Write session metadata
jq -n \
  --arg session_id "$SESSION_ID" \
  --arg started_at "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
  --arg user "$(whoami)" \
  --arg cwd "$CWD" \
  '{ session_id: $session_id, started_at: $started_at, user: $user, cwd: $cwd }' \
  > "$SESSION_DIR/session-meta.json"

# Export session directory for subsequent hooks
echo "export AGENT_SENTINEL_SESSION_DIR='$SESSION_DIR'" >> "$CLAUDE_ENV_FILE"
