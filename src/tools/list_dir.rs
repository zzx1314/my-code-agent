use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs;
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum ListDirError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Path not found: {path}")]
    NotFound { path: String },
    #[error("Path is not a directory: {path}")]
    NotADirectory { path: String },
}

#[derive(Deserialize, Serialize)]
pub struct ListDirArgs {
    pub path: String,
    #[serde(default = "default_max_depth")]
    pub max_depth: usize,
}

fn default_max_depth() -> usize {
    1
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ListDirOutput {
    pub path: String,
    pub entries: Vec<DirEntry>,
    pub total_files: usize,
    pub total_dirs: usize,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DirEntry {
    pub name: String,
    pub entry_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<DirEntry>>,
}

#[derive(Debug, Clone, Default)]
pub struct ListDir;

impl Tool for ListDir {
    const NAME: &'static str = "list_dir";
    type Error = ListDirError;
    type Args = ListDirArgs;
    type Output = ListDirOutput;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "List files and directories in a given path. \
                Returns a tree of entries showing file and directory names. \
                Use max_depth to control recursion depth (default: 1, i.e. flat listing). \
                Useful for exploring project structure and discovering files."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "The directory path to list (relative to project root or absolute). Default: current directory."
                    },
                    "max_depth": {
                        "type": "integer",
                        "description": "Maximum recursion depth. 1 = flat listing (default), 2 = one level of subdirectories, etc."
                    }
                },
                "required": ["path"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, ListDirError> {
        let path = Path::new(&args.path);

        if !path.exists() {
            return Err(ListDirError::NotFound {
                path: args.path.clone(),
            });
        }

        if !path.is_dir() {
            return Err(ListDirError::NotADirectory {
                path: args.path.clone(),
            });
        }

        let mut total_files = 0usize;
        let mut total_dirs = 0usize;

        let entries =
            list_dir_recursive(path, args.max_depth, 1, &mut total_files, &mut total_dirs);

        Ok(ListDirOutput {
            path: args.path,
            entries,
            total_files,
            total_dirs,
        })
    }
}

/// Recursively lists directory contents up to `max_depth`.
fn list_dir_recursive(
    dir: &Path,
    max_depth: usize,
    current_depth: usize,
    total_files: &mut usize,
    total_dirs: &mut usize,
) -> Vec<DirEntry> {
    let mut entries: Vec<DirEntry> = Vec::new();

    let read_dir = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => return entries,
    };

    // Collect and sort entries for deterministic output
    let mut dir_entries: Vec<_> = read_dir.filter_map(|e| e.ok()).collect();
    dir_entries.sort_by(|a, b| {
        // Directories first, then files; alphabetically within each group
        let a_is_dir = a.file_type().map(|t| t.is_dir()).unwrap_or(false);
        let b_is_dir = b.file_type().map(|t| t.is_dir()).unwrap_or(false);
        match (a_is_dir, b_is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.file_name().cmp(&b.file_name()),
        }
    });

    for entry in dir_entries {
        let name = entry.file_name().to_string_lossy().to_string();
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);

        if is_dir {
            *total_dirs += 1;
            let children = if current_depth < max_depth {
                let kids = list_dir_recursive(
                    &entry.path(),
                    max_depth,
                    current_depth + 1,
                    total_files,
                    total_dirs,
                );
                if kids.is_empty() { None } else { Some(kids) }
            } else {
                None
            };
            entries.push(DirEntry {
                name,
                entry_type: "directory".to_string(),
                children,
            });
        } else {
            *total_files += 1;
            entries.push(DirEntry {
                name,
                entry_type: "file".to_string(),
                children: None,
            });
        }
    }

    entries
}
