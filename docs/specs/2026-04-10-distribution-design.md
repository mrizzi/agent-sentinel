# agent-sentinel: Distribution and Architecture Design

## Problem

agent-sentinel implements the Dual LLM security pattern for Claude Code
plugins. It needs to be distributed so that:

1. Any Claude Code plugin author can adopt it for their domain (SDLC, CRM,
   DevOps, etc.)
2. Security configs are code-reviewed and committed, not user-generated
3. End users install and go — zero security-layer configuration
4. Security updates (hook logic, guardrail improvements) propagate to all
   domain plugins via a single upgrade

The current implementation uses bash+jq hook scripts, which have a shell
injection attack surface when processing untrusted MCP tool responses.

## Design

### agent-sentinel is a Rust CLI + Claude Code plugin

**Rust CLI** — the hook engine:

```
agent-sentinel hook post-tool-use --security-dir <path>
agent-sentinel hook pre-tool-use --security-dir <path>
agent-sentinel hook session-start --security-dir <path>
agent-sentinel hook session-end --security-dir <path>
```

Each subcommand reads Claude Code's hook JSON from stdin, orchestrates
fortified-llm-client and symref, and writes the hook response JSON to
stdout. All JSON parsing and construction happens in Rust — untrusted
content never flows through shell variables.

**Claude Code plugin** — the plugin-author tool:

Provides `/config-skill` for creating and managing interception configs
for new MCP tools. This is a plugin-author tool, not user-facing.

### Installation

```bash
# Install the security engine + dependencies
cargo install agent-sentinel fortified-llm-client symref

# Or download pre-built binaries from GitHub releases
```

All three binaries land on PATH. agent-sentinel invokes fortified-llm-client
and symref as subprocesses, resolving them from PATH.

Pre-built binaries are published per platform on GitHub releases alongside
crates.io for Rust users.

### Domain plugin integration

Each domain plugin (e.g., sdlc-plugins) commits:

```
sdlc-plugins/plugins/sdlc-workflow/
├── hooks/
│   ├── session-start.sh
│   ├── post-tool-use.sh
│   ├── pre-tool-use.sh
│   └── session-end.sh
├── security/
│   ├── config/
│   │   ├── jira-task.toml
│   │   ├── jira-feature.toml
│   │   └── figma-design.toml
│   ├── schemas/
│   │   ├── jira-task.json
│   │   ├── jira-feature.json
│   │   └── figma-design.json
│   ├── patterns/
│   │   ├── injection.txt
│   │   └── output-safety.txt
│   └── tool-registry.json
├── profiles/
│   ├── plan-feature.json
│   ├── implement-task.json
│   └── verify-pr.json
└── skills/
    ├── plan-feature/
    ├── implement-task/
    └── verify-pr/
```

**Hook scripts are one-liners:**

```bash
#!/usr/bin/env bash
exec agent-sentinel hook post-tool-use --security-dir "$CLAUDE_PLUGIN_ROOT/security"
```

`$CLAUDE_PLUGIN_ROOT` resolves correctly because these are the domain
plugin's own hooks. `agent-sentinel` is on PATH.

**Settings profiles reference the domain plugin's own hooks:**

```json
{
  "hooks": {
    "PostToolUse": [{
      "matcher": "mcp__atlassian__.*",
      "hooks": [{
        "type": "command",
        "command": "\"$CLAUDE_PLUGIN_ROOT/hooks/post-tool-use.sh\"",
        "statusMessage": "Quarantining MCP response..."
      }]
    }]
  }
}
```

No cross-plugin path discovery. No environment variable conventions.

### tool-registry.json

Replaces the hardcoded case statement in the current bash hooks. Maps MCP
tool names to their fortified-llm-client config files:

```json
{
  "post_tool_use": {
    "mcp__atlassian__getJiraIssue": {
      "config": "config/jira-task.toml",
      "prefix_from": "issueIdOrKey"
    },
    "mcp__plugin_figma_figma__get_design_context": {
      "config": "config/figma-design.toml",
      "prefix_from": "nodeId"
    }
  },
  "pre_tool_use": {
    "mcp__atlassian__createJiraIssue": {},
    "mcp__atlassian__editJiraIssue": {},
    "mcp__atlassian__addCommentToJiraIssue": {},
    "mcp__atlassian__transitionJiraIssue": {}
  }
}
```

