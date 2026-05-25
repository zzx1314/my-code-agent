use crate::core::context::file_cache::get_global_file_cache;
use crate::core::parser::ParsedFile;
use crate::core::types::ToolDefinition;
use crate::tools::Tool;
use serde::Deserialize;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::path::Path;

const CLUSTER_GAP: usize = 10;
const PADDING: usize = 3;
const MAX_OUTPUT: usize = 15_000;

#[derive(Deserialize)]
struct ExploreContextArgs {
    query: String,
    #[serde(default = "default_max_files")]
    max_files: usize,
}

fn default_max_files() -> usize {
    5
}

#[derive(Debug, Clone)]
pub struct RawMatch {
    pub file: String,
    pub line: usize,
}

#[derive(Debug)]
pub struct Cluster {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, Default)]
pub struct ExploreContext;

#[async_trait::async_trait]
impl Tool for ExploreContext {
    fn name(&self) -> &str {
        "explore_context"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: concat!(
                "Deep code exploration — returns comprehensive context for a topic in a SINGLE call. ",
                "Groups all relevant source code by file with structure overview and contiguous code sections. ",
                "Use this instead of multiple code_search + file_read calls when you need to understand ",
                "an unfamiliar module or trace how something works. ",
                "Tip: use specific code terms or symbol names in your query rather than vague descriptions."
            )
            .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "What to explore — use code-related terms, symbol names, file names, or concepts"
                    },
                    "max_files": {
                        "type": "integer",
                        "description": "Maximum number of files to show source code from (default: 5)",
                        "default": 5
                    }
                },
                "required": ["query"]
            }),
        }
    }

    async fn call(&self, raw: serde_json::Value) -> Result<String, String> {
        let args: ExploreContextArgs =
            serde_json::from_value(raw).map_err(|e| format!("Invalid args: {e}"))?;

        let terms = extract_terms(&args.query);
        if terms.is_empty() {
            return Err(
                "Could not extract meaningful search terms. Try a more specific query.".into(),
            );
        }

        let all_matches = run_rg_search(&terms).await?;
        if all_matches.is_empty() {
            return Ok(format!(
                "## Explore Context\n\n**Query:** `{}`\n\nNo results found.",
                args.query
            ));
        }

        let mut by_file: HashMap<String, Vec<RawMatch>> = HashMap::new();
        for m in all_matches {
            by_file.entry(m.file.clone()).or_default().push(m);
        }

        let mut sorted: Vec<(String, Vec<RawMatch>)> = by_file.into_iter().collect();
        sorted.sort_by(|a, b| b.1.len().cmp(&a.1.len()));

        let total: usize = sorted.iter().map(|(_, m)| m.len()).sum();
        let mut out = String::new();
        out.push_str(&format!(
            "## Explore Context\n\n**Query:** `{}`\n",
            args.query
        ));
        out.push_str(&format!(
            "**Summary:** {} matches across {} files\n",
            total,
            sorted.len()
        ));

        let limit = args.max_files.min(sorted.len());
        let mut done = 0usize;

        for idx in 0..limit {
            let (ref path, ref matches) = sorted[idx];

            let mut ms = matches.clone();
            ms.sort_by(|a, b| a.line.cmp(&b.line));
            ms.dedup_by(|a, b| a.line == b.line);

            let content = read_file(path).await?;
            let total_lines = content.lines().count();
            let parsed = ParsedFile::parse_with_path(content.clone(), path);

            out.push_str(&format!("\n---\n\n### `{}` ({} matches)\n", path, ms.len()));

            if let Some(ref p) = parsed {
                let structs = p.get_all_structures();
                if !structs.is_empty() {
                    out.push_str("\n**Structure:**\n");
                    for (i, s) in structs.iter().enumerate() {
                        let last = i == structs.len() - 1;
                        let pre = if last { "└── " } else { "├── " };
                        let name = s.name.as_deref().unwrap_or("(anon)");
                        let lines = s.end_line - s.start_line + 1;
                        out.push_str(&format!(
                            "{}[{}-{}: {} lines] {} {}\n",
                            pre,
                            s.start_line + 1,
                            s.end_line + 1,
                            lines,
                            s.kind,
                            name
                        ));
                    }
                }
            }

            let clusters = build_clusters(&ms);
            let lang = lang_tag(path);
            out.push_str(&format!("\n**Code:**\n```{}\n", lang));

            let file_lines: Vec<&str> = content.lines().collect();
            for (ci, cluster) in clusters.iter().enumerate() {
                if ci > 0 {
                    out.push_str("// ... (gap) ...\n");
                }
                let start_idx = cluster.start.saturating_sub(PADDING);
                let end_idx = (cluster.end + PADDING).min(total_lines.saturating_sub(1));

                if start_idx < cluster.start {
                    let cv = file_lines[cluster.start - 1];
                    out.push_str(&format!("{:>6} | {}\n", cluster.start, cv));
                }

                for lnum in cluster.start..=cluster.end {
                    if lnum < file_lines.len() {
                        out.push_str(&format!("{:>6} | {}\n", lnum + 1, file_lines[lnum]));
                    }
                }

                if end_idx > cluster.end {
                    let after = cluster.end + 1;
                    if after < file_lines.len() {
                        out.push_str(&format!("{:>6} | {}\n", after + 1, file_lines[after]));
                    }
                }
            }

            out.push_str("```\n");

            done = idx + 1;
            if out.len() > MAX_OUTPUT {
                out.push_str("\n... (output truncated)\n");
                break;
            }
        }

        if sorted.len() > done {
            out.push_str("\n**Additional relevant files:**\n");
            for (path, ms) in &sorted[done..] {
                out.push_str(&format!("- `{}` ({} matches)\n", path, ms.len()));
            }
        }

        Ok(out)
    }
}

