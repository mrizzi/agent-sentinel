# Project Configuration

## Repository Registry

| Repository | Role | Serena Instance |
|---|---|---|
| agent-sentinel | Rust security hook engine | serena-agent-sentinel |
| fortified-llm-client | Rust LLM client with guardrails | serena-fortified-llm-client |
| symref | Rust symbolic variable storage | serena-symref |

## Jira Configuration

- Project key: TC
- Cloud ID: redhat.atlassian.net
- Feature issue type ID: 10142
- Git Pull Request custom field: customfield_10875
- GitHub Issue custom field: customfield_10747

## Code Intelligence

Tools are prefixed by Serena instance name: `mcp__<instance>__<tool>`.

For example, to search for a symbol in the agent-sentinel repository:

    mcp__serena-agent-sentinel__find_symbol(
      name_path_pattern="MyService",
      substring_matching=true,
      include_body=false
    )

### Limitations

No known limitations.
