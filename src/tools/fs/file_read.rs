use crate::core::config::Config;
use crate::core::context::file_cache::get_global_file_cache;
use crate::core::parser::ParsedFile;
use crate::core::context::tool_dedup::get_global_tool_dedup;
use crate::core::types::ToolDefinition;
use crate::tools::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum FileReadError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Default number of lines returned when `limit` is not specified.
/// Used as a fallback when no config is provided.
///
/// This is intentionally lower than the `@filepath` expansion limit (500 lines)
/// because `file_read` is agent-initiated (may read many files in a single turn)
/// while `@filepath` is user-initiated (one explicit attachment at a time).
pub const DEFAULT_READ_LIMIT: usize = 200;

#[derive(Deserialize, Serialize)]
pub struct FileReadArgs {
    pub path: String,
    #[serde(default = "default_offset")]
    pub offset: Option<usize>,
    #[serde(default)]
    pub limit: Option<usize>,
}

fn default_offset() -> Option<usize> {
    None
}

#[derive(Debug, Deserialize, Serialize)]
pub struct FileReadOutput {
    pub path: String,
    pub content: String,
    /// Total lines in the file (not just the lines returned).
    pub lines: usize,
    /// Start line index (0-indexed) of the returned content.
    pub start: usize,
    /// End line index (exclusive, 0-indexed) of the returned content.
    pub end: usize,
    /// Whether the output was truncated because the file exceeds the requested/default limit.
    pub truncated: bool,
}

#[derive(Debug, Clone)]
pub struct FileRead {
    /// Maximum lines returned when the agent doesn't specify a limit.
    default_read_limit: usize,
}

impl Default for FileRead {
    fn default() -> Self {
        Self {
            default_read_limit: DEFAULT_READ_LIMIT,
        }
    }
}

impl FileRead {
    /// Creates a `FileRead` with config-specified limits.
    pub fn from_config(config: &Config) -> Self {
        Self {
            default_read_limit: config.files.default_read_limit,
        }
    }
}

#[async_trait::async_trait]
impl Tool for FileRead {
    fn name(&self) -> &str {
        "file_read"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Read the contents of a file from the local filesystem. \
                Returns the file content with line numbers. By default, only the first \
                200 lines are returned (set `limit` to read more, or use `offset` to skip ahead). \
                The output includes the file path, total line count, the exact line range read \
                (`start` and `end`), and whether it was truncated. \
                IMPORTANT: Always check conversation history before calling this tool — \
                if the file content is already present, do NOT re-read it. \
                Use `file_outline` first to identify function boundaries, then use `offset`/`limit` \
                to read the exact range needed. Ensure function/method boundaries are complete — \
                never read a partial function that cuts off mid-body."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "The path to the file to read (relative to the project root or absolute)"
                    },
                    "offset": {
                        "type": "integer",
                        "description": "Number of lines to skip from the start (0-indexed). Output line numbers are 1-indexed. Default: 0."
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of lines to read. Default: 200. Increase to read more of a large file."
                    }
                },
                "required": ["path"]
            }),
        }
    }

    async fn call(&self, args: serde_json::Value) -> Result<String, String> {
        let args: FileReadArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
        let offset = args.offset.unwrap_or(0);
        let limit = args.limit.unwrap_or(self.default_read_limit);

        // ── Dedup check ───────────────────────────────────────────────────
        // If we already read the same (path, offset, limit) and the file
        // hasn't been modified since, return a short message instead of the
        // full content.  This saves the model from re-consuming tokens for
        // identical reads.
        {
            let dedup = get_global_tool_dedup();
            let mut dedup_guard = dedup.lock().unwrap();
            match dedup_guard.check_file_read(&args.path, offset, limit) {
                crate::core::context::tool_dedup::DedupAction::ShortCircuit(info) => {
                    let msg = info.format_message();
                    return serde_json::to_string(&FileReadOutput {
                        path: args.path,
                        content: msg,
                        lines: info.total_lines,
                        start: info.start,
                        end: info.end,
                        truncated: false,
                    }).map_err(|e| e.to_string());
                }
                crate::core::context::tool_dedup::DedupAction::Allow => {}
            }
        }

        // ── File I/O (via disk cache) ─────────────────────────────────────
        let cache = get_global_file_cache();
        let content = {
            // Check cache first (sync, under lock — lock is dropped before `.await`)
            let cached = {
                let mut cache_guard = cache.lock().unwrap();
                cache_guard.get(&args.path).map(|entry| entry.content.clone())
            };

            if let Some(cached_content) = cached {
                cached_content
            } else {
                // Cache miss — read from disk asynchronously
                let content = tokio::fs::read_to_string(&args.path).await.map_err(|e| e.to_string())?;
                // Update cache
                let mut cache_guard = cache.lock().unwrap();
                cache_guard.insert(&args.path, content.clone());
                content
            }
        };

        let total_lines = content.lines().count();
        let start = offset.min(total_lines);
        let end = (start + limit).min(total_lines);
        let truncated = end < total_lines;

        // Only parse with tree-sitter when actually truncated and smart_read is needed.
        // This avoids the cost of building a full AST for the common case (small files
        // or targeted reads within the requested range).
        let (adjusted_end, structure_note) = if truncated {
            if let Some(parsed) = ParsedFile::parse_with_path(content.clone(), &args.path) {
                let result = parsed.smart_read(start, limit, total_lines);
                let note = result.extended_structure.map(|s| {
                    format!(
                        "... (extended to include complete {}: {})",
                        s.kind,
                        s.name.as_deref().unwrap_or("anonymous")
                    )
                });
                (result.adjusted_end, note)
            } else {
                (end, None)
            }
        } else {
            (end, None)
        };

        // Build output using iterator — avoids allocating Vec<String> for all lines
        let mut output = String::new();
        if adjusted_end > start {
            output.push_str(&format!(
                "[{}: lines {}-{} of {}]\n",
                args.path,
                start + 1,
                adjusted_end,
                total_lines
            ));
            for (i, line) in content.lines().skip(start).take(adjusted_end - start).enumerate() {
                if i > 0 {
                    output.push('\n');
                }
                output.push_str(&format!("{:>6} | {}", start + i + 1, line));
            }
        }
        if let Some(note) = structure_note {
            output.push_str(&format!("\n{}", note));
        }
        if adjusted_end < total_lines {
            let shown = adjusted_end - start;
            output.push_str(&format!(
                "\n\n... (showing {} of {} total lines. Use offset={} to read more)",
                shown, total_lines, adjusted_end
            ));
        }

        // ── Record in dedup cache ─────────────────────────────────────────
        // Future identical reads will be short-circuited.
        {
            let dedup = get_global_tool_dedup();
            let mut dedup_guard = dedup.lock().unwrap();
            dedup_guard.record_file_read(
                &args.path,
                offset,
                limit,
                total_lines,
                start,
                adjusted_end,
            );
        }

        let result = FileReadOutput {
            path: args.path,
            content: output,
            lines: total_lines,
            start,
            end: adjusted_end,
            truncated: adjusted_end < total_lines,
        };
        serde_json::to_string(&result).map_err(|e| e.to_string())
    }
}
