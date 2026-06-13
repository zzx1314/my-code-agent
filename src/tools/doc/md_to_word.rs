use crate::core::types::ToolDefinition;
use crate::tools::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum MdToWordError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Deserialize, Serialize)]
pub struct MdToWordArgs {
    /// Path to the input Markdown file
    pub input_file: String,
    /// Path to the output Word file (optional, defaults to same name with .docx extension)
    #[serde(default)]
    pub output_file: Option<String>,
    /// Custom template file path (optional)
    #[serde(default)]
    pub template: Option<String>,
    /// Reference document for styling (optional)
    #[serde(default)]
    pub reference_doc: Option<String>,
}

#[derive(Deserialize, Serialize)]
pub struct MdToWordOutput {
    pub success: bool,
    pub input_file: String,
    pub output_file: String,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct MdToWord;

impl MdToWord {
    pub fn new() -> Self {
        Self
    }

    fn get_default_output_path(input: &Path) -> PathBuf {
        let mut output = input.to_path_buf();
        output.set_extension("docx");
        output
    }
}

#[async_trait::async_trait]
impl Tool for MdToWord {
    fn name(&self) -> &str {
        "md_to_word"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Convert a Markdown file to Word (docx) format using pandoc. \
                This tool allows you to create Word documents by writing Markdown content, \
                which is then converted to professionally formatted Word documents."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "input_file": {
                        "type": "string",
                        "description": "Path to the input Markdown file (.md)"
                    },
                    "output_file": {
                        "type": "string",
                        "description": "Path to the output Word file (.docx). If not specified, uses the same name as input with .docx extension."
                    },
                    "template": {
                        "type": "string",
                        "description": "Path to a custom Pandoc template file for formatting"
                    },
                    "reference_doc": {
                        "type": "string",
                        "description": "Path to a reference Word document for styling (e.g., custom fonts, margins)"
                    }
                },
                "required": ["input_file"]
            }),
        }
    }

    async fn call(&self, args: serde_json::Value) -> Result<String, String> {
        let args: MdToWordArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;

        let input_path = Path::new(&args.input_file);

        // Validate input file exists
        if !input_path.exists() {
            return Err(format!("Input file not found: {}", args.input_file));
        }

        // Validate input file extension
        if input_path.extension().and_then(|e| e.to_str()) != Some("md") {
            return Err("Input file must have .md extension".to_string());
        }

        // Determine output path
        let output_path = match &args.output_file {
            Some(path) => PathBuf::from(path),
            None => Self::get_default_output_path(input_path),
        };

        // Check if pandoc is available
        let pandoc_check = tokio::process::Command::new("pandoc")
            .arg("--version")
            .output()
            .await;

        if let Err(e) = pandoc_check {
            return Err(format!(
                "Pandoc is not installed or not in PATH. Please install pandoc first. Error: {}",
                e
            ));
        }

        // Build pandoc command
        let mut cmd = tokio::process::Command::new("pandoc");
        cmd.arg(&args.input_file)
            .arg("-o")
            .arg(&output_path)
            .arg("--to")
            .arg("docx");

        // Add optional template
        if let Some(template) = &args.template {
            if !Path::new(template).exists() {
                return Err(format!("Template file not found: {}", template));
            }
            cmd.arg("--template").arg(template);
        }

        // Add optional reference document
        if let Some(reference_doc) = &args.reference_doc {
            if !Path::new(reference_doc).exists() {
                return Err(format!("Reference document not found: {}", reference_doc));
            }
            cmd.arg("--reference-doc").arg(reference_doc);
        }

        // Execute pandoc
        let output = cmd.output().await.map_err(|e| e.to_string())?;

        if output.status.success() {
            let result = MdToWordOutput {
                success: true,
                input_file: args.input_file.clone(),
                output_file: output_path.display().to_string(),
                message: format!(
                    "Successfully converted {} to {}",
                    args.input_file,
                    output_path.display()
                ),
            };
            serde_json::to_string(&result).map_err(|e| e.to_string())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(format!("Pandoc conversion failed: {}", stderr))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_output_path() {
        let input = Path::new("document.md");
        let output = MdToWord::get_default_output_path(input);
        assert_eq!(output, PathBuf::from("document.docx"));
    }

    #[test]
    fn test_default_output_path_with_directory() {
        let input = Path::new("/path/to/document.md");
        let output = MdToWord::get_default_output_path(input);
        assert_eq!(output, PathBuf::from("/path/to/document.docx"));
    }
}
