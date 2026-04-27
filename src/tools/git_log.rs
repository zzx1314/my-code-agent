use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum GitLogError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Git error: {0}")]
    Git(String),
}

#[derive(Deserialize, Serialize)]
pub struct GitLogArgs {
    /// Number of commits to show. Default: 10.
    #[serde(default)]
    pub max_count: Option<usize>,
    /// Working directory of the git repository. Default: current directory.
    #[serde(default)]
    pub cwd: Option<String>,
    /// Show only commits that affect this file.
    #[serde(default)]
    pub file: Option<String>,
    /// Pretty format: "oneline", "short", "medium", "full". Default: "oneline".
    #[serde(default)]
    pub format: Option<String>,
}

#[derive(Deserialize, Serialize)]
pub struct CommitInfo {
    pub hash: String,
    pub short_hash: String,
    pub author: String,
    pub date: String,
    pub message: String,
}

#[derive(Deserialize, Serialize)]
pub struct GitLogOutput {
    pub commits: Vec<CommitInfo>,
    pub total_shown: usize,
}

#[derive(Debug, Clone)]
pub struct GitLog;

impl GitLog {
    const DEFAULT_MAX_COUNT: usize = 10;
}

impl Tool for GitLog {
    const NAME: &'static str = "git_log";
    type Error = GitLogError;
    type Args = GitLogArgs;
    type Output = GitLogOutput;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Show git commit history in a structured format. \
                Returns a list of commits with hash, author, date, and message. \
                Use this instead of `shell_exec` with `git log` for better structured output."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "max_count": {
                        "type": "integer",
                        "description": "Number of commits to show. Default: 10."
                    },
                    "cwd": {
                        "type": "string",
                        "description": "Working directory of the git repository. Default: current directory."
                    },
                    "file": {
                        "type": "string",
                        "description": "Show only commits that affect this file."
                    },
                    "format": {
                        "type": "string",
                        "description": "Pretty format: 'oneline', 'short', 'medium', 'full'. Default: 'oneline'."
                    }
                }
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let max_count = args.max_count.unwrap_or(Self::DEFAULT_MAX_COUNT);
        let cwd = args.cwd.as_deref();
        let format = args.format.as_deref().unwrap_or("oneline");

        let mut cmd = tokio::process::Command::new("git");
        cmd.arg("log")
            .arg(&format!("--max-count={}", max_count))
            .arg(&format!("--pretty=format:{}", match format {
                "oneline" => "%H|%h|%an|%ar|%s",
                "short" => "%H|%h|%an|%ar|%s",
                "medium" => "%H|%h|%an|%ai|%s%n%b",
                "full" => "%H|%h|%an|%ae|%ai|%cn|%ce|%s%n%b",
                _ => "%H|%h|%an|%ar|%s",
            }));

        if let Some(file) = &args.file {
            cmd.arg("--").arg(file);
        }

        if let Some(cwd) = cwd {
            cmd.current_dir(cwd);
        }

        cmd.stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        let output = cmd.output().await.map_err(GitLogError::Io)?;

        if !output.status.success() {
            return Err(GitLogError::Git(
                String::from_utf8_lossy(&output.stderr).to_string(),
            ));
        }

        let log_output = String::from_utf8_lossy(&output.stdout).to_string();
        let mut commits = Vec::new();

        for line in log_output.lines() {
            if line.trim().is_empty() {
                continue;
            }

            let parts: Vec<&str> = line.splitn(5, '|').collect();
            if parts.len() >= 5 {
                commits.push(CommitInfo {
                    hash: parts[0].to_string(),
                    short_hash: parts[1].to_string(),
                    author: parts[2].to_string(),
                    date: parts[3].to_string(),
                    message: parts[4].to_string(),
                });
            } else if parts.len() >= 3 {
                // Fallback for simpler formats
                commits.push(CommitInfo {
                    hash: parts[0].to_string(),
                    short_hash: parts[0][..8.min(parts[0].len())].to_string(),
                    author: parts.get(1).unwrap_or(&"").to_string(),
                    date: parts.get(2).unwrap_or(&"").to_string(),
                    message: parts.get(3).unwrap_or(&"").to_string(),
                });
            }
        }

        let total = commits.len();
        Ok(GitLogOutput {
            commits,
            total_shown: total,
        })
    }
}
