use crate::core::types::ToolDefinition;
use crate::tools::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Deserialize, Serialize)]
pub struct ApplyPatchArgs {
    pub path: String,
    pub patch: String,
}

#[derive(Debug, Serialize)]
pub struct ApplyPatchOutput {
    pub path: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_diff: Option<String>,
}

/// A line in a unified diff hunk body.
#[derive(Debug, Clone)]
struct HunkLine {
    /// ' ' = context, '-' = removal, '+' = addition
    kind: char,
    content: String,
}

/// A parsed hunk from a unified diff.
#[derive(Debug, Clone)]
struct Hunk {
    /// 1-indexed original file start line (from the @@ header)
    old_start: usize,
    body: Vec<HunkLine>,
}

#[derive(Debug, Clone, Default)]
pub struct ApplyPatch;

#[async_trait::async_trait]
impl Tool for ApplyPatch {
    fn name(&self) -> &str {
        "apply_patch"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Apply a unified diff (patch) to a file. \
                The patch should be in standard unified diff format with @@ hunks. \
                Use this for batch file changes that would otherwise require multiple file_update calls."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the file to patch"
                    },
                    "patch": {
                        "type": "string",
                        "description": "Unified diff content to apply"
                    }
                },
                "required": ["path", "patch"]
            }),
        }
    }

    async fn call(&self, args: serde_json::Value) -> Result<String, String> {
        let args: ApplyPatchArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;

        let content = tokio::fs::read_to_string(&args.path)
            .await
            .map_err(|e| format!("Failed to read {}: {}", args.path, e))?;

        let hunks = parse_patch(&args.patch)?;

        if hunks.is_empty() {
            return Err("Patch contains no hunks".to_string());
        }

        let new_content = apply_hunks(&content, &hunks, &args.path)?;

        let (_, git_diff) = super::fs_write_with_tracking(
            &args.path,
            &new_content,
            "apply_patch",
            Some(content),
        )
        .await?;

        serde_json::to_string(&ApplyPatchOutput {
            path: args.path,
            message: "Patch applied successfully".to_string(),
            git_diff,
        })
        .map_err(|e| e.to_string())
    }
}

fn parse_patch(patch: &str) -> Result<Vec<Hunk>, String> {
    let mut hunks = Vec::new();
    let mut current_body: Option<Vec<HunkLine>> = None;
    let mut current_old_start: usize = 0;

    for line in patch.lines() {
        if line.starts_with("@@") {
            if let Some(body) = current_body.take() {
                if !body.is_empty() {
                    hunks.push(Hunk {
                        old_start: current_old_start,
                        body,
                    });
                }
            }
            match parse_hunk_header(line) {
                Some((old_start, _, _, _)) => {
                    current_old_start = old_start;
                    current_body = Some(Vec::new());
                }
                None => {
                    return Err(format!("Malformed hunk header: {}", line));
                }
            }
            continue;
        }

        let Some(body) = &mut current_body else {
            continue;
        };

        if line.starts_with("---") || line.starts_with("+++") {
            continue;
        }

        if line.starts_with('\\') {
            continue;
        }

        if line.is_empty() {
            continue;
        }

        let kind = line.chars().next().unwrap();
        match kind {
            ' ' | '-' | '+' => {
                body.push(HunkLine {
                    kind,
                    content: line[1..].to_string(),
                });
            }
            _ => continue,
        }
    }

    if let Some(body) = current_body {
        if !body.is_empty() {
            hunks.push(Hunk {
                old_start: current_old_start,
                body,
            });
        }
    }

    Ok(hunks)
}

fn parse_hunk_header(line: &str) -> Option<(usize, Option<usize>, usize, Option<usize>)> {
    let content = line
        .trim_start_matches("@@")
        .trim_end_matches("@@")
        .trim();

    let parts: Vec<&str> = content.split_whitespace().collect();
    if parts.len() < 2 {
        return None;
    }

    let old_part = parts[0].strip_prefix('-')?;
    let old_split: Vec<&str> = old_part.split(',').collect();
    let old_start: usize = old_split[0].parse().ok()?;
    let old_count = old_split.get(1).and_then(|s| s.parse().ok());

    let new_part = parts[1].strip_prefix('+')?;
    let new_split: Vec<&str> = new_part.split(',').collect();
    let new_start: usize = new_split[0].parse().ok()?;
    let new_count = new_split.get(1).and_then(|s| s.parse().ok());

    Some((old_start, old_count, new_start, new_count))
}

fn apply_hunks(content: &str, hunks: &[Hunk], path: &str) -> Result<String, String> {
    let lines: Vec<&str> = content.lines().collect();
    let total_lines = lines.len();
    let mut result: Vec<String> = Vec::with_capacity(total_lines + 16);
    let mut file_idx: usize = 0;

    for (hunk_idx, hunk) in hunks.iter().enumerate() {
        let target_pos = hunk.old_start.saturating_sub(1);

        if target_pos < file_idx {
            return Err(format!(
                "Hunk {}: old_start {} is before current file position (line {}). \
                 Hunks may be out of order or overlapping in file '{}'.",
                hunk_idx + 1,
                hunk.old_start,
                file_idx + 1,
                path,
            ));
        }

        if target_pos > total_lines
            && !(target_pos == total_lines && hunk.body.iter().all(|hl| hl.kind == '+'))
        {
            return Err(format!(
                "Hunk {}: old_start {} exceeds file length ({} lines). File: {}",
                hunk_idx + 1,
                hunk.old_start,
                total_lines,
                path,
            ));
        }

        for i in file_idx..target_pos.min(total_lines) {
            result.push(lines[i].to_string());
        }
        file_idx = target_pos.min(total_lines);

        for hl in &hunk.body {
            match hl.kind {
                ' ' | '-' => {
                    if file_idx >= total_lines {
                        return Err(format!(
                            "Hunk {}: unexpected end of file at original line {} (expected '{}'). File: {}",
                            hunk_idx + 1,
                            file_idx + 1,
                            hl.content,
                            path,
                        ));
                    }
                    if lines[file_idx] != hl.content {
                        return Err(format!(
                            "Hunk {}: content mismatch at original line {}.\n  Expected (from patch): '{}'\n  Actual (in file):    '{}'\n  File: {}",
                            hunk_idx + 1,
                            file_idx + 1,
                            hl.content,
                            lines[file_idx],
                            path,
                        ));
                    }
                    if hl.kind == ' ' {
                        result.push(lines[file_idx].to_string());
                    }
                    file_idx += 1;
                }
                '+' => {
                    result.push(hl.content.clone());
                }
                _ => unreachable!(),
            }
        }
    }

    for i in file_idx..total_lines {
        result.push(lines[i].to_string());
    }

    let mut new_content = result.join("\n");
    if content.ends_with('\n') {
        new_content.push('\n');
    }

    Ok(new_content)
}