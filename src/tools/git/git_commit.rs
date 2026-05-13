use crate::core::types::ToolDefinition;
use crate::tools::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::tools::exec::confirmation::ConfirmationHandle;
use crate::tools::exec::safety::confirm_action;

#[derive(Debug, thiserror::Error)]
pub enum GitCommitError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Git error: {0}")]
    Git(String),
    #[error("Action cancelled by user: {0}")]
    Cancelled(String),
    #[error("No changes staged for commit")]
    NothingToCommit,
}

#[derive(Deserialize, Serialize)]
pub struct GitCommitArgs {
    /// Commit message.
    pub message: String,
    /// Automatically stage all modified and deleted files before committing (like `git commit -a`).
    #[serde(default)]
    pub all: bool,
    /// Working directory of the git repository. Default: current directory.
    #[serde(default)]
    pub cwd: Option<String>,
    /// If true, skip the safety confirmation prompt. Should only be set by the user, never by the agent.
    #[serde(default)]
    pub auto_approve: bool,
}

#[derive(Deserialize, Serialize)]
pub struct GitCommitOutput {
    pub success: bool,
    pub commit_hash: Option<String>,
    pub message: String,
    pub files_changed: usize,
}

#[derive(Debug, Clone)]
pub struct GitCommit {
    confirmation_handle: ConfirmationHandle,
}

impl GitCommit {
    pub fn new(confirmation_handle: ConfirmationHandle) -> Self {
        Self {
            confirmation_handle,
        }
    }

    /// Check if there are staged changes
    async fn has_staged_changes(cwd: Option<&str>) -> Result<bool, GitCommitError> {
        let mut cmd = tokio::process::Command::new("git");
        cmd.arg("diff").arg("--cached").arg("--quiet");
        if let Some(cwd) = cwd {
            cmd.current_dir(cwd);
        }

        let output = cmd.output().await.map_err(GitCommitError::Io)?;

        // git diff --cached --quiet returns 0 if no changes, 1 if there are changes
        Ok(!output.status.success())
    }

    /// Get the commit hash after committing
    async fn get_commit_hash(cwd: Option<&str>) -> Option<String> {
        let mut cmd = tokio::process::Command::new("git");
        cmd.arg("rev-parse").arg("HEAD");
        if let Some(cwd) = cwd {
            cmd.current_dir(cwd);
        }

        match cmd.output().await {
            Ok(output) => {
                let hash = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if hash.is_empty() { None } else { Some(hash) }
            }
            Err(_) => None,
        }
    }
}

#[async_trait::async_trait]
impl Tool for GitCommit {
    fn name(&self) -> &str {
        "git_commit"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Commit changes to the git repository with a message. \
                Before committing, checks if there are staged changes. \
                Use `git_status` first to see what's staged. \
                For safety, this tool will prompt for confirmation unless auto_approve is set."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "message": {
                        "type": "string",
                        "description": "Commit message."
                    },
                    "all": {
                        "type": "boolean",
                        "description": "Automatically stage all modified and deleted files before committing (like `git commit -a`)."
                    },
                    "cwd": {
                        "type": "string",
                        "description": "Working directory of the git repository. Default: current directory."
                    },
                    "auto_approve": {
                        "type": "boolean",
                        "description": "If true, skip the safety confirmation prompt. Only set this if you are confident the commit is safe. Default: false."
                    }
                },
                "required": ["message"]
            }),
        }
    }

    async fn call(&self, args: serde_json::Value) -> Result<String, String> {
        let args: GitCommitArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
        let cwd = args.cwd.as_deref();

        if !args.auto_approve {
            let approved = confirm_action(
                &self.confirmation_handle,
                "You are about to commit changes to the repository:",
                &format!("commit message: {}", args.message),
            )
            .await;
            if !approved {
                return Err("Commit cancelled by user".to_string());
            }
        }

        if !args.all && !Self::has_staged_changes(cwd).await.map_err(|e| e.to_string())? {
            return Err("No changes staged for commit".to_string());
        }

        let mut cmd = tokio::process::Command::new("git");
        cmd.arg("commit").arg("-m").arg(&args.message);

        if args.all {
            cmd.arg("-a");
        }

        if let Some(cwd) = cwd {
            cmd.current_dir(cwd);
        }

        cmd.stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        let output = cmd.output().await.map_err(|e| e.to_string())?;

        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).to_string());
        }

        let commit_hash = Self::get_commit_hash(cwd).await;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let files_changed = stdout
            .lines()
            .filter(|line| line.contains("file changed") || line.contains("files changed"))
            .filter_map(|line| {
                line.split_whitespace()
                    .next()
                    .and_then(|n| n.parse::<usize>().ok())
            })
            .next()
            .unwrap_or(0);

        let result = GitCommitOutput {
            success: true,
            commit_hash,
            message: args.message,
            files_changed,
        };
        serde_json::to_string(&result).map_err(|e| e.to_string())
    }
}
