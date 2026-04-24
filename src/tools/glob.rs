use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum GlobError {
    #[error("Glob pattern error: {0}")]
    Pattern(#[from] glob::GlobError),
    #[error("Invalid glob pattern: {0}")]
    InvalidPattern(#[from] glob::PatternError),
}

#[derive(Deserialize, Serialize)]
pub struct GlobArgs {
    pub pattern: String,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default = "default_max_results")]
    pub max_results: usize,
}

fn default_max_results() -> usize {
    100
}

#[derive(Debug, Deserialize, Serialize)]
pub struct GlobOutput {
    pub pattern: String,
    pub matches: Vec<String>,
    pub total_matches: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, Default)]
pub struct GlobSearch;

impl Tool for GlobSearch {
    const NAME: &'static str = "glob";
    type Error = GlobError;
    type Args = GlobArgs;
    type Output = GlobOutput;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Find files matching a glob pattern. \
                Supports standard glob syntax: * matches any characters except /, \
                ** matches any characters including /, ? matches a single character, \
                [abc] matches one of the characters in brackets. \
                Useful for finding files by name, extension, or directory structure."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "pattern": {
                        "type": "string",
                        "description": "The glob pattern to match against. Examples: '**/*.rs', 'src/**/*.ts', '*.toml', 'tests/test_*.py'"
                    },
                    "cwd": {
                        "type": "string",
                        "description": "Working directory to search in. Default: current directory."
                    },
                    "max_results": {
                        "type": "integer",
                        "description": "Maximum number of matching paths to return. Default: 100."
                    }
                },
                "required": ["pattern"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, GlobError> {
        let base = Path::new(args.cwd.as_deref().unwrap_or("."));
        let full_pattern = if args.pattern.starts_with('/') {
            args.pattern.clone()
        } else {
            let joined = base.join(&args.pattern);
            joined.to_string_lossy().to_string()
        };

        let glob_iter = glob::glob(&full_pattern)?;

        let mut matches = Vec::new();
        let mut total_matches = 0usize;
        let mut truncated = false;

        for entry in glob_iter {
            match entry {
                Ok(path) => {
                    total_matches += 1;
                    if matches.len() < args.max_results {
                        // Strip the base prefix for cleaner relative paths
                        let display = if !args.pattern.starts_with('/') {
                            path.strip_prefix(base)
                                .map(|p| p.to_string_lossy().to_string())
                                .unwrap_or_else(|_| path.to_string_lossy().to_string())
                        } else {
                            path.to_string_lossy().to_string()
                        };
                        matches.push(display);
                    } else {
                        truncated = true;
                    }
                }
                Err(_) => {
                    // Skip paths with permission or IO errors silently
                }
            }
        }

        Ok(GlobOutput {
            pattern: args.pattern,
            matches,
            total_matches,
            truncated,
        })
    }
}
