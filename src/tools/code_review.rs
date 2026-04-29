use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum CodeReviewError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("No code files found in path: {0}")]
    NoCodeFiles(String),
    #[error("Path is not a file or directory: {0}")]
    InvalidPath(String),
}

#[derive(Deserialize, Serialize)]
pub struct CodeReviewArgs {
    /// File or directory path to review.
    pub path: String,
    /// File extensions to include when reviewing a directory (e.g., ["rs", "ts", "py"]).
    /// If not provided, reviews common code file extensions.
    #[serde(default)]
    pub file_extensions: Option<Vec<String>>,
    /// Maximum number of files to review. Default: 10.
    #[serde(default = "default_max_files")]
    pub max_files: usize,
    /// Maximum lines per file to read. Default: 500.
    #[serde(default = "default_max_lines_per_file")]
    pub max_lines_per_file: usize,
}

fn default_max_files() -> usize {
    10
}

fn default_max_lines_per_file() -> usize {
    500
}

#[derive(Deserialize, Serialize)]
pub struct CodeReviewOutput {
    pub path: String,
    pub files: Vec<FileToReview>,
    pub total_files: usize,
    pub truncated: bool,
}

#[derive(Deserialize, Serialize)]
pub struct FileToReview {
    pub file: String,
    pub language: Option<String>,
    pub content: String,
    pub line_count: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, Default)]
pub struct CodeReview;

impl CodeReview {
    /// Common code file extensions to review when no extensions specified
    const DEFAULT_CODE_EXTENSIONS: &'static [&'static str] = &[
        "rs", "ts", "js", "py", "go", "java", "c", "cpp", "h", "hpp", "cs", "rb", "php", "swift",
        "kt", "scala", "r", "jl", "m", "mm", "sh", "bash", "zsh", "fish", "ps1", "bat", "cmd",
        "sql", "html", "css", "scss", "less", "vue", "svelte", "jsx", "tsx", "elm", "clj", "hs",
        "ml", "fs", "fsx", "lua", "pl", "pm", "t", "rkt", "dart", "ex", "exs", "erl", "hrl",
    ];

    /// Check if a file extension is a code file
    fn is_code_file(path: &std::path::Path, allowed_extensions: &[String]) -> bool {
        let extensions: Vec<&str> = if allowed_extensions.is_empty() {
            Self::DEFAULT_CODE_EXTENSIONS.to_vec()
        } else {
            allowed_extensions.iter().map(|s| s.as_str()).collect()
        };

        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| extensions.contains(&ext))
            .unwrap_or(false)
    }

    /// Get language hint from file extension
    fn get_language(path: &std::path::Path) -> Option<String> {
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| match ext {
                "rs" => "rust",
                "ts" => "typescript",
                "tsx" => "typescript",
                "js" => "javascript",
                "jsx" => "javascript",
                "py" => "python",
                "go" => "go",
                "java" => "java",
                "c" => "c",
                "cpp" => "cpp",
                "h" | "hpp" => "cpp",
                "cs" => "csharp",
                "rb" => "ruby",
                "php" => "php",
                "swift" => "swift",
                "kt" => "kotlin",
                "scala" => "scala",
                "r" => "r",
                "jl" => "julia",
                "sh" | "bash" | "zsh" | "fish" => "shell",
                "ps1" => "powershell",
                "bat" | "cmd" => "batch",
                "sql" => "sql",
                "html" => "html",
                "css" | "scss" | "less" => "css",
                "vue" => "vue",
                "svelte" => "svelte",
                "elm" => "elm",
                "clj" => "clojure",
                "hs" => "haskell",
                "ml" => "ocaml",
                "fs" | "fsx" => "fsharp",
                "lua" => "lua",
                "pl" | "pm" => "perl",
                "rkt" => "racket",
                "dart" => "dart",
                "ex" | "exs" => "elixir",
                "erl" | "hrl" => "erlang",
                _ => ext,
            }.to_string())
    }

    /// Read a file with line limit
    fn read_file_with_limit(path: &std::path::Path, max_lines: usize) -> Result<(String, usize, bool), std::io::Error> {
        let content = std::fs::read_to_string(path)?;
        let lines: Vec<&str> = content.lines().collect();
        let line_count = lines.len();
        
        if line_count > max_lines {
            let truncated: Vec<&str> = lines.into_iter().take(max_lines).collect();
            Ok((
                format!("{}\n\n... [file truncated, {} more lines]", truncated.join("\n"), line_count - max_lines),
                line_count,
                true,
            ))
        } else {
            Ok((content, line_count, false))
        }
    }

    /// Collect files to review from a path
    fn collect_files(
        &self,
        path: &std::path::Path,
        extensions: &[String],
        max_files: usize,
    ) -> Result<Vec<std::path::PathBuf>, CodeReviewError> {
        if path.is_file() {
            if Self::is_code_file(path, extensions) {
                Ok(vec![path.to_path_buf()])
            } else {
                Err(CodeReviewError::InvalidPath(
                    format!("File {} is not a recognized code file", path.display())
                ))
            }
        } else if path.is_dir() {
            let mut files = Vec::new();
            let mut entries = std::fs::read_dir(path)?
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .collect::<Vec<_>>();
            
            // Sort for consistent ordering
            entries.sort();

            for entry in entries {
                if files.len() >= max_files {
                    break;
                }

                if entry.is_file() && Self::is_code_file(&entry, extensions) {
                    files.push(entry);
                } else if entry.is_dir() {
                    // Recursively search subdirectories
                    let mut sub_files = self.collect_files(&entry, extensions, max_files - files.len())?;
                    files.append(&mut sub_files);
                }
            }

            if files.is_empty() {
                Err(CodeReviewError::NoCodeFiles(path.display().to_string()))
            } else {
                Ok(files)
            }
        } else {
            Err(CodeReviewError::InvalidPath(
                format!("Path {} is not a file or directory", path.display())
            ))
        }
    }
}

