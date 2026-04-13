use crate::claude::{resolve_session_dir, HookInput, HookOutput};
use crate::process::{find_binary, run_process, run_process_in};
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

    // Resolve binaries
    let flc_bin = find_binary("fortified-llm-client", "FORTIFIED_LLM_CLIENT_BIN")?;
    let symref_bin = find_binary("symref", "SYMREF_BIN")?;

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
        // Extraction failed — return error via updatedMCPToolOutput
        let detail = parse_flc_error(&flc_output.stdout);
        let output = HookOutput::extraction_failed(&input.tool_name, &detail);
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    // Parse fortified-llm-client response
    let flc_response: serde_json::Value = serde_json::from_str(&flc_output.stdout)
        .context("Failed to parse fortified-llm-client output")?;

    if flc_response["status"] != "success" {
        let detail = flc_response["error"]["message"]
            .as_str()
            .unwrap_or("Non-success status");
        let output = HookOutput::extraction_failed(&input.tool_name, detail);
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    let extraction = flc_response["response"].to_string();

    // Invoke symref store
    let symref_output = run_process(
        &symref_bin,
        &["store", "--session", &session_dir, "--prefix", &prefix],
        Some(&extraction),
    )?;

    if symref_output.exit_code != 0 {
        // symref failed — return extraction without refs
        eprintln!(
            "WARN: symref store failed (exit {})",
            symref_output.exit_code
        );
        let output = HookOutput::post_tool_use(flc_response["response"].clone());
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    // Parse symref output and return refs
    let symref_response: serde_json::Value =
        serde_json::from_str(&symref_output.stdout).context("Failed to parse symref output")?;

    let output = HookOutput::post_tool_use(serde_json::json!({
        "issue_key": issue_key,
        "refs": symref_response["refs"]
    }));
    println!("{}", serde_json::to_string_pretty(&output)?);

    Ok(())
}

fn parse_flc_error(stdout: &str) -> String {
    serde_json::from_str::<serde_json::Value>(stdout)
        .ok()
        .and_then(|v| v["error"]["message"].as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "Unknown error".to_string())
}
