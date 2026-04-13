#!/usr/bin/env bash
# SessionEnd hook: collect transcript and session artifacts
#
# Reads: stdin JSON from Claude Code (transcript_path, session_id)
# Requires: AGENT_SENTINEL_SESSION_DIR (set by SessionStart hook)
set -euo pipefail

HOOK_INPUT=$(cat)
TRANSCRIPT_PATH=$(printf '%s' "$HOOK_INPUT" | jq -r '.transcript_path // empty')

# Graceful degradation if session dir not available
if [[ -z "${AGENT_SENTINEL_SESSION_DIR:-}" ]]; then
  echo "WARN: AGENT_SENTINEL_SESSION_DIR not set. Skipping transcript collection." >&2
  exit 0
fi

# Copy transcript to session directory
if [[ -n "$TRANSCRIPT_PATH" && -f "$TRANSCRIPT_PATH" ]]; then
  cp "$TRANSCRIPT_PATH" "$AGENT_SENTINEL_SESSION_DIR/transcript.jsonl"
else
  echo "WARN: Transcript not found at '$TRANSCRIPT_PATH'. Skipping." >&2
fi
