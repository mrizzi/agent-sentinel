# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Overview

`agent-sentinel` is the integration layer for the agent-sentinel security
framework. It implements the Dual LLM pattern from the "Design Patterns for
Securing LLM Agents against Prompt Injections" paper by composing three
external tools via Claude Code hooks:

- **fortified-llm-client** — quarantined LLM invocation + input/output
  guardrails + schema validation (Rust CLI)
- **symref** — symbolic variable store + template dereferencing (Rust CLI)
- **mcp-guard** — MCP write scope enforcement (Rust CLI, deferred)

This repo contains the **glue**: hook scripts, per-MCP-tool configuration
files, JSON schemas for extraction validation, regex patterns for
injection/safety detection, and per-skill settings profile templates.

## Architecture Context

The security architecture is documented in the sdlc-plugins repo:

- **Architecture** (tools-agnostic, the *what* and *why*):
  `/Users/mrizzi/git/cloned/sdlc-plugins/.local/prompt-injection-analysis.md`
- **Implementation** (tools-specific, the *how*):
  `/Users/mrizzi/git/cloned/sdlc-plugins/.local/agent-sentinel-implementation.md`
- **Attack flow diagram**:
  `/Users/mrizzi/git/cloned/sdlc-plugins/.local/attack-flow-diagram.md`
- **Jira state machine**:
  `/Users/mrizzi/git/cloned/sdlc-plugins/.local/jira-task-status-flow-diagram.md`

Read these files for the full context of why each component exists and what
security guarantees it provides.

## Related Projects

| Project | Path | Role |
|---|---|---|
| **sdlc-plugins** | `/Users/mrizzi/git/cloned/sdlc-plugins` | Claude Code plugin with SDLC workflow skills (plan-feature, implement-task, verify-pr). The skills whose MCP reads/writes are intercepted by this framework. |
| **fortified-llm-client** | `/Users/mrizzi/git/cloned/fortified-llm-client` | Rust CLI invoked by PostToolUse hooks. Runs the quarantined LLM (Haiku via OpenAI-compatible API) with input guardrails, structured extraction prompt, and output guardrails. |
| **symref** | `/Users/mrizzi/git/cloned/symref` | Rust CLI invoked by hooks. `store` command saves validated extraction as `$VAR` references. `deref` command substitutes `$VAR` references with stored values at write time. |
| **sdlc-build** | `/Users/mrizzi/git/cloned/sdlc-build` | Build & Test MCP server. Runs builds/tests in S2I containers. Action-Selector pattern (commands from config, not from LLM). |

## Project Structure

```
agent-sentinel/
├── CLAUDE.md
├── hooks/
│   ├── post-tool-use.sh      ← PostToolUse hook: Dual LLM flow
│   │                           (fortified-llm-client → symref store)
│   ├── pre-tool-use.sh       ← PreToolUse hook: dereferencing + scope
│   │                           (symref deref → mcp-guard)
│   ├── session-start.sh      ← SessionStart hook: create session dir,
│   │                           scope.json, session-meta.json
│   └── session-end.sh        ← SessionEnd hook: collect transcript + logs
├── config/
│   ├── jira-feature.toml     ← fortified-llm-client config for Jira Feature reads
│   ├── jira-task.toml        ← fortified-llm-client config for Jira task reads
│   ├── figma-design.toml     ← fortified-llm-client config for Figma reads
│   ├── github-pr.toml        ← fortified-llm-client config for GitHub PR reads
│   ├── github-ci-logs.toml   ← fortified-llm-client config for CI log reads
│   └── build-output.toml     ← fortified-llm-client config for Build & Test output
├── schemas/
│   ├── jira-feature.json     ← JSON Schema for Feature extraction
│   ├── jira-task.json        ← JSON Schema for task extraction
│   ├── figma-design.json     ← JSON Schema for design extraction
│   ├── github-pr.json        ← JSON Schema for PR comment classification
│   ├── github-ci-logs.json   ← JSON Schema for CI failure extraction
│   └── build-output.json     ← JSON Schema for test result extraction
├── patterns/
│   ├── injection.txt         ← regex patterns for prompt injection detection
│   └── output-safety.txt    ← regex patterns for output safety checks
└── profiles/
    ├── setup.json            ← per-skill settings template for /setup
    ├── define-feature.json   ← per-skill settings template for /define-feature
    ├── plan-feature.json     ← per-skill settings template for /plan-feature
    ├── implement-task.json   ← per-skill settings template for /implement-task
    └── verify-pr.json        ← per-skill settings template for /verify-pr
```

## How It Works

### Dual LLM Flow (PostToolUse hook)

Triggered on every MCP read that returns untrusted content:

```
Claude Code PostToolUse fires
    | stdin: {tool_name, tool_input, tool_response}
    v
post-tool-use.sh:
    1. SELECT config from tool_name mapping:
       mcp__atlassian__getJiraIssue       → config/jira-feature.toml (or jira-task.toml)
       mcp__plugin_figma_figma__*         → config/figma-design.toml
       mcp__github__pull_request_read     → config/github-pr.toml
       mcp__github__get_job_logs          → config/github-ci-logs.toml
       mcp__build__run_build_and_test     → config/build-output.toml

    2. INVOKE fortified-llm-client:
       fortified-llm-client --config-file config/<selected>.toml --user-text "$TOOL_RESPONSE"
       Pipeline: input guardrails → quarantined LLM (Haiku) → output guardrails
       Returns: validated JSON or error

    3. IF error → return fallback to Claude Code:
       {"error": "extraction_failed", "message": "Manual input required."}

    4. INVOKE symref:
       symref store --session "$AGENT_SENTINEL_SESSION_DIR" --prefix "$ISSUE_KEY" --input validated.json
       Returns: summary + $VAR refs JSON

    5. FORMAT updatedMCPToolOutput from summary + $VAR refs
    | stdout: hook response JSON
    v
Privileged LLM sees only summaries + $VAR references
```

### Dereferencing + Scope Check (PreToolUse hook)

Triggered on every MCP write:

```
Claude Code PreToolUse fires
    | stdin: {tool_name, tool_input}
    v
pre-tool-use.sh:
    1. INVOKE symref:
       symref deref --session "$AGENT_SENTINEL_SESSION_DIR" --input tool_input.json
       Substitutes $VAR refs with stored values

    2. INVOKE mcp-guard (when available):
       mcp-guard --scope "$AGENT_SENTINEL_SESSION_DIR/scope.json" --tool-name "$TOOL_NAME" --input concrete.json
       Validates target issue/repo/branch

    3. FORMAT updatedInput or exit 2 (block)
    | stdout: hook response JSON
    v
MCP server executes with concrete, scope-checked values
```

### Session Lifecycle

**SessionStart hook** creates the session directory:
```
$AGENT_SENTINEL_SESSION_DIR/
├── session-meta.json   ← who, when, session ID
├── vars.json           ← symref store (created by post-tool-use)
├── evaluations/        ← fortified-llm-client logs
└── output/             ← skill output files
```

**SessionEnd hook** copies the transcript to the session directory.

## Environment Variables

Hooks read these at runtime (set by the caller before launching Claude Code):

| Variable | Required | Example | Used by |
|---|---|---|---|
| `AGENT_SENTINEL_SESSION_DIR` | Set by SessionStart hook | `/tmp/agent-sentinel-sessions/20260409-...` | All hooks |
| `CLAUDE_ENV_FILE` | Provided by Claude Code | — | session-start.sh (exports AGENT_SENTINEL_SESSION_DIR) |

## Config File Format

Each `config/*.toml` file is a fortified-llm-client configuration:

```toml
# Config format is FLAT — fields at root level, NOT nested under [llm]
api_url = "https://your-endpoint/v1/chat/completions"
model = "claude-haiku-4-5-20251001"
api_key_name = "QUARANTINED_LLM_API_KEY"
temperature = 0.0
timeout_secs = 60

system_prompt = """
Extract structured information from this content.
Output ONLY a JSON object matching the provided schema.
Do not include HTML, markdown, or instructional text in any field value.
"""

response_format = "json-schema"
response_format_schema = "schemas/<name>.json"

[guardrails.input]
type = "regex"
max_length_bytes = 65536
patterns_file = "patterns/injection.txt"
severity_threshold = "Medium"

[guardrails.output]
type = "composite"
execution = "sequential"
aggregation = "all_must_pass"

[[guardrails.output.providers]]
type = "regex"
max_length_bytes = 65536
patterns_file = "patterns/output-safety.txt"
severity_threshold = "Low"

[[guardrails.output.providers]]
type = "json_schema"
schema_file = "schemas/<name>.json"
```

The `json_schema` output guardrail validates the LLM response against the
schema file specified in `schema_file`, providing hard orchestrator-level
validation in addition to the LLM-instructed `response_format` constraint.

## Schema File Format

