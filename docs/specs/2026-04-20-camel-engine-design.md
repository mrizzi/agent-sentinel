# CaMeL Engine — Design Specification

A Rust implementation of the CaMeL architecture from "Defeating Prompt Injections by Design"
(Debenedetti et al., arXiv:2503.18813), with the Python reference implementation at
github.com/google-research/camel-prompt-injection as the authoritative source for behavior.

The engine safeguards any skill against prompt injection by tracking data provenance at the
value level through a Python-subset interpreter and enforcing security policies on
side-effecting tool calls.

## References

- Paper: https://arxiv.org/abs/2503.18813
- Python reference: https://github.com/google-research/camel-prompt-injection

## Design Decisions

| Decision | Choice | Rationale |
|---|---|---|
| Crate structure | Library + CLI binary | Faithful to paper (one system), idiomatic Rust (lib/binary split), reusable |
| LLM-generated language | Python subset | Paper's choice. LLMs are fluent in Python. Security comes from the interpreter, not the syntax |
| Security policies | Declarative rules in skill config | Base policy covers most cases. Auditable. No expression language needed |
| Quarantined LLM | Built-in via fortified-llm-client | FLC provides provider abstraction and structured output. Self-contained engine |
| Capability granularity | Per-value, strings as whole units | Matches paper's formal model. Per-character (Python ref) is an implementation detail, not a requirement |
| Python subset scope | Full subset matching reference | Partial interpreter would break LLM-generated code that uses comprehensions, class defs, try/except |

## Architecture Overview

Two crates in a Cargo workspace:

**`camel-core`** (library) — the CaMeL algorithm:
- Value system with capability metadata tracking
- Python subset interpreter via `rustpython-parser`
- Security policy engine
- Quarantined LLM via `fortified-llm-client`
- Skill config loader and system prompt generator

**`camel-cli`** (binary) — the entry point:
- Loads skill config TOML
- Accepts user query
- Calls privileged LLM to generate Python code
- Passes code to interpreter
- Retries on errors
- Outputs result

### Data Flow

```
User Query
    |
    v
Privileged LLM (generates Python code using tool signatures)
    |
    v
Python AST Parser (rustpython-parser)
    |
    v
CaMeL Interpreter (executes code, tracks capabilities on every value)
    |-- Tool calls --> execute real tools, tag return values with Tool source
    |-- query_ai_assistant --> FLC quarantined LLM, returns structured data
    |-- Side-effecting tool calls --> security policy check on argument capabilities
    |
    v
Result (or error --> fed back to Privileged LLM for retry)
```

## Value System & Capabilities

Every runtime value carries provenance metadata. This is the heart of CaMeL.

### Sources

Where a value came from:

```rust
enum Source {
    User,                  // literal in LLM-generated code
    CaMeL,                 // produced by the interpreter (loop var, comparison result)
    Tool(ToolSource),      // returned by a tool call
    TrustedToolSource,     // returned by a tool marked trusted in config
}

struct ToolSource {
    tool_name: String,
    inner_sources: BTreeSet<Source>,
}
```

### Readers

Who can see the value:

```rust
enum Readers {
    Public,                       // anyone
    Restricted(BTreeSet<String>), // named readers only
}
```

### Capabilities

Combined metadata on every value:

```rust
struct Capabilities {
    sources: BTreeSet<Source>,
    readers: Readers,
}
```

### CamelValue

```rust
struct CamelValue {
    inner: ValueKind,
    capabilities: Capabilities,
    dependencies: Vec<Arc<CamelValue>>,
}

enum ValueKind {
    None,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    List(Vec<CamelValue>),
    Tuple(Vec<CamelValue>),
    Dict(Vec<(CamelValue, CamelValue)>),
    Set(Vec<CamelValue>),
    ClassInstance(ClassInstance),
    Function(FunctionRef),
    Class(ClassDef),
}

struct ClassDef {
    name: String,
    fields: Vec<(String, String)>,  // (field_name, type_annotation)
    methods: HashMap<String, FunctionRef>,
    is_totally_ordered: bool,
    is_builtin: bool,
}

struct ClassInstance {
    class: Arc<ClassDef>,
    fields: HashMap<String, CamelValue>,
    frozen: bool,
}

struct FunctionRef {
    name: String,
    callable: FunctionCallable,  // enum: BuiltinFn | ToolFn | ClassConstructor
    receiver: Option<Arc<CamelValue>>,
    is_builtin: bool,
}
```

### Capability Propagation Rules

