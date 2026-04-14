# Design: Use symref as a Library Instead of External Binary

**Date:** 2026-04-13
**Status:** Draft

## Problem

agent-sentinel invokes symref as an external subprocess for variable storage (`symref store`) and dereferencing (`symref deref`). This requires:

- symref installed on PATH or configured via `SYMREF_BIN` env var
- Subprocess spawn overhead on every hook invocation (two per tool call)
- JSON serialization/deserialization at the stdin/stdout boundary
- Exit code + stderr parsing for error handling

Both projects are Rust, same author, same trust domain. The subprocess boundary adds complexity without meaningful isolation benefits.

## Goal

Make agent-sentinel a single self-contained binary that does not require symref installed on PATH. Use symref as a Rust library dependency instead of shelling out to it.

## Approach

Split symref into a library-first crate with a thin CLI wrapper. Agent-sentinel depends on the library directly.

### symref Library API

Refactor symref's `store::run()` and `deref::run()` into two layers:

**Library functions (`lib.rs`):**

```rust
// symref::store
pub fn store(session: &Path, prefix: &str, input: &Map<String, Value>) -> Result<StoreOutput>

// symref::deref
pub fn deref(session: &Path, input: &Value) -> Result<Value>
```

- `store()` takes a parsed JSON object, assigns refs, merges into `vars.json`, returns `StoreOutput` directly (no stdout printing).
- `deref()` takes a parsed JSON value, loads `vars.json`, substitutes variables, returns the resolved value directly.
- Both still read/write `vars.json` on disk (persistent session state).
- Types (`VarStore`, `VarRef`, `StoreOutput`) are re-exported from `lib.rs`.

**CLI wrapper (`main.rs`):**

Stays as a thin wrapper: parses CLI args with clap, reads stdin/file into JSON, calls the library function, prints the result to stdout. The `clap` dependency moves to a `[bin]`-only concern.

**Unchanged:** `naming.rs` module, `vars.json` format, variable naming conventions.

### Agent-sentinel Integration

**`Cargo.toml`:**

```toml
symref = { git = "https://github.com/mrizzi/symref.git" }
```

No new transitive dependencies except `subst` (small). `anyhow`, `serde`, `serde_json` are already in agent-sentinel.

**`pre_tool_use.rs` changes:**

Replace subprocess call with direct library call:

```rust
// Before:
let symref_bin = find_binary("symref", "SYMREF_BIN")?;
let deref_output = run_process(&symref_bin, &["deref", "--session", &session_dir], Some(&tool_input))?;
let updated_input: Value = serde_json::from_str(&deref_output.stdout)...

// After:
let tool_input_value: Value = input.tool_input.unwrap_or(json!({}));
let updated_input = symref::deref(Path::new(&session_dir), &tool_input_value)?;
```

- Remove `find_binary("symref", "SYMREF_BIN")`.
- Remove exit code checking — errors come back as `Result::Err`.
- Graceful passthrough logic stays (no session dir, no vars.json -> passthrough).
- The "symref not found" fallback disappears since it's compiled in.

**`post_tool_use.rs` changes:**

```rust
// Before:
let symref_bin = find_binary("symref", "SYMREF_BIN")?;
let symref_output = run_process(&symref_bin, &["store", "--session", &session_dir, "--prefix", &prefix], Some(&extraction))?;
let symref_response: Value = serde_json::from_str(&symref_output.stdout)...

// After:
let extraction: Map<String, Value> = serde_json::from_value(flc_response["response"].clone())?;
let store_output = symref::store(Path::new(&session_dir), &prefix, &extraction)?;
```

- `StoreOutput` comes back as a typed struct — no JSON parsing of stdout.
- Error handling for "symref store failed" becomes a normal `Result` match.

**`process.rs`:** Stays for `fortified-llm-client`. Only symref-specific binary discovery (`SYMREF_BIN`) is removed.

### Error Handling

**`pre_tool_use` — graceful passthrough (unchanged behavior):**
- No session dir -> passthrough, warn on stderr.
- No `vars.json` -> passthrough, warn on stderr.
- `symref::deref()` returns `Err` -> passthrough with original input.

**`post_tool_use` — fail-closed with fallback (unchanged behavior):**
- `symref::store()` returns `Err` -> fall back to returning FLC extraction without refs.

**What improves:** Error types become more precise. Instead of "exit code != 0" + parsing stderr strings, you get `anyhow::Error` with proper context chains.

**What disappears:**
- The "symref not found" error path in `pre_tool_use` (compiled in, always available).
- The `SYMREF_BIN` environment variable.

### Testing

**In symref:**
- Existing unit tests in `store.rs`, `deref.rs`, `naming.rs` continue to work as-is.
- Integration tests using file I/O keep working since library functions still read/write `vars.json`.

**In agent-sentinel:**
- Tests that mock symref as a subprocess change to call library functions directly.
- Simpler: no need to create mock binaries or set `SYMREF_BIN` to a fake path.
- Tests set up a temp session dir with `vars.json`, call the hook, assert on output.

**Dropped:**
- Tests for symref binary discovery.
- `SYMREF_BIN` env var tests.

## Scope Boundary

**In scope:**
- Add `lib.rs` to symref, refactor `store::run()` and `deref::run()` into library functions + CLI wrappers.
- Add symref as a git dependency in agent-sentinel's `Cargo.toml`.
- Replace subprocess calls in `pre_tool_use.rs` and `post_tool_use.rs` with direct library calls.
- Remove symref-specific binary discovery (`SYMREF_BIN` env var handling).
- Update affected tests in both projects.

**Out of scope:**
- Changing `fortified-llm-client` from binary to library.
- Changing `vars.json` format or variable naming conventions.
- Changing the `process.rs` module beyond removing symref usage.
- Publishing symref to crates.io.
- Changes to the tool registry, session management, or hook output format.

## Dependency Strategy

Git dependency now, crates.io later:

```toml
# Now:
symref = { git = "https://github.com/mrizzi/symref.git" }

# Later:
symref = "0.1"
```
