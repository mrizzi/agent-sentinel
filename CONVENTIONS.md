# Coding Conventions

## Language and Framework

- **Language**: Rust (2021 edition, MSRV 1.86)
- **Build tool**: Cargo
- **CLI**: Clap with derive macros
- **Async**: Tokio (current-thread runtime, used only in `post_tool_use`)
- **Serialization**: Serde + serde_json
- **Error handling**: Anyhow
- **Compiled-in libraries**: fortified-llm-client (LLM quarantine), symref (symbolic variables), chrono (timestamps)

## Code Style

- Run `cargo fmt` before committing
- Run `cargo clippy -- -D warnings` — zero warnings policy
- No custom `.rustfmt.toml` — use rustfmt defaults
- Line wrapping follows rustfmt defaults (~100 chars)

## Naming Conventions

- **Structs/Enums**: PascalCase (`ToolRegistry`, `HookInput`, `HookOutput`)
- **Functions**: snake_case (`derive_prefix`, `lookup_post_tool_use`, `resolve_session_dir`)
- **Modules/Files**: snake_case (`post_tool_use.rs`, `session_start.rs`)
- **Fields**: snake_case (`hook_event_name`, `tool_response`), with serde rename for camelCase wire format
- **Constants**: UPPER_SNAKE_CASE (`EXTRACTION_JSON`, `CLEAN_EXTRACTION`)

## File Organization

- `src/main.rs` — CLI entry point with Clap subcommands
- `src/claude.rs` — Hook I/O types (`HookInput`, `HookOutput`) and session utilities
- `src/registry.rs` — Tool registry parsing and lookup
- `src/hooks/` — One file per hook event (`session_start.rs`, `pre_tool_use.rs`, `post_tool_use.rs`, `session_end.rs`)
- `tests/` — Integration tests, one file per hook (`post_tool_use_test.rs`, `boundary_test.rs`, etc.)
- `tests/fixtures/` — JSON fixtures for tests
- `profiles/` — Example Claude Code hook profiles
- `skills/` — Claude Code skills (plugin components)
- `docs/` — Design specs, constraints

New hooks go in `src/hooks/` with a corresponding test in `tests/`.

## Error Handling

- Use `anyhow::Result<T>` for all fallible functions
- Chain context with `.context("message")` or `.with_context(|| format!(...))`
- Propagate errors with `?` operator
- Hook entry points: `pub fn run(security_dir: &Path) -> Result<()>`
- Exit codes: `ExitCode::SUCCESS` (0) for success, `ExitCode::from(2)` for errors
- Security errors: sanitize before crossing the Dual LLM boundary (see `sanitize_error_code()`)

## Testing Conventions

- **Unit tests**: `#[cfg(test)] mod tests` within source files
- **Integration tests**: `tests/{hook_name}_test.rs`
- **Boundary tests**: `tests/boundary_test.rs` — verifies injection payloads cannot cross the Dual LLM boundary
- **Fixtures**: `tests/fixtures/` for JSON test data
- **Test deps**: `assert_cmd` (subprocess), `mockito` (HTTP mocks), `predicates` (assertions), `tempfile` (temp dirs)
- **Pattern**: Create temp dirs, set up registry + config, spawn `agent-sentinel` via `Command::cargo_bin`, assert on stdout JSON
- **CI**: Tests run on ubuntu + macOS, stable + beta toolchains

## Commit Messages

Follow Conventional Commits: `<type>: <description>`

Types used in this project:
- `feat` — new feature
- `fix` — bug fix
- `refactor` — code restructuring without behavior change
- `test` — test additions or changes
- `build` — dependency or build changes
- `docs` — documentation
- `chore` — maintenance tasks
- `style` — formatting (cargo fmt)

## Shared Modules and Reuse

- `src/claude.rs` — All hook I/O types (`HookInput`, `HookOutput`, `HookSpecificOutput`) and session utilities (`resolve_session_dir`, `sessions_base_dir`). Every hook uses these.
- `src/registry.rs` — `ToolRegistry` with `load()`, `lookup_post_tool_use()`, `lookup_pre_tool_use()`, `derive_prefix()`. Shared across post and pre tool use hooks.
- Hook signature convention: `pub fn run(security_dir: &Path) -> Result<()>` — all hooks follow this pattern.

## Documentation

- `README.md` — Project overview, architecture, setup guide, testing
- `CLAUDE.md` — Project configuration (Jira, repository registry, code intelligence)
- `CONVENTIONS.md` — This file
- `docs/constraints.md` — Architectural constraints for SDLC workflow skills
- `docs/superpowers/specs/` — Design specs for features
- Code comments: security-critical decisions documented inline (boundary safety, side effects, error sanitization)

## Dependencies

- **MSRV**: Rust 1.86 (enforced in CI)
- **Git deps pinned to rev**: `symref` (`rev = "e953b1b"`), `fortified-llm-client` (`rev = "a6da4cc"`)
- **License policy** (`deny.toml`): Only permissive licenses allowed (MIT, Apache-2.0, BSD, ISC, Zlib, Unicode)
- **Source policy** (`deny.toml`): Only crates.io + two allow-listed git repos
- **Release profile**: `strip = true`, `lto = true`, `codegen-units = 1`
- Run `cargo deny check` before merging