impl Tool for CodeReview {
    const NAME: &'static str = "code_review";
    type Error = CodeReviewError;
    type Args = CodeReviewArgs;
    type Output = CodeReviewOutput;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Review code files for quality, potential issues, and improvements. \
                Accepts a file path or directory path. For directories, recursively finds code files \
                and reads their contents for review. \
                Returns the code content structured by file for the model to analyze. \
                Use this tool when asked to review code, perform code review, or analyze code quality."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File or directory path to review."
                    },
                    "file_extensions": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "File extensions to include when reviewing a directory (e.g., [\"rs\", \"ts\", \"py\"]). If not provided, reviews common code file extensions."
                    },
                    "max_files": {
                        "type": "integer",
                        "description": "Maximum number of files to review. Default: 10."
                    },
                    "max_lines_per_file": {
                        "type": "integer",
                        "description": "Maximum lines per file to read. Default: 500."
                    }
                },
                "required": ["path"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let path = std::path::Path::new(&args.path);
        
        if !path.exists() {
            return Err(CodeReviewError::InvalidPath(
                format!("Path {} does not exist", args.path)
            ));
        }

        let extensions = args.file_extensions.unwrap_or_default();
        let files = self.collect_files(path, &extensions, args.max_files)?;
        
        let mut files_to_review = Vec::new();
        let mut truncated = false;

        for file in files.iter().take(args.max_files) {
            let language = Self::get_language(file);
            
            match Self::read_file_with_limit(file, args.max_lines_per_file) {
                Ok((content, line_count, file_truncated)) => {
                    truncated |= file_truncated;
                    files_to_review.push(FileToReview {
                        file: file.display().to_string(),
                        language,
                        content,
                        line_count,
                        truncated: file_truncated,
                    });
                }
                Err(e) => {
                    // Skip files that can't be read, but continue with others
                    tracing::warn!(file = %file.display(), error = %e, "Could not read file");
                }
            }
        }

        Ok(CodeReviewOutput {
            path: args.path,
            files: files_to_review,
            total_files: files.len(),
            truncated,
        })
    }
}
