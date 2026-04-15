# agent-sentinel

Security hook engine implementing the [Dual LLM pattern](https://simonwillison.net/2023/Apr/25/dual-llm-pattern/) ([Beurer-Kellner et al., 2025](https://arxiv.org/abs/2506.08837)) for [Claude Code](https://docs.anthropic.com/en/docs/claude-code).

Intercepts MCP tool responses via Claude Code hooks, quarantines them through a secondary LLM ([fortified-llm-client](https://github.com/mrizzi/fortified-llm-client)), and returns only structured extractions to the privileged LLM. Prevents prompt injection from untrusted tool responses crossing into the agent's context.

## How it works

agent-sentinel registers four [Claude Code hooks](https://docs.anthropic.com/en/docs/claude-code/hooks) that form a security pipeline around MCP tool calls:

| Hook | Purpose |
|------|---------|
| **SessionStart** | Creates a per-session directory for variable storage and metadata |
| **PostToolUse** | Quarantines the tool response through a secondary LLM, stores structured extraction as symbolic variables |
| **PreToolUse** | Dereferences `$VAR` symbolic references in tool inputs before execution |
| **SessionEnd** | Archives the session transcript |

The security-critical path is **PostToolUse**:

```
MCP tool response (untrusted)
  │
  ▼
agent-sentinel PostToolUse hook
  │
  ├─► fortified-llm-client (quarantined LLM extraction)
  │     Returns structured JSON only — no raw content crosses the boundary
  │
  ├─► symref::store() (assigns symbolic $VAR references)
  │
  ▼
Structured extraction + refs → stdout → Claude Code (privileged LLM)
```

The untrusted MCP tool response never reaches Claude Code directly. Only the structured extraction produced by the quarantined LLM crosses the boundary.

## Security model

- **Structural boundary** — Untrusted content is quarantined by architecture, not by prompting the LLM to ignore it
- **Extraction only** — Only structured JSON fields produced by the quarantine LLM cross the boundary
- **Error sanitization** — Error codes are filtered to `[a-zA-Z0-9_]` before crossing; error messages never cross
- **Fail-closed** — Intercepted tools require a valid session directory; missing session causes a hard failure
- **Symbolic indirection** — Extracted values are stored as `$PREFIX_FIELD` references, adding a layer of indirection between untrusted content and tool inputs

## How to use it

### Prerequisites

- Rust 1.86+
- An LLM API key for the quarantine LLM (configured in fortified-llm-client)
- One or more MCP servers configured in Claude Code

### Build

```bash
git clone https://github.com/mrizzi/agent-sentinel.git
cd agent-sentinel
cargo build --release
# Binary at target/release/agent-sentinel
```

The release binary is fully self-contained — no external binaries or PATH dependencies required.

### Set up a security directory

agent-sentinel expects a `--security-dir` containing the tool registry and FLC configs:

```
security/
├── tool-registry.json
└── config/
    └── jira-task.toml
```

#### tool-registry.json

Defines which MCP tools to intercept and how:

```json
{
  "post_tool_use": {
    "mcp__atlassian__getJiraIssue": {
      "config": "config/jira-task.toml",
      "prefix_from": "issueIdOrKey"
    }
  },
  "pre_tool_use": {
    "mcp__atlassian__editJiraIssue": {},
    "mcp__atlassian__createJiraIssue": {}
  }
}
```

- **post_tool_use** entries specify a `config` (FLC config file path relative to security-dir) and `prefix_from` (which field in `tool_input` to derive the variable prefix from, e.g. `"issueIdOrKey"` → `"TC-42"` → prefix `TC42`)
- **pre_tool_use** entries list tools whose inputs should have `$VAR` references dereferenced before execution
- Tools not listed in either section pass through unmodified

#### FLC config (TOML)

Each `config` file tells fortified-llm-client how to extract structured data:

```toml
api_url = "https://api.openai.com/v1/chat/completions"
model = "gpt-4o"
system_prompt = "Extract the following fields from the Jira issue..."
temperature = 0.0
max_tokens = 2000
timeout_secs = 30
response_format = "json-object"
api_key_name = "OPENAI_API_KEY"
```

- `api_key_name` is the name of an environment variable containing the API key (resolved at runtime)
- `system_prompt` should instruct the LLM to extract specific fields into a JSON structure
- `response_format = "json-object"` ensures the extraction is valid JSON
- See the [fortified-llm-client configuration guide](https://mrizzi.github.io/fortified-llm-client/user-guide/configuration.html) for the full config reference

### Configure Claude Code hooks

Add the hooks to your Claude Code settings (`.claude/settings.json` or project-level):

```json
{
  "hooks": {
    "SessionStart": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "/path/to/agent-sentinel hook session-start --security-dir /path/to/security"
          }
        ]
      }
    ],
    "PostToolUse": [
      {
        "matcher": "mcp__atlassian__.*",
        "hooks": [
          {
            "type": "command",
            "command": "/path/to/agent-sentinel hook post-tool-use --security-dir /path/to/security",
            "statusMessage": "Quarantining MCP response..."
          }
        ]
      }
    ],
    "PreToolUse": [
      {
        "matcher": "mcp__atlassian__.*(create|edit|addComment|transition).*",
        "hooks": [
          {
            "type": "command",
            "command": "/path/to/agent-sentinel hook pre-tool-use --security-dir /path/to/security",
            "statusMessage": "Dereferencing symbolic variables..."
          }
        ]
      }
    ],
    "SessionEnd": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "/path/to/agent-sentinel hook session-end --security-dir /path/to/security"
          }
        ]
      }
    ]
  }
}
```

- `matcher` uses regex to filter which tools trigger the hook
- Replace `/path/to/agent-sentinel` with the actual binary path
- Replace `/path/to/security` with your security directory path

### Verify it works

1. Start a Claude Code session — the SessionStart hook should create a session directory under `$TMPDIR/agent-sentinel-sessions/`
2. Use a tool that matches a `post_tool_use` registry entry — the hook should intercept the response, run it through FLC, and return structured extraction with `$VAR` refs
3. Check the session directory for `vars.json` (symbolic variable store)

## Testing

```bash
cargo test
```

The test suite includes boundary crossing tests that verify injection payloads (prompt injection, HTML comment injection, tracking beacons, fake compliance footers, role impersonation) cannot cross the Dual LLM boundary into the privileged LLM's context.
