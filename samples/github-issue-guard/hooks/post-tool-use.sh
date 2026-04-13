#!/usr/bin/env bash
SAMPLE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
AGENT_SENTINEL="${AGENT_SENTINEL_BIN:-$(cd "$SAMPLE_DIR/../.." && pwd)/target/release/agent-sentinel}"
exec "$AGENT_SENTINEL" hook post-tool-use --security-dir "$SAMPLE_DIR"
