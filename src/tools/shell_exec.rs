use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::convert::Infallible;
use std::time::Duration;

#[derive(Deserialize, Serialize)]
pub struct ShellExecArgs {
    pub command: String,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    #[serde(default)]
    pub cwd: Option<String>,
}

fn default_timeout() -> u64 {
    30
}

#[derive(Deserialize, Serialize)]
pub struct ShellExecOutput {
    pub command: String,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ShellExec;

impl Tool for ShellExec {
    const NAME: &'static str = "shell_exec";
    type Error = Infallible;
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
                        "description": "Maximum execution time in seconds. Default: 30."
                    },
                    "cwd": {
                        "type": "string",
                        "description": "Working directory for the command. Default: current directory."
                    }
                },
                "required": ["command"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let timeout = Duration::from_secs(args.timeout_secs);

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
            Ok(Err(e)) => Ok(ShellExecOutput {
                command: args.command,
                exit_code: None,
                stdout: String::new(),
                stderr: format!("Failed to execute: {}", e),
                timed_out: false,
            }),
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
        let truncated = &s[..max_len];
        format!(
            "{}\n\n... [output truncated, {} chars remaining]",
            truncated,
            s.len() - max_len
        )
    }
}
