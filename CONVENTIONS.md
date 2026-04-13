# Coding Conventions

## Language and Framework

- Shell scripts: Bash (`/bin/bash`)
- Configuration: TOML (fortified-llm-client configs)
- Schemas: JSON Schema (draft 2020-12)
- Patterns: plain text (regex, one per line with severity prefix)
- Profiles: JSON (Claude Code settings templates)

## Code Style

- Shell scripts: `set -euo pipefail` at the top
- Quoting: always double-quote variable expansions (`"$VAR"`)
- JSON output on stdout, diagnostics/warnings on stderr
- 2-space indentation for JSON files
- Standard TOML formatting for config files

## Naming Conventions

- Shell scripts: kebab-case (e.g., `post-tool-use.sh`, `session-start.sh`)
- Config files: kebab-case matching MCP tool domain (e.g., `jira-feature.toml`, `figma-design.toml`)
- Schema files: kebab-case matching config counterpart (e.g., `jira-feature.json`)
- Pattern files: kebab-case descriptive names (e.g., `injection.txt`, `output-safety.txt`)
- Profile files: kebab-case matching skill names (e.g., `plan-feature.json`, `implement-task.json`)
- Environment variables: UPPER_SNAKE_CASE (e.g., `AGENT_SENTINEL_SESSION_DIR`)
- Local shell variables: lower_snake_case (e.g., `tool_name`, `tool_response`)

## File Organization

- `hooks/` — Claude Code hook scripts (PostToolUse, PreToolUse, SessionStart, SessionEnd)
- `config/` — TOML configs for fortified-llm-client, one per MCP tool domain
- `schemas/` — JSON Schemas for extraction validation, one per MCP tool domain
- `patterns/` — regex pattern files for guardrail pre-screening
- `profiles/` — Claude Code settings templates, one per SDLC skill
- `docs/` — project documentation (constraints, methodology)
- Config and schema files are paired: `config/foo.toml` references `schemas/foo.json`

## Error Handling

- Exit codes: 0 (success), 1 (error), 2 (block/deny — scope violation)
- Structured error output: `{"error": "error_type", "message": "Human-readable description"}`
- Fallback on extraction failure: return error JSON so privileged LLM can request manual input
- Unresolved `$VAR` references: warn on stderr, exit 0 (partial success)

## Testing Conventions

- Hook integration tests: pipe JSON to stdin and verify stdout output
  ```bash
  echo '{"tool_name":"...", "tool_input":{}, "tool_response":"..."}' | hooks/post-tool-use.sh
  ```
- Verify side effects: session directory contents (vars.json, evaluations/)
- Verify output: summaries + `$VAR` refs instead of raw content

## Commit Messages

- Conventional Commits: `type(scope): description`
- Types: feat, fix, refactor, docs, test, ci, chore
- Jira issue reference in footer: `Implements TC-123`
- Attribution trailer: `Assisted-by: Claude Code`

## Shared Modules and Reuse

- No shared shell libraries — each hook script is self-contained
- Shared patterns: `patterns/injection.txt` used by all input guardrail configs
- Shared patterns: `patterns/output-safety.txt` used by all output guardrail configs
- Config/schema pairing: each `config/*.toml` references a corresponding `schemas/*.json`

## Documentation

- `CLAUDE.md` — comprehensive developer guide (architecture, flows, formats, status)
- `docs/constraints.md` — deterministic architectural rules for SDLC skills
- Architecture docs in sdlc-plugins repo (linked from CLAUDE.md)

## Dependencies

- External CLIs (must be on PATH): `fortified-llm-client`, `symref`, `mcp-guard` (deferred)
- Standard Unix tools: `bash`, `jq`, `mkdir`, `cat`
- Claude Code runtime: provides `CLAUDE_ENV_FILE` and hook execution
