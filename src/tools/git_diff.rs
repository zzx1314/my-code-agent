use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum GitDiffError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Git error: {0}")]
    Git(String),
}

#[derive(Deserialize, Serialize)]
pub struct GitDiffArgs {
    /// File path to diff. If not provided, shows all changes.
    #[serde(default)]
    pub file: Option<String>,
    /// Working directory of the git repository. Default: current directory.
    #[serde(default)]
    pub cwd: Option<String>,
    /// Show staged changes (--cached). Default: false.
    #[serde(default)]
    pub cached: bool,
    /// Max number of lines to return. Default: 2000.
    #[serde(default)]
    pub max_lines: Option<usize>,
}

#[derive(Deserialize, Serialize)]
pub struct GitDiffOutput {
    pub file: Option<String>,
    pub cached: bool,
    pub diff: String,
    pub truncated: bool,
}

#[derive(Debug, Clone)]
pub struct GitDiff;

impl GitDiff {
    const DEFAULT_MAX_LINES: usize = 2000;
}

impl Tool for GitDiff {
    const NAME: &'static str = "git_diff";
    type Error = GitDiffError;
    type Args = GitDiffArgs;
    type Output = GitDiffOutput;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Show git diff for the entire repository or a specific file. \
                Returns the diff output showing changes between commits, or between the \
                staging area and the working directory. Use this instead of `shell_exec` \
                with `git diff` for better integration."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "file": {
                        "type": "string",
                        "description": "File path to diff. If not provided, shows all changes."
                    },
                    "cwd": {
                        "type": "string",
                        "description": "Working directory of the git repository. Default: current directory."
                    },
                    "cached": {
                        "type": "boolean",
                        "description": "Show staged changes (--cached). Default: false."
                    },
                    "max_lines": {
                        "type": "integer",
                        "description": "Max number of lines to return. Default: 2000."
                    }
                }
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let max_lines = args.max_lines.unwrap_or(Self::DEFAULT_MAX_LINES);
        let cwd = args.cwd.as_deref();

        let mut cmd = tokio::process::Command::new("git");
        cmd.arg("diff");

        if args.cached {
            cmd.arg("--cached");
        }

        // Add file path if specified
        if let Some(file) = &args.file {
            cmd.arg("--").arg(file);
        }

        if let Some(cwd) = cwd {
            cmd.current_dir(cwd);
        }

        cmd.stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        let output = cmd.output().await.map_err(GitDiffError::Io)?;

        if !output.status.success() {
            return Err(GitDiffError::Git(
                String::from_utf8_lossy(&output.stderr).to_string(),
            ));
        }

        let diff = String::from_utf8_lossy(&output.stdout).to_string();

        // Truncate if too long
        let (truncated_diff, was_truncated) = if diff.lines().count() > max_lines {
            let truncated: Vec<&str> = diff.lines().take(max_lines).collect();
            (
                format!(
                    "{}\n\n... [diff truncated, {} more lines]",
                    truncated.join("\n"),
                    diff.lines().count() - max_lines
                ),
                true,
            )
        } else {
            (diff, false)
        };

        Ok(GitDiffOutput {
            file: args.file,
            cached: args.cached,
            diff: truncated_diff,
            truncated: was_truncated,
        })
    }
}