- **Binary operations**: result gets `Source::CaMeL`, dependencies include both operands
- **Tool call return values**: tagged with `Source::Tool(tool_name)`, `Readers::Public`
- **Trusted tool return values**: tagged with `Source::TrustedToolSource`, `Readers::Public`
- **Constants in generated code**: tagged with `Source::User`, `Readers::Public`
- **Interpreter-produced values**: tagged with `Source::CaMeL`
- **`get_all_readers(value)`**: walks dependency chain, intersects all readers
- **`get_all_sources(value)`**: walks dependency chain, unions all sources
- **`is_public(value)`**: `get_all_readers(value) == Readers::Public`
- **`is_trusted(value)`**: all sources in `{User, CaMeL, TrustedToolSource}`

## Python Subset Interpreter

Parses Python source via `rustpython-parser`, walks the AST, evaluates while threading
`CamelValue` capability metadata through every operation.

### Core Evaluation Signature

```rust
struct EvalState {
    namespace: Namespace,
    tool_calls: Vec<FunctionCall>,
    dependencies: Vec<Arc<CamelValue>>,
}

enum EvalResult {
    Ok(CamelValue),
    Error(CamelException),
}
```

### Namespace

```rust
struct Namespace {
    variables: HashMap<String, CamelValue>,
}
```

### Supported AST Nodes

**Expressions:**
- Literals: int, float, str, bool, None (tagged `Source::User`)
- Names: variable lookup
- Attributes: field access
- Subscript: index/key access
- Binary ops: `+`, `-`, `*`, `/`, `//`, `%`, `**`, `|`, `&`, `^`, `<<`, `>>`
- Unary ops: `-`, `+`, `~`, `not`
- Boolean ops: `and`, `or`
- Compare ops: `==`, `!=`, `<`, `>`, `<=`, `>=`, `in`, `not in`, `is`, `is not`
- Call: function/method calls (security policy checks happen here)
- IfExp: ternary
- F-strings: JoinedStr + FormattedValue
- List/Dict/Set/Tuple literals
- List/Dict/Set comprehensions
- Starred unpacking

**Statements:**
- Assign / AugAssign / AnnAssign
- Expr (bare function call)
- For (no while)
- If / elif / else
- Try / Except / Else / Finally
- Raise / Assert
- ClassDef (only BaseModel subclasses and @dataclass)
- Pass

**Excluded (matching paper):**
- import, def, lambda, while, with, yield, async, eval, exec, break, continue

### Function Call — Security-Critical Path

1. Evaluate callable expression
2. Evaluate positional and keyword args
3. Bind receiver for methods
4. **Security policy check**: `policy_engine.check_policy(tool_name, args_with_values, dependencies)`
5. If `Denied` and function is a tool (not builtin): raise `SecurityPolicyDeniedError`
6. Execute function, wrap return value with `Source::Tool(tool_name)`
7. Record `FunctionCall` in tool calls chain

### Built-in Functions

`print`, `len`, `range`, `sorted`, `reversed`, `enumerate`, `zip`, `map`, `filter`,
`min`, `max`, `sum`, `abs`, `round`, `isinstance`, `type`, `str`, `int`, `float`,
`bool`, `list`, `tuple`, `dict`, `set`, `hash`

All tagged `is_builtin = true`, bypass policy checks.

### Built-in Methods

Per-type methods matching the Python reference:
- `str`: `lower`, `upper`, `split`, `join`, `strip`, `lstrip`, `rstrip`, `replace`, `startswith`, `endswith`, `find`, `format`, `count`, `isdigit`, `isalpha`
- `list`: `index`, `count`
- `dict`: `keys`, `values`, `items`, `get`
- `set`: `add`, `union`, `intersection`, `difference`

### Special Built-in Functions

| Function | Side Effects | Policy Exempt | Description |
|---|---|---|---|
| `query_ai_assistant(query, schema)` | No | Yes | Calls FLC quarantined LLM for unstructured-to-structured parsing |
| `print(...)` | No | Yes | Accumulates output for display |
| `prompt_user(question)` | Yes (blocks) | Yes | Pauses for user input, returns `Source::User` value |

### Error Handling

