use crate::claude::HookInput;
use anyhow::Result;
use std::path::Path;

pub fn run(_security_dir: &Path) -> Result<()> {
    let input = HookInput::from_stdin()?;

    let session_dir = match std::env::var("SDLC_SESSION_DIR") {
        Ok(dir) => dir,
        Err(_) => {
            eprintln!("WARN: SDLC_SESSION_DIR not set. Skipping transcript collection.");
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