pub fn extract_terms(query: &str) -> Vec<String> {
    let common: HashSet<&str> = [
        "the", "and", "for", "are", "was", "were", "has", "had", "but", "not", "all", "any", "can",
        "how", "what", "when", "where", "which", "who", "why", "this", "that", "with", "from",
        "have", "been", "will", "would", "could", "should", "does", "done", "make", "made", "use",
        "used", "using", "work", "works", "find", "found", "show", "call", "called", "get", "set",
        "add", "also", "than", "then", "them", "each", "other", "some", "such", "only", "same",
        "about", "more", "most", "very", "just", "does", "into", "over", "too", "need", "like",
        "look", "code", "data", "file", "line", "does",
    ]
    .iter()
    .copied()
    .collect();

    let mut terms: Vec<String> = Vec::new();
    let trimmed = query.trim();
    if trimmed.len() >= 3 {
        terms.push(trimmed.to_string());
    }

    for raw in trimmed.split_whitespace() {
        let clean: String = raw
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if clean.len() < 3 {
            continue;
        }
        let lower = clean.to_lowercase();
        if common.contains(lower.as_str()) {
            continue;
        }
        if clean != trimmed {
            terms.push(clean);
        }
    }

    let mut seen = HashSet::new();
    terms.retain(|t| seen.insert(t.clone()));

    terms
}

async fn run_rg_search(terms: &[String]) -> Result<Vec<RawMatch>, String> {
    let mut combined: Vec<RawMatch> = Vec::new();
    let mut seen: HashSet<(String, usize)> = HashSet::new();

    for term in terms {
        let output = tokio::process::Command::new("rg")
            .arg("-n")
            .arg("--no-heading")
            .arg("--color=never")
            .arg("-i")
            .arg("--max-count")
            .arg("100")
            .arg(term)
            .arg(".")
            .output()
            .await
            .map_err(|e| format!("ripgrep exec failed: {e}"))?;

        let stdout = if output.status.success() || output.status.code() == Some(1) {
            String::from_utf8_lossy(&output.stdout).to_string()
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            tracing::warn!("rg error for term '{term}': {}", stderr.trim());
            continue;
        };

        for line in stdout.lines() {
            let parts: Vec<&str> = line.splitn(3, ':').collect();
            if parts.len() < 3 {
                continue;
            }
            let line_num: usize = match parts[1].parse() {
                Ok(n) => n,
                Err(_) => continue,
            };
            let key = (parts[0].to_string(), line_num);
            if seen.insert(key.clone()) {
                combined.push(RawMatch {
                    file: key.0,
                    line: key.1,
                });
            }
        }
    }

    Ok(combined)
}

pub fn build_clusters(matches: &[RawMatch]) -> Vec<Cluster> {
    if matches.is_empty() {
        return vec![];
    }

    let mut clusters: Vec<Cluster> = Vec::new();
    let mut start = matches[0].line;
    let mut end = matches[0].line;

    for w in matches.windows(2) {
        let prev = w[0].line;
        let curr = w[1].line;

        if curr > prev + CLUSTER_GAP {
            clusters.push(Cluster { start, end });
            start = curr;
            end = curr;
        } else {
            end = curr;
        }
    }

    clusters.push(Cluster { start, end });
    clusters
}

async fn read_file(path: &str) -> Result<String, String> {
    let cache = get_global_file_cache();
    let cached = {
        let mut guard = cache.lock().unwrap();
        guard.get(path).map(|e| e.content.clone())
    };
    if let Some(c) = cached {
        return Ok(c);
    }
    let content = tokio::fs::read_to_string(path)
        .await
        .map_err(|e| format!("Cannot read {path}: {e}"))?;
    let mut guard = cache.lock().unwrap();
    guard.insert(path, content.clone());
    Ok(content)
}

pub fn lang_tag(path: &str) -> &'static str {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    match ext {
        "rs" => "rust",
        "js" | "jsx" | "mjs" | "cjs" => "javascript",
        "ts" | "tsx" | "mts" | "cts" => "typescript",
        "py" => "python",
        "java" => "java",
        "go" => "go",
        "rb" => "ruby",
        "php" => "php",
        "cs" => "csharp",
        "swift" => "swift",
        "kt" | "kts" => "kotlin",
        "c" | "h" => "c",
        "cpp" | "hpp" | "cc" | "cxx" => "cpp",
        "html" | "htm" => "html",
        "css" | "scss" | "less" => "css",
        "toml" => "toml",
        "json" => "json",
        "yaml" | "yml" => "yaml",
        "md" => "markdown",
        "sql" => "sql",
        "sh" | "bash" => "bash",
        _ => "text",
    }
}