`CamelException` contains the Python exception, AST node(s), and dependency chain. Untrusted
exception messages are redacted (matching `is_trusted` check in reference's `format_camel_exception`).
Errors are formatted as Python tracebacks and fed back to the privileged LLM for retry.

## Security Policy Engine

### Evaluation Flow

```
Tool call in interpreter
    |
    v
Tool in no_side_effect_tools? --yes--> Allowed
    |no
    v
Non-public values in accumulated dependencies? --yes--> Denied
    |no
    v
Find matching policy by tool name (glob patterns) --no match--> Denied (fail-closed)
    |match
    v
Evaluate policy predicate on argument capabilities
    |
    v
Allowed or Denied
```

### Policy Predicates

| Predicate | Check | Use Case |
|---|---|---|
| `all_args_public` | `get_all_readers(arg) == Public` for all args | Base policy from paper |
| `all_args_trusted` | `is_trusted(arg)` for all args | Stricter, blocks tool-sourced data |
| `no_check` | Always Allowed | Explicit opt-out |

### Per-Argument Source Constraints

```toml
[tools.send_message.args.channel]
must_be_from = ["user", "get_channels"]
```

Checks `get_all_sources(arg)` — every source in the transitive set must be in the allowed list.

### Policy Result

```rust
enum PolicyResult {
    Allowed,
    Denied { reason: String },
}
```

`Denied` raises `SecurityPolicyDeniedError` — a hard stop, not retried.

## Skill Configuration Format

A single TOML file declares everything the engine needs.

### Sections

#### `[skill]` — Metadata

```toml
[skill]
name = "plan-feature"
description = "Generate implementation plan from Jira feature"
max_retries = 10
system_prompt_preamble = "..."
```

#### `[llm]` — LLM Configuration

```toml
[llm.privileged]
config = "privileged.toml"   # FLC config for code-generating LLM

[llm.quarantined]
config = "quarantined.toml"  # FLC config for query_ai_assistant
```

#### `[tools.*]` — Tool Definitions

```toml
[tools.get_jira_issue]
description = "Get a Jira issue by key"
side_effects = false
trusted = false              # default; set true for trusted tool sources

[tools.get_jira_issue.params]
issue_key = { type = "str", description = "Issue key e.g. PROJ-42" }

[tools.get_jira_issue.returns]
type = "JiraIssue"

[tools.send_message]
description = "Send a message"
side_effects = true
policy = "all_args_public"

[tools.send_message.params]
channel = { type = "str" }
message = { type = "str" }

[tools.send_message.returns]
type = "str"

[tools.send_message.args.channel]
must_be_from = ["user", "get_channels"]
```

#### `[execution.*]` — Execution Backends

Four backend types:

**MCP (static server):**
```toml
[execution.get_jira_issue]
backend = "mcp"
server = "atlassian"
tool = "get_issue"
param_mapping = { issue_key = "issueIdOrKey" }
```

**MCP (dynamic server from parameter):**
```toml
[execution.find_symbol]
backend = "mcp"
server_from_param = "repo"
tool = "find_symbol"
exclude_params = ["repo"]
```

**HTTP:**
```toml
[execution.some_api]
backend = "http"
method = "POST"
url = "https://api.example.com/endpoint"
headers = { Authorization = "Bearer ${API_TOKEN}" }
body = { field = "{param_name}" }
```

**Filesystem:**
```toml
[execution.read_file]
backend = "filesystem"
operation = "read"
param_mapping = { path = "file_path" }
```

| Field | Description |
|---|---|
| `backend` | `mcp`, `http`, `filesystem` |
| `server` | Static MCP server name (resolved via `[servers]`) |
| `server_from_param` | Parameter whose value selects the MCP server (resolved via `[servers.repos]`) |
| `tool` | MCP tool name |
| `param_mapping` | Rename params between skill interface and backend |
| `exclude_params` | Params used for routing, not passed to backend |
| `method` | HTTP method |
| `url` | HTTP URL with `{param}` substitution |
| `headers` | HTTP headers with `${ENV_VAR}` substitution |
| `body` | HTTP body template |
| `operation` | Filesystem operation: `read`, `list`, `glob` |

#### MCP Server Connectivity

The engine connects to MCP servers that are already running and accessible. It does not
spawn or manage MCP server processes. The `[servers]` entries resolve to MCP server
identifiers (e.g., `mcp__atlassian`), and the engine communicates via JSON-RPC over stdio
to a multiplexing MCP client that routes to the appropriate server. In CLI mode, the engine
expects an MCP client proxy (or direct server stdio pipes) to be configured at startup.

#### `[servers]` — Server Registry

```toml
[servers]
atlassian = "mcp__atlassian"
plugin_figma = "mcp__plugin_figma_figma"

[servers.repos]
agent-sentinel = "mcp__serena-agent-sentinel"
symref = "mcp__serena-symref"
```

`[servers]` maps logical names to MCP server identifiers.
`[servers.repos]` maps `server_from_param` values to MCP server identifiers.

#### `[types.*]` — Type Definitions

```toml
[types.JiraIssue]
description = "A Jira issue"

[types.JiraIssue.fields]
key = { type = "str" }
summary = { type = "str" }
links = { type = "list[IssueLink]" }
```

Supported types: `str`, `int`, `float`, `bool`, `list[T]`, `dict[K, V]`, and references
to other `[types.*]` entries. These become `BaseModel` classes in the interpreter namespace
and Python `class` definitions in the system prompt.

### System Prompt Generation

The engine auto-generates the privileged LLM's system prompt from the skill config,
matching `system_prompt_generator.py` from the reference:

1. `system_prompt_preamble` (if provided)
2. Built-in types list
3. Built-in functions list
4. Built-in methods per type
5. Tool function signatures as Python `def` stubs with docstrings
6. Type definitions as Python `class` stubs
7. CaMeL coding restrictions (no import, no while, no def, use `query_ai_assistant` for parsing, etc.)

## Crate Structure

```
camel-engine/
+-- Cargo.toml                  # workspace root
+-- camel-core/
|   +-- Cargo.toml
|   +-- src/
|       +-- lib.rs              # public API
|       +-- value.rs            # CamelValue, Capabilities, Source, Readers
|       +-- interpreter.rs      # Python AST walker
|       +-- namespace.rs        # variable scope
|       +-- builtins.rs         # built-in functions and methods
|       +-- policy.rs           # SecurityPolicyEngine
|       +-- skill.rs            # SkillConfig, system prompt generation
|       +-- execution.rs        # MCP/HTTP/filesystem dispatch
|       +-- types.rs            # type definition parsing
|       +-- error.rs            # CamelException, formatting
+-- camel-cli/
|   +-- Cargo.toml
|   +-- src/
|       +-- main.rs             # CLI entry point
+-- tests/
    +-- interpreter_tests.rs
    +-- policy_tests.rs
    +-- skill_config_tests.rs
    +-- integration_tests.rs
```

### Dependencies

**camel-core:**
- `rustpython-parser` 0.4 — Python AST parser
- `fortified-llm-client` (git) — quarantined and privileged LLM invocation
- `serde`, `serde_json`, `toml` — serialization and config
- `reqwest` — HTTP execution backend
- `tokio` — async runtime (FLC is async)
- `anyhow`, `thiserror` — error handling

**camel-cli:**
- `camel-core` (path)
- `clap` — CLI argument parsing
- `tokio` — async runtime
- `anyhow` — error handling

### Public API

```rust
/// Run the full CaMeL pipeline: privileged LLM -> interpret -> retry on error.
pub async fn run_pipeline(
    config: &SkillConfig,
    user_query: &str,
) -> Result<PipelineOutput>;

/// Single-shot: interpret already-generated Python code.
pub fn interpret(
    code: &str,
    namespace: Namespace,
    eval_args: EvalArgs,
) -> EvalResult;

/// Load and validate a skill config from a TOML file.
pub fn load_skill_config(path: &Path) -> Result<SkillConfig>;
```

## Testing Strategy

### Layer 1: Value System & Capability Propagation

Unit tests for `Capabilities` metadata flow — source tagging, reader intersection,
transitive dependency propagation, `is_public`, `is_trusted`, circular dependency safety.

### Layer 2: Interpreter — Per AST Node

Each supported AST node type gets targeted tests. Python code strings passed to `interpret()`
with a test namespace, asserting on result value and its capabilities.

Coverage: literals, variables, assignment, binary/unary/boolean/compare ops, collections,
f-strings, control flow (if, for), comprehensions, try/except, class definitions, attribute
access, method calls, unpacking, assert, raise, and all forbidden constructs (import, while,
def, lambda, eval).

### Layer 3: Security Policy Enforcement

Tests with mock tools in the namespace verifying:
- Base policy predicates (all_args_public, all_args_trusted)
- Per-argument must_be_from constraints
- Fail-closed behavior (no matching policy)
- Dependency chain non-public blocking
- Prompt injection scenarios: injected text in tool response flows to side-effecting tool,
  injected text not used, injected text parsed by quarantined LLM

### Layer 4: Skill Config Loading

TOML parsing, validation (missing execution entries, invalid policies, unresolved type
references), server registry resolution, param mapping, system prompt generation.

### Layer 5: Integration Tests

End-to-end with mock backends (mockito for HTTP, mock MCP servers, temp dirs for filesystem).
Covers: read-write pipeline, query_ai_assistant with FLC mock, error retry loop, prompt_user,
MCP dispatch, dynamic server routing, simplified plan-feature scenario.

### Layer 6: Boundary Tests

Known prompt injection payloads in tool responses verified to be blocked by policy when
flowing to side-effecting tools: HTML comment injection, instruction overrides, unicode
smuggling, nested JSON injection, f-string injection.
