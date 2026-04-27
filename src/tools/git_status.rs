use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum GitStatusError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Git error: {0}")]
    Git(String),
    #[error("Not a git repository")]
    NotGitRepo,
}

#[derive(Deserialize, Serialize)]
pub struct GitStatusArgs {
    /// Working directory of the git repository. Default: current directory.
    #[serde(default)]
    pub cwd: Option<String>,
}

#[derive(Deserialize, Serialize)]
pub struct FileStatus {
    pub path: String,
    /// Git status code (e.g., "M ", " M", "??", "A ", etc.)
    pub status: String,
    /// Human-readable status description
    pub description: String,
}

#[derive(Deserialize, Serialize)]
pub struct GitStatusOutput {
    /// Whether the directory is a git repository
    pub is_git_repo: bool,
    /// Current branch name
    pub branch: Option<String>,
    /// List of files with their status
    pub files: Vec<FileStatus>,
    /// Summary counts
    pub summary: StatusSummary,
    /// Raw porcelain output for debugging
    pub raw_output: String,
}

#[derive(Deserialize, Serialize)]
pub struct StatusSummary {
    pub modified: usize,
    pub added: usize,
    pub deleted: usize,
    pub untracked: usize,
    pub renamed: usize,
    pub copied: usize,
}

impl Default for StatusSummary {
    fn default() -> Self {
        Self {
            modified: 0,
            added: 0,
            deleted: 0,
            untracked: 0,
            renamed: 0,
            copied: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct GitStatus;

impl GitStatus {
    /// Parse porcelain v1 output into structured data
    fn parse_porcelain(output: &str) -> (Vec<FileStatus>, StatusSummary) {
        let mut files = Vec::new();
        let mut summary = StatusSummary::default();

        for line in output.lines() {
            if line.len() < 4 {
                continue;
            }

            let index_status = line.chars().nth(0).unwrap_or(' ');
            let worktree_status = line.chars().nth(1).unwrap_or(' ');
            let path = line[3..].to_string();

            let status_code = format!("{}{}", index_status, worktree_status);
            let description = Self::status_description(index_status, worktree_status);

            // Count summary
            match index_status {
                'M' => summary.modified += 1,
                'A' => summary.added += 1,
                'D' => summary.deleted += 1,
                'R' => summary.renamed += 1,
                'C' => summary.copied += 1,
                _ => {}
            }
            match worktree_status {
                'M' => summary.modified += 1,
                'D' => summary.deleted += 1,
                '?' => summary.untracked += 1,
                _ => {}
            }

            files.push(FileStatus {
                path,
                status: status_code,
                description,
            });
        }

        (files, summary)
    }

    fn status_description(index: char, worktree: char) -> String {
        match (index, worktree) {
            ('M', ' ') => "modified (staged)".to_string(),
            (' ', 'M') => "modified (unstaged)".to_string(),
            ('M', 'M') => "modified (staged and unstaged)".to_string(),
            ('A', ' ') => "added (staged)".to_string(),
            ('D', ' ') => "deleted (staged)".to_string(),
            (' ', 'D') => "deleted (unstaged)".to_string(),
            ('R', ' ') => "renamed (staged)".to_string(),
            ('C', ' ') => "copied (staged)".to_string(),
            ('?', '?') => "untracked".to_string(),
            ('!', '!') => "ignored".to_string(),
            (i, w) => format!("index: '{}', worktree: '{}'", i, w),
        }
    }

    /// Get current branch name
    async fn get_branch(cwd: Option<&str>) -> Option<String> {
        let mut cmd = tokio::process::Command::new("git");
        cmd.arg("branch").arg("--show-current");
        if let Some(cwd) = cwd {
            cmd.current_dir(cwd);
        }

        match cmd.output().await {
            Ok(output) => {
                let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if branch.is_empty() {
                    None
                } else {
                    Some(branch)
                }
            }
            Err(_) => None,
        }
    }
}

impl Tool for GitStatus {
    const NAME: &'static str = "git_status";
    type Error = GitStatusError;
    type Args = GitStatusArgs;
    type Output = GitStatusOutput;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Get the current git repository status in a structured format. \
                Returns information about modified, added, deleted, untracked files, \
                and the current branch. Use this instead of `shell_exec` with `git status` \
                for better structured output that's easier to parse."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "cwd": {
                        "type": "string",
                        "description": "Working directory of the git repository. Default: current directory."
                    }
                }
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let cwd = args.cwd.as_deref();

        // Check if it's a git repo
        let mut check_cmd = tokio::process::Command::new("git");
        check_cmd.arg("rev-parse").arg("--git-dir");
        if let Some(cwd) = cwd {
            check_cmd.current_dir(cwd);
        }

        match check_cmd.output().await {
            Ok(output) if output.status.success() => {}
            _ => {
                return Ok(GitStatusOutput {
                    is_git_repo: false,
                    branch: None,
                    files: vec![],
                    summary: StatusSummary::default(),
                    raw_output: String::new(),
                });
            }
        }

        // Get branch
        let branch = Self::get_branch(cwd).await;

        // Get porcelain status
        let mut status_cmd = tokio::process::Command::new("git");
        status_cmd.arg("status").arg("--porcelain");
        if let Some(cwd) = cwd {
            status_cmd.current_dir(cwd);
        }

        let output = status_cmd.output().await.map_err(GitStatusError::Io)?;

        if !output.status.success() {
            return Err(GitStatusError::Git(
                String::from_utf8_lossy(&output.stderr).to_string(),
            ));
        }

        let raw_output = String::from_utf8_lossy(&output.stdout).to_string();
        let (files, summary) = Self::parse_porcelain(&raw_output);

        Ok(GitStatusOutput {
            is_git_repo: true,
            branch,
            files,
            summary,
            raw_output,
        })
    }
}