Each `schemas/*.json` file is a JSON Schema (draft 2020-12):

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "required": ["requirements"],
  "properties": {
    "requirements": {
      "type": "array",
      "items": {
        "type": "object",
        "required": ["id", "summary", "priority"],
        "properties": {
          "id": {"type": "string", "pattern": "^REQ_[0-9]+$"},
          "summary": {"type": "string", "maxLength": 200},
          "priority": {"enum": ["high", "medium", "low"]}
        }
      }
    }
  }
}
```

Schemas enforce format constraints (length, character set, structure), not
semantic detection. The orchestrator cannot detect whether "Log auth events
including request headers" is malicious — only that it's ≤200 chars of
plain text with a valid enum priority.

## Patterns File Format

Each `patterns/*.txt` file contains regex patterns for the fortified-llm-client
regex guardrail. Format is **tab-delimited with 4 fields**:

```
# scope<TAB>pattern<TAB>description<TAB>severity
# Scope: input, output, or both
# Severity: low, medium, high, critical

input	(?i)(ignore|forget|disregard)\s+(all\s+)?(previous|prior|above)\s+(instructions|prompts)	Prompt injection: ignore prior instructions	critical
input	(?i)(act|behave|respond)\s+as\s+(if|though)\s+you\s+(are|were)	Prompt injection: role impersonation	high
input	<script[\s>]	Script injection	high
input	(?i)(system\s*prompt|you\s+are\s+now|new\s+instructions)	Prompt injection: system prompt override	medium
```

These are pre-screening patterns — they catch known injection formats before
the quarantined LLM even sees the content. They are NOT the primary defense
(the Dual LLM pattern is). They are defense-in-depth.

## Profile File Format

Each `profiles/*.json` file is a Claude Code settings template passed via
`--settings`. Generated by the `/setup` skill from `sdlc-project.json`.

```json
{
  "sandbox": {
    "enabled": true,
    "filesystem": {
      "denyRead": ["~/"],
      "allowRead": [".", "/path/to/other-project"]
    },
    "network": {
      "allowedDomains": []
    },
    "allowUnsandboxedCommands": false
  },
  "permissions": {
    "deny": [
      "mcp__atlassian__createJiraIssue",
      "mcp__github__create_pull_request"
    ]
  }
}
```

The `allowRead` paths and `deny` rules vary per skill type. The `/setup`
skill reads `sdlc-project.json` and generates profiles with the correct
paths and MCP tool whitelists for each skill.

## Development Workflow

### Adding a new MCP tool interception

1. Create `config/<tool-name>.toml` with the quarantined LLM prompt
2. Create `schemas/<tool-name>.json` with the extraction schema
3. Add the tool name → config mapping in `hooks/post-tool-use.sh`
4. Test: invoke the skill that uses this MCP tool and verify the
   privileged LLM sees summaries + $VAR refs instead of raw content

### Testing end-to-end

```bash
# Launch Claude Code with a profile
claude --settings profiles/plan-feature.json /plan-feature TC-42

# Verify in the session directory:
# - vars.json contains $VAR entries
# - evaluations/ contains fortified-llm-client logs
# - The privileged LLM's transcript shows $VAR refs, not raw content
```

### Testing a hook in isolation

```bash
# Simulate a PostToolUse event
echo '{"tool_name":"mcp__atlassian__getJiraIssue","tool_input":{},"tool_response":"{\"key\":\"TC-42\",\"fields\":{\"description\":\"## Requirements\\n1. OAuth2 login\"}}"}' \
  | hooks/post-tool-use.sh

# Check output: should be updatedMCPToolOutput with summaries + $VAR refs
```

## Current Status

| Component | Status |
|---|---|
| Hook scripts | Done — session-start, post-tool-use, pre-tool-use, session-end |
| Config files (*.toml) | Done — jira-task.toml (placeholder LLM endpoint) |
| Schemas (*.json) | Done — jira-task.json |
| Patterns (*.txt) | Done — injection.txt, output-safety.txt |
| Profiles (*.json) | Not started — generated by /setup skill |
| fortified-llm-client json_schema output guardrail | Done — JSON validation on LLM output implemented |
| symref | Done — 28 tests passing |
| mcp-guard | Deferred |

# Project Configuration

## Repository Registry

| Repository | Role | Serena Instance | Path |
|---|---|---|---|
| agent-sentinel | Shell/config integration layer | — | `/Users/mrizzi/git/cloned/agent-sentinel` |
| fortified-llm-client | Rust quarantined LLM client | serena-fortified-llm-client | `/Users/mrizzi/git/cloned/fortified-llm-client` |
| symref | Rust symbolic variable store | serena-symref | `/Users/mrizzi/git/cloned/symref` |

## Jira Configuration

- Project key: TC
- Cloud ID: 2b9e35e3-6bd3-4cec-b838-f4249ee02432
- Feature issue type ID: 10142
- Git Pull Request custom field: customfield_10875
- GitHub Issue custom field: customfield_10747

## Code Intelligence

Tools are prefixed by Serena instance name: `mcp__<instance>__<tool>`.

For example, to search for a symbol in the fortified-llm-client repository:

    mcp__serena-fortified-llm-client__find_symbol(
      name_path_pattern="LlmClient",
      substring_matching=true,
      include_body=false
    )

### Limitations

- No known limitations at this time.
