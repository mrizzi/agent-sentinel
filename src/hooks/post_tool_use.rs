use crate::claude::{resolve_session_dir, HookInput, HookOutput};
use crate::process::{find_binary, run_process_in};
use crate::registry::{derive_prefix, ToolRegistry};
use anyhow::{bail, Context, Result};
use std::path::Path;

pub fn run(security_dir: &Path) -> Result<()> {
    let input = HookInput::from_stdin()?;

    // Load registry and check if this tool is intercepted
    let registry = ToolRegistry::load(security_dir)?;
    let entry = match registry.lookup_post_tool_use(&input.tool_name) {
        Some(entry) => entry.clone(),
        None => return Ok(()), // Passthrough — exit 0, no output
    };

    // Fail closed: session dir required for intercepted tools
    let session_dir = resolve_session_dir()
        .context("AGENT_SENTINEL_SESSION_DIR not set. Cannot quarantine without session.")?;

    let tool_response = input
        .tool_response
        .as_ref()
        .map(|v| serde_json::to_string(v).unwrap_or_else(|_| v.to_string()))
        .context("Empty tool_response")?;

    // Resolve config path
    let config_path = security_dir.join(&entry.config);
    if !config_path.exists() {
        bail!("Config file not found: {}", config_path.display());
    }

    // Resolve fortified-llm-client binary (still a subprocess)
    let flc_bin = find_binary("fortified-llm-client", "FORTIFIED_LLM_CLIENT_BIN")?;

    // Derive prefix from tool_input
    let prefix_field = entry.prefix_from.as_deref().unwrap_or("issueIdOrKey");
    let issue_key = input
        .tool_input_field(prefix_field)
        .unwrap_or_else(|| "UNKNOWN".to_string());
    let prefix = derive_prefix(&issue_key);

    // Write tool_response to temp file (avoid ARG_MAX)
    let temp_file = tempfile::NamedTempFile::new().context("Failed to create temp file")?;
    std::fs::write(temp_file.path(), tool_response)?;

    // Invoke fortified-llm-client from security_dir so config-relative
    // paths (schemas/, patterns/) resolve correctly
    let flc_output = run_process_in(
        &flc_bin,
        &[
            "--config-file",
            config_path.to_str().unwrap(),
            "--user-file",
            temp_file.path().to_str().unwrap(),
            "--quiet",
        ],
        None,
        Some(security_dir),
    )?;

    if flc_output.exit_code != 0 {
        // Extraction failed — return safe error code only (see parse_flc_error_code)
        if !flc_output.stderr.is_empty() {
            eprintln!("WARN: FLC stderr: {}", flc_output.stderr.trim());
        }
        let error_code = parse_flc_error_code(&flc_output.stdout);
        let output = HookOutput::extraction_failed(&input.tool_name, &error_code);
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    // Parse fortified-llm-client response
    let flc_response: serde_json::Value = serde_json::from_str(&flc_output.stdout)
        .context("Failed to parse fortified-llm-client output")?;

    if flc_response["status"] != "success" {
        let error_code = parse_flc_error_code(&flc_output.stdout);
        let output = HookOutput::extraction_failed(&input.tool_name, &error_code);
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    // Parse the extraction as a JSON object for symref
    let extraction: serde_json::Map<String, serde_json::Value> =
        serde_json::from_value(flc_response["response"].clone())
            .context("FLC response is not a JSON object")?;

    let session_path = Path::new(&session_dir);

    // Invoke symref store as library call
    match symref::store(session_path, &prefix, &extraction) {
        Ok(store_output) => {
            let output = HookOutput::post_tool_use(serde_json::json!({
                "issue_key": issue_key,
                "refs": store_output.refs
            }));
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
        Err(e) => {
            // symref failed — return extraction without refs
            eprintln!("WARN: symref store failed: {e:#}");
            let output = HookOutput::post_tool_use(flc_response["response"].clone());
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
    }

    Ok(())
}

/// Extract only the error CODE from FLC output — never the message.
/// The message may echo untrusted input (e.g., schema validation errors
/// include the invalid value), which would breach the Dual LLM boundary.
/// The code is sanitized to alphanumeric + underscore to prevent injection
/// via crafted error codes.
fn parse_flc_error_code(stdout: &str) -> String {
    let code = match serde_json::from_str::<serde_json::Value>(stdout) {
        Ok(v) => v["error"]["code"]
            .as_str()
            .map(|s| s.to_string())
            .unwrap_or_else(|| {
                eprintln!("WARN: FLC returned JSON without error.code field");
                "UNKNOWN_ERROR".to_string()
            }),
        Err(e) => {
            eprintln!("WARN: FLC output is not valid JSON: {e}");
            "UNKNOWN_ERROR".to_string()
        }
    };

    // Sanitize: only allow alphanumeric and underscores to prevent
    // injection via crafted error codes crossing the boundary
    if code.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        code
    } else {
        eprintln!("WARN: FLC error code contains unexpected characters, sanitizing");
        "INVALID_ERROR_CODE".to_string()
    }
}
