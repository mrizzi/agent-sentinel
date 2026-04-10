use crate::claude::{HookInput, HookOutput};
use crate::process::{find_binary, run_process};
use crate::registry::ToolRegistry;
use anyhow::Result;
use std::path::Path;

pub fn run(security_dir: &Path) -> Result<()> {
    let input = HookInput::from_stdin()?;

    let registry = ToolRegistry::load(security_dir)?;
    if !registry.is_pre_tool_use_intercepted(&input.tool_name) {
        return Ok(()); // Passthrough
    }

    // Check prerequisites — graceful passthrough if not available
    let session_dir = match std::env::var("SDLC_SESSION_DIR") {
        Ok(dir) => dir,
        Err(_) => {
            eprintln!("WARN: SDLC_SESSION_DIR not set. Passthrough without dereferencing.");
            return Ok(());
        }
    };

    let vars_path = Path::new(&session_dir).join("vars.json");
    if !vars_path.exists() {
        eprintln!("WARN: No vars.json in session dir. Passthrough without dereferencing.");
        return Ok(());
    }

    let symref_bin = match find_binary("symref", "SYMREF_BIN") {
        Ok(bin) => bin,
        Err(_) => {
            eprintln!("WARN: symref not found. Passthrough without dereferencing.");
            return Ok(());
        }
    };

    let tool_input = input.tool_input
        .as_ref()
        .map(|v| v.to_string())
        .unwrap_or_else(|| "{}".to_string());

    let deref_output = run_process(
        &symref_bin,
        &["deref", "--session", &session_dir],
        Some(&tool_input),
    )?;

    if deref_output.exit_code != 0 {
        eprintln!("WARN: symref deref failed (exit {}). Passthrough.", deref_output.exit_code);
        return Ok(());
    }

    let updated_input: serde_json::Value = serde_json::from_str(&deref_output.stdout)
        .unwrap_or_else(|_| input.tool_input.unwrap_or(serde_json::json!({})));

    let output = HookOutput::pre_tool_use(updated_input);
    println!("{}", serde_json::to_string_pretty(&output)?);

    Ok(())
}
