use crate::claude::{resolve_session_dir, HookInput};
use anyhow::Result;
use std::path::Path;

pub fn run(_security_dir: &Path) -> Result<()> {
    let input = HookInput::from_stdin()?;

    let session_dir = match resolve_session_dir() {
        Some(dir) => dir,
        None => {
            eprintln!("WARN: AGENT_SENTINEL_SESSION_DIR not set. Skipping transcript collection.");
            return Ok(());
        }
    };

    if let Some(transcript_path) = &input.transcript_path {
        let src = Path::new(transcript_path);
        if src.exists() {
            let dest = Path::new(&session_dir).join("transcript.jsonl");
            std::fs::copy(src, dest)?;
        } else {
            eprintln!("WARN: Transcript not found at '{transcript_path}'. Skipping.");
        }
    }

    Ok(())
}
