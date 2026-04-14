use crate::claude::{resolve_session_dir, HookInput, HookOutput};
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
    let session_dir = match resolve_session_dir() {
        Some(dir) => dir,
        None => {
            eprintln!(
                "WARN: AGENT_SENTINEL_SESSION_DIR not set. Passthrough without dereferencing."
            );
            return Ok(());
        }
    };

    let session_path = Path::new(&session_dir);
    let vars_path = session_path.join("vars.json");
    if !vars_path.exists() {
        eprintln!("WARN: No vars.json in session dir. Passthrough without dereferencing.");
        return Ok(());
    }

    let tool_input = input.tool_input.unwrap_or(serde_json::json!({}));

    let updated_input = match symref::deref(session_path, &tool_input) {
        Ok(value) => value,
        Err(e) => {
            eprintln!("WARN: symref deref failed: {e:#}. Passthrough.");
            return Ok(());
        }
    };

    let output = HookOutput::pre_tool_use(updated_input);
    println!("{}", serde_json::to_string_pretty(&output)?);

    Ok(())
}
