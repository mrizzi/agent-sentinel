use crate::claude::{resolve_session_dir, HookInput, HookOutput};
use crate::registry::{derive_prefix, ToolRegistry};
use anyhow::{Context, Result};
use fortified_llm_client::config_builder::ConfigBuilder;
use fortified_llm_client::load_config_file;
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
        return Err(anyhow::anyhow!(
            "Config file not found: {}",
            config_path.display()
        ));
    }

    // Derive prefix from tool_input
    let prefix_field = entry.prefix_from.as_deref().unwrap_or("issueIdOrKey");
    let issue_key = input
        .tool_input_field(prefix_field)
        .unwrap_or_else(|| "UNKNOWN".to_string());
    let prefix = derive_prefix(&issue_key);

    // Set CWD to security_dir so config-relative paths (schemas/, patterns/)
    // resolve correctly when FLC's load_config_file() reads them.
    // SIDE EFFECT: Changes process-wide CWD. Acceptable because each hook
    // invocation is a separate short-lived process and no subsequent code
    // depends on the original CWD. Thread-safe here because this runs before
    // the tokio runtime is created, and we use new_current_thread().
    std::env::set_current_dir(security_dir).context("Failed to set CWD to security_dir")?;

    let file_config = load_config_file(&config_path)
        .map_err(|e| anyhow::anyhow!("Failed to load FLC config: {e}"))?;

    // Build evaluation config
    let mut builder = ConfigBuilder::new()
        .user_prompt(tool_response)
        .merge_file_config(&file_config);

    // Resolve API key from environment variable if specified in config
    if let Some(ref key_name) = file_config.api_key_name {
        match std::env::var(key_name) {
            Ok(key_value) => {
                builder = builder.api_key(key_value);
            }
            Err(_) => {
                eprintln!("WARN: API key env var '{key_name}' not set");
            }
        }
    }

    let config = builder
        .build()
        .map_err(|e| anyhow::anyhow!("Failed to build FLC config: {e}"))?;

    // Create a single-threaded tokio runtime for the async evaluate() call.
    // Current-thread is sufficient — we only need one block_on() call.
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("Failed to create tokio runtime")?;

    let flc_result = rt.block_on(fortified_llm_client::evaluate(config));

    let flc_output = match flc_result {
        Err(cli_error) => {
            // Hard error — CliError.code() returns &'static str, inherently safe
            // for stdout. The Display impl logged below may contain untrusted
            // content (e.g. HTTP response body), but stderr does not cross the
            // Dual LLM boundary.
            eprintln!("WARN: FLC evaluation failed: {cli_error}");
            let error_code = cli_error.code();
            let output = HookOutput::extraction_failed(&input.tool_name, error_code);
            println!("{}", serde_json::to_string_pretty(&output)?);
            return Ok(());
        }
        Ok(output) => output,
    };

    if flc_output.status != "success" {
        // Soft error — CliOutput.error.code is a String (not &'static str like
        // CliError::code()), so it could theoretically contain arbitrary content.
        // Sanitize defensively to ensure only safe characters cross the boundary.
        let error_code = sanitize_error_code(flc_output.error_code());
        let output = HookOutput::extraction_failed(&input.tool_name, &error_code);
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    let flc_response = match flc_output.response {
        Some(resp) => resp,
        None => {
            eprintln!("WARN: FLC returned success but no response");
            let output = HookOutput::extraction_failed(&input.tool_name, "MISSING_RESPONSE");
            println!("{}", serde_json::to_string_pretty(&output)?);
            return Ok(());
        }
    };

    // Parse the extraction as a JSON object for symref
    let extraction: serde_json::Map<String, serde_json::Value> =
        match serde_json::from_value(flc_response.clone()) {
            Ok(map) => map,
            Err(e) => {
                // FLC response is not a JSON object — return extraction without refs
                eprintln!("WARN: FLC response is not a JSON object: {e:#}");
                let output = HookOutput::post_tool_use(flc_response);
                println!("{}", serde_json::to_string_pretty(&output)?);
                return Ok(());
            }
        };

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
            let output = HookOutput::post_tool_use(flc_response);
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
    }

    Ok(())
}

/// Sanitize an error code from FLC CliOutput for safe inclusion in hook output.
///
/// The code comes from `CliOutput.error_code()` which returns `Option<&str>`
/// borrowed from the underlying `ErrorInfo.code: String`. Unlike `CliError::code()`
/// which returns `&'static str` (compile-time constant, inherently safe), this is
/// a runtime String that could theoretically contain arbitrary content. We sanitize
/// defensively: only alphanumeric + underscore characters are allowed.
fn sanitize_error_code(code: Option<&str>) -> String {
    let code = match code {
        Some(c) => c.to_string(),
        None => {
            eprintln!("WARN: FLC returned error status without error code");
            return "UNKNOWN_ERROR".to_string();
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
