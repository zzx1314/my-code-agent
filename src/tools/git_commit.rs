use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::safety::confirm_action;

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
pub struct GitCommit;

impl GitCommit {
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
                if hash.is_empty() {
                    None
                } else {
                    Some(hash)
                }
            }
            Err(_) => None,
        }
    }
}

impl Tool for GitCommit {
    const NAME: &'static str = "git_commit";
    type Error = GitCommitError;
    type Args = GitCommitArgs;
    type Output = GitCommitOutput;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
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

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let cwd = args.cwd.as_deref();

        // Safety check
        if !args.auto_approve {
            let approved = confirm_action(
                "You are about to commit changes to the repository:",
                &format!("commit message: {}", args.message),
            )
            .await;
            if !approved {
                return Err(GitCommitError::Cancelled(
                    "Commit cancelled by user".to_string(),
                ));
            }
        }

        // Check for staged changes (unless --all is used)
        if !args.all && !Self::has_staged_changes(cwd).await? {
            return Err(GitCommitError::NothingToCommit);
        }

        // Build commit command
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

        let output = cmd.output().await.map_err(GitCommitError::Io)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            return Err(GitCommitError::Git(stderr));
        }

        let commit_hash = Self::get_commit_hash(cwd).await;

        // Parse output to get files changed
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

        Ok(GitCommitOutput {
            success: true,
            commit_hash,
            message: args.message,
            files_changed,
        })
    }
}