The `prefix_from` field tells agent-sentinel which field in `tool_input`
to use for deriving the symref prefix (e.g., `TC-42` from `issueIdOrKey`).

Tools not in the registry are passed through (no interception).

### /config-skill

A Claude Code skill provided by the agent-sentinel plugin. Invoked by
plugin authors (not end users) when adding interception for a new MCP tool.

**Invocation:** `/config-skill <tool-name>`

**Flow:**

1. Asks which MCP tool to intercept and what data it returns
2. Generates a JSON Schema for the structured extraction
3. Generates a fortified-llm-client TOML config with system prompt,
   response format, and guardrails
4. Creates default pattern files if they don't exist
5. Adds the tool to `tool-registry.json`
6. Validates the config by running fortified-llm-client with a sample input

**Output:** Files ready to commit to the domain plugin's `security/` directory.

The skill uses the agent-sentinel plugin's `$CLAUDE_PLUGIN_ROOT` to find
templates and reference configs. It writes output to the current working
directory (the domain plugin repo).

### Security model

| Artifact | Committed by | Rationale |
|---|---|---|
| schemas/*.json | Plugin maintainer | Format constraints are security boundaries — must be code-reviewed |
| patterns/*.txt | Plugin maintainer | Injection detection patterns — weakening them must be visible in git |
| config/*.toml | Plugin maintainer | Guardrail settings — disabling them must be visible in git |
| tool-registry.json | Plugin maintainer | Skipping interception for a tool must be visible in git |
| profiles/*.json | Plugin maintainer | Hook wiring — uses $CLAUDE_PLUGIN_ROOT, no user-specific paths |
| hooks/*.sh | Plugin maintainer | One-liners that exec into agent-sentinel — no logic to tamper with |
| LLM endpoint | End user (env var) | `api_url` in TOML uses a placeholder; `api_key_name` references an env var the user sets |

**Nothing in the security path is user-generated.** The only user-provided
values are the quarantined LLM endpoint URL (in the TOML, replaceable by
the maintainer for managed deployments) and the API key (via environment
variable).

### Exit code semantics

The agent-sentinel CLI enforces fail-closed behavior:

| Scenario | Exit code | Effect |
|---|---|---|
| Tool not in registry | 0 | Passthrough (no interception) |
| Successful extraction + store | 0 | updatedMCPToolOutput returned |
| fortified-llm-client extraction fails | 0 | Error JSON returned via updatedMCPToolOutput |
| Unexpected error (binary not found, invalid config, JSON parse failure) | 2 | Claude Code blocks the tool call |
| Session dir not set for intercepted tool | 2 | Claude Code blocks the tool call |

Exit 2 = fail closed. The privileged LLM never sees raw untrusted content
due to an infrastructure failure.

### Relationship to existing components

```
                    ┌─────────────────────────────────┐
                    │  Domain Plugin (e.g. sdlc-plugins)│
                    │                                   │
                    │  skills/     ← SDLC workflow      │
                    │  security/   ← configs, schemas   │
                    │  hooks/      ← one-liner wrappers │
                    │  profiles/   ← settings files     │
                    └──────────────┬────────────────────┘
                                   │ exec
                    ┌──────────────▼────────────────────┐
                    │  agent-sentinel CLI                │
                    │  (Rust binary on PATH)             │
                    │                                    │
                    │  hook post-tool-use                │
                    │  hook pre-tool-use                 │
                    │  hook session-start                │
                    │  hook session-end                  │
                    └───────┬──────────────┬────────────┘
                            │              │
               ┌────────────▼───┐   ┌──────▼──────┐
               │ fortified-     │   │   symref     │
               │ llm-client     │   │              │
               │ (Rust, PATH)   │   │ (Rust, PATH) │
               └────────────────┘   └──────────────┘
```

### Migration from current bash hooks

The current bash hooks in this repo serve as the reference implementation
and test harness. The migration path:

1. Implement `agent-sentinel` Rust CLI with the same logic
2. Replace the bash hooks with one-liner wrappers
3. Move SDLC-specific configs to sdlc-plugins
4. Test end-to-end with the domain plugin
5. Remove bash hook logic from this repo

The current bash hooks remain useful for development and testing until
the Rust CLI is complete.
