use crate::core::config::Config;
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::Duration;

use super::safety::{confirm_action, is_dangerous_shell_command};

#[derive(Debug, thiserror::Error)]
pub enum ShellExecError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Action cancelled by user: {0}")]
    Cancelled(String),
}

#[derive(Deserialize, Serialize)]
pub struct ShellExecArgs {
    pub command: String,
    #[serde(default)]
    pub timeout_secs: Option<u64>,
    #[serde(default)]
    pub cwd: Option<String>,
    /// If true, skip the safety confirmation prompt for dangerous commands.
    /// Should only be set by the user, never by the agent.
    #[serde(default)]
    pub auto_approve: bool,
}

#[derive(Deserialize, Serialize)]
pub struct ShellExecOutput {
    pub command: String,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
}

#[derive(Debug, Clone)]
pub struct ShellExec {
    /// Default command timeout in seconds (from config).
    default_timeout_secs: u64,
}

impl Default for ShellExec {
    fn default() -> Self {
        Self {
            default_timeout_secs: 30,
        }
    }
}

impl ShellExec {
    /// Creates a `ShellExec` with config-specified defaults.
    pub fn from_config(config: &Config) -> Self {
        Self {
            default_timeout_secs: config.shell.default_timeout_secs,
        }
    }
}

impl Tool for ShellExec {
    const NAME: &'static str = "shell_exec";
    type Error = ShellExecError;
    type Args = ShellExecArgs;
    type Output = ShellExecOutput;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Execute a shell command and return its output. \
                Use this to run build commands, tests, linters, and other CLI tools. \
                Commands run in bash. Output is truncated if too long."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The shell command to execute"
                    },
                    "timeout_secs": {
                        "type": "integer",
                        "description": "Maximum execution time in seconds. Defaults to the value in config.toml (default: 30)."
                    },
                    "cwd": {
                        "type": "string",
                        "description": "Working directory for the command. Default: current directory."
                    },
                    "auto_approve": {
                        "type": "boolean",
                        "description": "If true, skip the safety confirmation for dangerous commands. Only set this if you are confident the command is safe. Default: false."
                    }
                },
                "required": ["command"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        // Safety check: prompt for confirmation if the command matches a dangerous pattern
        if !args.auto_approve
            && let Some(pattern) = is_dangerous_shell_command(&args.command)
        {
            let approved = confirm_action(
                "This command matches a dangerous pattern:",
                &format!("matched '{}' in: {}", pattern, args.command),
            )
            .await;
            if !approved {
                return Err(ShellExecError::Cancelled(args.command));
            }
        }

        let timeout = Duration::from_secs(args.timeout_secs.unwrap_or(self.default_timeout_secs));

        let mut cmd = tokio::process::Command::new("bash");
        cmd.arg("-c").arg(&args.command);

        if let Some(cwd) = &args.cwd {
            cmd.current_dir(cwd);
        }

        cmd.stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        let result = tokio::time::timeout(timeout, cmd.output()).await;

        match result {
            Ok(Ok(output)) => {
                let stdout = truncate_string(&String::from_utf8_lossy(&output.stdout), 10_000);
                let stderr = truncate_string(&String::from_utf8_lossy(&output.stderr), 5_000);

                Ok(ShellExecOutput {
                    command: args.command,
                    exit_code: output.status.code(),
                    stdout,
                    stderr,
                    timed_out: false,
                })
            }
            Ok(Err(e)) => Err(ShellExecError::Io(e)),
            Err(_) => Ok(ShellExecOutput {
                command: args.command,
                exit_code: None,
                stdout: String::new(),
                stderr: format!("Command timed out after {:?}", timeout),
                timed_out: true,
            }),
        }
    }
}

fn truncate_string(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        // Find the nearest valid char boundary at or before max_len
        let boundary = s.floor_char_boundary(max_len);
        let truncated = &s[..boundary];
        format!(
            "{}\n\n... [output truncated, {} chars remaining]",
            truncated,
            s.len() - boundary
        )
    }
}
