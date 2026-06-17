use std::path::Path;
use serde::{Deserialize, Serialize};
use serde_json::json;

use base64::Engine;
use crate::tools::Tool;
use crate::core::types::ToolDefinition;

#[derive(Debug, Deserialize)]
pub struct SendFileArgs {
    pub path: String,
    pub message: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SendFileOutput {
    pub path: String,
    pub name: String,
    pub mime: String,
    pub data: String,
    pub size: u64,
    pub encoding: String,
    pub message: Option<String>,
}

pub struct SendFile;

#[async_trait::async_trait]
impl Tool for SendFile {
    fn name(&self) -> &str {
        "send_file"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "send_file".to_string(),
            description: "Send a file from the local filesystem to the user's mobile device. "
                .to_string() + "Use this when the user asks you to send a file to their phone. "
                + "The file will be transferred and the user can save or share it. "
                + "Works with any file type (code, documents, images, etc.).",
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Absolute path to the file to send"
                    },
                    "message": {
                        "type": "string",
                        "description": "Optional message to accompany the file"
                    }
                },
                "required": ["path"]
            }),
        }
    }

    async fn call(&self, args: serde_json::Value) -> Result<String, String> {
        let args: SendFileArgs = serde_json::from_value(args)
            .map_err(|e| format!("Invalid arguments: {}", e))?;

        let path = Path::new(&args.path);
        if !path.exists() {
            return Err(format!("File not found: {}", args.path));
        }

        let raw = std::fs::read(&args.path)
            .map_err(|e| format!("Failed to read file: {}", e))?;

        let name = path.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        // Check file size (warn if > 10MB)
        if raw.len() > 10 * 1024 * 1024 {
            return Err(format!(
                "File too large ({}). Maximum allowed is 10MB.",
                format_size(raw.len())
            ));
        }

        let mime = infer_mime(path);
        let b64 = base64::engine::general_purpose::STANDARD.encode(&raw);

        let output = SendFileOutput {
            path: args.path.clone(),
            name,
            mime,
            data: b64,
            size: raw.len() as u64,
            encoding: "base64".to_string(),
            message: args.message,
        };

        serde_json::to_string(&output)
            .map_err(|e| format!("Failed to serialize output: {}", e))
    }
}

fn infer_mime(path: &Path) -> String {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "rs" | "js" | "ts" | "py" | "go" | "java" | "c" | "cpp"
        | "rb" | "php" | "swift" | "kt" | "scala" | "h" | "hpp"
        | "r" | "m" | "mm" | "dart" | "lua" => "text/plain".to_string(),
        "html" | "htm" => "text/html".to_string(),
        "css" => "text/css".to_string(),
        "json" => "application/json".to_string(),
        "md" | "txt" | "log" => "text/plain".to_string(),
        "xml" => "application/xml".to_string(),
        "yaml" | "yml" => "application/yaml".to_string(),
        "toml" => "application/toml".to_string(),
        "png" => "image/png".to_string(),
        "jpg" | "jpeg" => "image/jpeg".to_string(),
        "gif" => "image/gif".to_string(),
        "svg" => "image/svg+xml".to_string(),
        "ico" => "image/x-icon".to_string(),
        "pdf" => "application/pdf".to_string(),
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document".to_string(),
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet".to_string(),
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation".to_string(),
        "zip" | "tar" | "gz" | "bz2" | "xz" => "application/zip".to_string(),
        "wasm" => "application/wasm".to_string(),
        _ => "application/octet-stream".to_string(),
    }
}

fn format_size(bytes: usize) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size > 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    format!("{:.1} {}", size, UNITS[unit])
}
