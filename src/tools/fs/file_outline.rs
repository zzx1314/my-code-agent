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

        // ── Dedup check ───────────────────────────────────────────────
        {
            let dedup = get_global_tool_dedup();
            let mut dedup_guard = dedup.lock().unwrap();
            match dedup_guard.check_file_outline(&args.path) {
                crate::core::context::tool_dedup::DedupAction::ShortCircuit(info) => {
                    return serde_json::to_string(&FileOutlineOutput {
                        path: args.path,
                        total_lines: info.total_lines,
                        outline: info.format_message(),
                    }).map_err(|e| e.to_string());
                }
                crate::core::context::tool_dedup::DedupAction::Allow => {}
            }
        }

        let content = std::fs::read_to_string(&args.path).map_err(|e| e.to_string())?;
        let total_lines = content.lines().count();

        let outline = if let Some(parsed) = ParsedFile::parse_with_path(content, &args.path) {
            let structures = parsed.get_all_structures();
            format_outline(&structures, total_lines)
        } else {
            format!("(unable to parse file - not a supported language)")
        };

        // ── Record in dedup cache ────────────────────────────────────
        {
            let dedup = get_global_tool_dedup();
            let mut dedup_guard = dedup.lock().unwrap();
            dedup_guard.record_file_outline(&args.path, total_lines);
        }

        serde_json::to_string(&FileOutlineOutput {
            path: args.path,
            total_lines,
            outline,
        })
        .map_err(|e| e.to_string())
    }
}

fn format_outline(structures: &[crate::core::parser::StructureInfo], total_lines: usize) -> String {
    if structures.is_empty() {
        return "(no structures found)".to_string();
    }

    let mut output = String::new();
    for (i, s) in structures.iter().enumerate() {
        let is_last = i == structures.len() - 1;
        let prefix = if is_last { "└── " } else { "├── " };
        let name = s.name.as_deref().unwrap_or("anonymous");
        let lines = s.end_line - s.start_line + 1;

        output.push_str(&format!(
            "{}[{}-{}: {} lines] {} {}\n",
            prefix,
            s.start_line + 1,
            s.end_line + 1,
            lines,
            s.kind,
            name
        ));
    }

    output.push_str(&format!("\nTotal: {} lines", total_lines));
    output
}
