use crate::core::context::tool_dedup::get_global_tool_dedup;
use crate::core::parser::ParsedFile;
use crate::core::types::ToolDefinition;
use crate::tools::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Deserialize, Serialize)]
pub struct FileOutlineArgs {
    pub path: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct FileOutlineOutput {
    pub path: String,
    pub total_lines: usize,
    pub outline: String,
}

#[derive(Debug, Clone)]
pub struct FileOutline;

#[async_trait::async_trait]
impl Tool for FileOutline {
    fn name(&self) -> &str {
        "file_outline"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Show the structure outline of a source file. \
                Returns a tree view of all functions, structs, enums, impls, traits, and modules \
                with their line ranges. Supports Rust, JavaScript/JSX, TypeScript/TSX, Java, HTML, \
                and Vue files. Use this before file_read on unfamiliar files to understand \
                the file structure and decide which parts to read. \
                Check context first — if you already have the outline, don't re-read it."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "The path to the file to outline (relative to the project root or absolute)"
                    }
                },
                "required": ["path"]
            }),
        }
    }

    async fn call(&self, args: serde_json::Value) -> Result<String, String> {
        let args: FileOutlineArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;

        // Canonicalize path for consistent dedup caching.
        // Different formats like "src/foo.rs", "./src/foo.rs", and absolute paths
        // all resolve to the same cache key, preventing unnecessary re-reads.
        let canonical_path = match std::path::Path::new(&args.path).canonicalize() {
            Ok(p) => p.to_string_lossy().to_string(),
            Err(_) => args.path.clone(),
        };

        // ── Dedup check ───────────────────────────────────────────────
        {
            let dedup = get_global_tool_dedup();
            let mut dedup_guard = dedup.lock().unwrap();
            match dedup_guard.check_file_outline(&canonical_path) {
                crate::core::context::tool_dedup::DedupAction::ShortCircuit(info) => {
                    let outline = info.cached_outline;
                    return serde_json::to_string(&FileOutlineOutput {
                        path: args.path,
                        total_lines: info.total_lines,
                        outline,
                    })
                    .map_err(|e| e.to_string());
                }
                crate::core::context::tool_dedup::DedupAction::Allow => {}
            }
        }

        let content = tokio::fs::read_to_string(&canonical_path)
            .await
            .map_err(|e| e.to_string())?;
        let total_lines = content.lines().count();

        let outline = if let Some(parsed) = ParsedFile::parse_with_path(content, &canonical_path) {
            parsed.get_outline_string()
        } else {
            format!("(unable to parse file - not a supported language)")
        };

        // ── Record in dedup cache ────────────────────────────────────
        {
            let dedup = get_global_tool_dedup();
            let mut dedup_guard = dedup.lock().unwrap();
            dedup_guard.record_file_outline(&canonical_path, total_lines, &outline);
        }

        serde_json::to_string(&FileOutlineOutput {
            path: args.path,
            total_lines,
            outline,
        })
        .map_err(|e| e.to_string())
    }
}
