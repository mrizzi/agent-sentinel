use anyhow::{bail, Context, Result};
use std::process::{Command, Stdio};

pub struct ProcessOutput {
    pub exit_code: i32,
    pub stdout: String,
    #[allow(dead_code)] // Available for error diagnostics
    pub stderr: String,
}

/// Find a binary: check env var override, then PATH
pub fn find_binary(name: &str, env_var: &str) -> Result<String> {
    // Check environment variable override first
    if let Ok(path) = std::env::var(env_var) {
        if std::path::Path::new(&path).is_file() {
            return Ok(path);
        }
    }

    // Check PATH
    if let Ok(output) = Command::new("which").arg(name).output() {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                return Ok(path);
            }
        }
    }

    bail!("{name} not found. Set {env_var} or add to PATH")
}

/// Run a subprocess and capture output
pub fn run_process(binary: &str, args: &[&str], stdin_data: Option<&str>) -> Result<ProcessOutput> {
    run_process_in(binary, args, stdin_data, None)
}

/// Run a subprocess in a specific working directory and capture output
pub fn run_process_in(
    binary: &str,
    args: &[&str],
    stdin_data: Option<&str>,
    cwd: Option<&std::path::Path>,
) -> Result<ProcessOutput> {
    let mut cmd = Command::new(binary);
    cmd.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());

    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }

    if stdin_data.is_some() {
        cmd.stdin(Stdio::piped());
    }

    let mut child = cmd
        .spawn()
        .with_context(|| format!("Failed to spawn {binary}"))?;

    if let Some(data) = stdin_data {
        use std::io::Write;
        if let Some(ref mut stdin) = child.stdin {
            stdin
                .write_all(data.as_bytes())
                .with_context(|| format!("Failed to write stdin to {binary}"))?;
        }
        // Drop stdin to signal EOF
        child.stdin.take();
    }

    let output = child
        .wait_with_output()
        .with_context(|| format!("Failed to wait for {binary}"))?;

    Ok(ProcessOutput {
        exit_code: output.status.code().unwrap_or(1),
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_binary_from_path() {
        // "echo" should always be on PATH
        let result = find_binary("echo", "NONEXISTENT_ENV_VAR");
        assert!(result.is_ok());
    }

    #[test]
    fn test_find_binary_missing() {
        let result = find_binary("nonexistent_binary_xyz", "NONEXISTENT_ENV_VAR");
        assert!(result.is_err());
    }

    #[test]
    fn test_run_process_success() {
        let output = run_process("echo", &["hello"], None).unwrap();
        assert_eq!(output.exit_code, 0);
        assert_eq!(output.stdout.trim(), "hello");
    }

    #[test]
    fn test_run_process_with_stdin() {
        let output = run_process("cat", &[], Some("test input")).unwrap();
        assert_eq!(output.exit_code, 0);
        assert_eq!(output.stdout.trim(), "test input");
    }

    #[test]
    fn test_run_process_failure() {
        let output = run_process("false", &[], None).unwrap();
        assert_ne!(output.exit_code, 0);
    }

    #[test]
    fn test_run_process_in_with_cwd() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("testfile.txt"), "hello from cwd").unwrap();
        let output = run_process_in("cat", &["testfile.txt"], None, Some(tmp.path())).unwrap();
        assert_eq!(output.exit_code, 0);
        assert_eq!(output.stdout.trim(), "hello from cwd");
    }
}
