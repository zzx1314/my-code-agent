use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum CodeSearchError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Deserialize, Serialize)]
pub struct CodeSearchArgs {
    pub pattern: String,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub file_type: Option<String>,
    #[serde(default = "default_max_results")]
    pub max_results: usize,
    #[serde(default)]
    pub case_insensitive: bool,
}

fn default_max_results() -> usize {
    50
}

#[derive(Deserialize, Serialize)]
pub struct CodeSearchOutput {
    pub pattern: String,
    pub matches: Vec<SearchMatch>,
    pub total_matches: usize,
}

#[derive(Deserialize, Serialize)]
pub struct SearchMatch {
    pub file: String,
    pub line_number: usize,
    pub line: String,
}

#[derive(Debug, Clone, Default)]
pub struct CodeSearch;

impl Tool for CodeSearch {
    const NAME: &'static str = "code_search";
    type Error = CodeSearchError;
    type Args = CodeSearchArgs;
    type Output = CodeSearchOutput;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Search for a text pattern in source code files using ripgrep (rg). \
                Returns matching lines with file paths and line numbers. \
                Automatically respects .gitignore and skips binary files. \
                Useful for finding function definitions, imports, usages, etc."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "pattern": {
                        "type": "string",
                        "description": "The text pattern to search for"
                    },
                    "path": {
                        "type": "string",
                        "description": "Directory to search in. Default: current directory."
                    },
                    "file_type": {
                        "type": "string",
                        "description": "File extension filter (e.g., 'rs', 'ts', 'py'). Default: search all files."
                    },
                    "max_results": {
                        "type": "integer",
                        "description": "Maximum number of results to return. Default: 50."
                    },
                    "case_insensitive": {
                        "type": "boolean",
                        "description": "Whether to search case-insensitively. Default: false."
                    }
                },
                "required": ["pattern"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, CodeSearchError> {
        let mut cmd = tokio::process::Command::new("rg");

        cmd.arg("-n")          // line numbers
            .arg("--no-heading") // don't group by file
            .arg("--color=never"); // plain text output

        if args.case_insensitive {
            cmd.arg("-i");
        }

        if let Some(ref ext) = args.file_type {
            cmd.arg("-g").arg(format!("*.{}", ext));
        }

        cmd.arg(&args.pattern);

        if let Some(ref path) = args.path {
            cmd.arg(path);
        } else {
            cmd.arg(".");
        }

        let output = cmd.output().await?;

        // rg exits with code 1 when no matches found — treat as empty result, not error
        let stdout = if output.status.success() || output.status.code() == Some(1) {
            String::from_utf8_lossy(&output.stdout).to_string()
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(CodeSearchError::Io(std::io::Error::other(
                format!("rg failed: {}", stderr.trim()),
            )));
        };

        let mut matches: Vec<SearchMatch> = Vec::new();

        for line in stdout.lines() {
            if matches.len() >= args.max_results {
                break;
            }

            // Parse rg output format (same as grep): path:line_num:content
            let parts: Vec<&str> = line.splitn(3, ':').collect();
            if parts.len() == 3
                && let Ok(line_num) = parts[1].parse::<usize>()
            {
                matches.push(SearchMatch {
                    file: parts[0].to_string(),
                    line_number: line_num,
                    line: parts[2].to_string(),
                });
            }
        }

        let total_matches = matches.len();

        Ok(CodeSearchOutput {
            pattern: args.pattern,
            matches,
            total_matches,
        })
    }
}
