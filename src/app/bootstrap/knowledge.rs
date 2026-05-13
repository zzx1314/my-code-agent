//! Knowledge file generation and management for the `/init` command.
//!
//! This module contains pure business logic for:
//! - Generating project knowledge content locally (without LLM)
//! - Cleaning LLM responses (strip code fences, strip preamble)
//! - Writing knowledge files and rebuilding the agent

use crate::app::InitResult;
use crate::core::agent::preamble::Agent;
use crate::tools::create_mcp_tools;

// ── Response cleaning ────────────────────────────────────────────────────────

/// Strip wrapping code fences from LLM response (e.g. ```markdown ... ```)
pub fn strip_code_fences(raw: &str) -> &str {
    if raw.starts_with("```") && raw.ends_with("```") {
        let inner = &raw[raw.find('\n').unwrap_or(3)..raw.len() - 3];
        inner.trim()
    } else {
        raw
    }
}

/// Strip any preamble text before the first Markdown heading (#).
/// LLMs sometimes prepend explanatory text like "Here is the knowledge document:"
/// before the actual content. This function finds the first line starting with `#`
/// and removes everything before it.
pub fn strip_preamble_before_heading(raw: &str) -> &str {
    for (i, line) in raw.lines().enumerate() {
        if line.starts_with('#') {
            // Found the first heading — return from here
            let offset: usize = raw.lines().take(i).map(|l| l.len() + 1).sum();
            return &raw[offset..];
        }
        // Keep looking only through non-empty preamble lines;
        // if we hit a non-empty, non-heading line followed by content, still continue
        // until we find a heading or exhaust the search
    }
    // No heading found — return as-is
    raw
}

// ── Result building ──────────────────────────────────────────────────────────

/// Write knowledge content to disk and rebuild the agent, returning an `InitResult`
pub fn build_init_result(
    knowledge_file: &str,
    new_content: &str,
    config: &crate::core::config::Config,
    is_update: bool,
) -> InitResult {
    let action = if is_update { "Updated" } else { "Created" };
    match std::fs::write(knowledge_file, new_content) {
        Ok(_) => match rebuild_agent(config) {
            Ok(new_agent) => InitResult {
                message: format!(
                    "✅ {} '{}' ({} bytes) with current project info.\nAgent reloaded with updated knowledge.",
                    action, knowledge_file,
                    new_content.len()
                ),
                new_agent: Some(new_agent),
            },
            Err(e) => InitResult {
                message: format!(
                    "✅ {} '{}' with current project info.\n⚠️ Failed to reload agent: {}",
                    action, knowledge_file, e
                ),
                new_agent: None,
            },
        },
        Err(e) => InitResult {
            message: format!("❌ Failed to write '{}': {}", knowledge_file, e),
            new_agent: None,
        },
    }
}

/// Rebuild the agent (used for /init)
fn rebuild_agent(config: &crate::core::config::Config) -> anyhow::Result<Agent> {
    use crate::core::agent::preamble::{build_client, build_preamble};
    use crate::core::tool::ToolRegistry;
    let client = build_client(config);
    let system_prompt = build_preamble();
    let mut tools = ToolRegistry::from_config(config);
    let mcp_tools = futures::executor::block_on(create_mcp_tools(config));
    for tool in mcp_tools {
        tools.register_dyn(tool);
    }
    Ok(Agent::new(client, system_prompt, tools))
}

// ── Local knowledge generation (no LLM fallback) ────────────────────────────

/// Local fallback for knowledge generation (no LLM)
pub fn generate_knowledge_content_local() -> String {
    // Parse Cargo.toml once; all sections that need it share this reference
    let cargo_toml = std::fs::read_to_string("Cargo.toml")
        .ok()
        .and_then(|c| c.parse::<toml::Value>().ok());

    let mut content = String::new();
    content.push_str("# Project Knowledge\n\n");

    content.push_str(&section_what_this_is());
    content.push_str(&section_project_structure());
    content.push_str(&section_entry_points());
    content.push_str(&section_dependencies(cargo_toml.as_ref()));
    content.push_str(&section_project_metadata(cargo_toml.as_ref()));
    content.push_str(&section_test_files());
    content.push_str(&section_conventions());

    content
}

/// Extract the first meaningful paragraph from README.md
fn section_what_this_is() -> String {
    let mut s = "## What This Is\n".to_string();
    if let Ok(readme) = std::fs::read_to_string("README.md") {
        let meaningful: String = readme
            .lines()
            .skip_while(|line| line.starts_with('#') || line.trim().is_empty())
            .take_while(|line| !line.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        if !meaningful.is_empty() {
            s.push_str(&meaningful);
            s.push_str("\n\n");
            return s;
        }
    }
    s.push_str("[Describe your project here]\n\n");
    s
}

/// Glob source files with a limit and overflow message
fn format_glob_files(pattern: &str, limit: usize) -> Vec<String> {
    let Ok(entries) = glob::glob(pattern) else {
        return Vec::new();
    };
    let mut files: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter(|e| !e.to_string_lossy().contains("target/"))
        .map(|e| e.to_string_lossy().to_string())
        .collect();
    files.sort();
    files.dedup();
    let total = files.len();
    files.truncate(limit);
    if total > limit {
        files.push(format!("... ({} more files)", total - limit));
    }
    files
}

/// Project directory structure
fn section_project_structure() -> String {
    let mut s = "## Project Structure\n\n```\n".to_string();
    for file in format_glob_files("**/*.rs", 40) {
        s.push_str(&format!("{}\n", file));
    }
    s.push_str("```\n\n");
    s
}

/// Common entry-point files
fn section_entry_points() -> String {
    let mut s = "## Entry Points\n\n".to_string();
    for entry in &["src/main.rs", "src/lib.rs", "src/index.rs", "src/app.rs"] {
        if std::path::Path::new(entry).exists() {
            s.push_str(&format!("- `{}`\n", entry));
        }
    }
    s.push('\n');
    s
}

/// Parse a single dependency value from Cargo.toml into a display version string
fn parse_dep_version(value: &toml::Value) -> String {
    match value {
        toml::Value::String(v) => v.clone(),
        toml::Value::Table(t) => {
            let ver = t.get("version").and_then(|v| v.as_str()).unwrap_or("*");
            let features = t
                .get("features")
                .and_then(|f| f.as_array())
                .map(|arr| {
                    let feats: Vec<&str> = arr.iter().filter_map(|v| v.as_str()).collect();
                    format!(" (features: {})", feats.join(", "))
                })
                .unwrap_or_default();
            format!("{}{}", ver, features)
        }
        _ => "*".to_string(),
    }
}

/// Format a dependency table as a sorted Markdown list
fn format_dep_table(deps: &toml::value::Table, skip_underscore: bool) -> String {
    let mut list: Vec<String> = deps
        .iter()
        .filter(|(name, _)| !(skip_underscore && name.starts_with('_')))
        .map(|(name, value)| format!("- **{}** v{}", name, parse_dep_version(value)))
        .collect();
    list.sort();
    list.join("\n")
}

/// Key dependencies and dev-dependencies
fn section_dependencies(cargo_toml: Option<&toml::Value>) -> String {
    let mut s = "## Key Dependencies\n\n".to_string();
    let Some(toml) = cargo_toml else {
        return s;
    };
    if let Some(deps) = toml.get("dependencies").and_then(|d| d.as_table()) {
        let list = format_dep_table(deps, true);
        if !list.is_empty() {
            s.push_str(&list);
            s.push_str("\n\n");
        }
    }
    if let Some(dev_deps) = toml.get("dev-dependencies").and_then(|d| d.as_table()) {
        if !dev_deps.is_empty() {
            s.push_str("### Dev Dependencies\n\n");
            s.push_str(&format_dep_table(dev_deps, false));
            s.push_str("\n\n");
        }
    }
    s
}

/// Project metadata (edition, crate name)
fn section_project_metadata(cargo_toml: Option<&toml::Value>) -> String {
    let Some(package) = cargo_toml.and_then(|t| t.get("package")) else {
        return String::new();
    };
    let mut meta_parts = Vec::new();
    if let Some(edition) = package.get("edition").and_then(|e| e.as_str()) {
        meta_parts.push(format!("- Rust edition: {}", edition));
    }
    if let Some(name) = package.get("name").and_then(|n| n.as_str()) {
        meta_parts.push(format!("- Crate name: {}", name));
    }
    if meta_parts.is_empty() {
        return String::new();
    }
    format!("## Project Metadata\n\n{}\n\n", meta_parts.join("\n"))
}

/// Test file listing
fn section_test_files() -> String {
    let mut s = "## Tests\n\n".to_string();
    let Ok(entries) = glob::glob("tests/**/*.rs") else {
        return s;
    };
    let mut test_files: Vec<String> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.to_string_lossy().to_string())
        .collect();
    test_files.sort();
    if test_files.is_empty() {
        s.push_str("[No test files found in tests/]\n\n");
        return s;
    }
    let total = test_files.len();
    for file in test_files.iter().take(15) {
        s.push_str(&format!("- `{}`\n", file));
    }
    if total > 15 {
        s.push_str(&format!("... ({} more test files)\n", total - 15));
    }
    s.push('\n');
    s
}

/// Auto-detected conventions
fn section_conventions() -> String {
    let mut conventions = Vec::new();
    if std::path::Path::new(".gitignore").exists() {
        conventions.push("- Project uses .gitignore for version control");
    }
    if std::path::Path::new("clippy.toml").exists()
        || std::path::Path::new("rustfmt.toml").exists()
    {
        conventions.push("- Clippy/rustfmt configuration present — follow formatting rules");
    }
    if std::path::Path::new(".github/workflows").exists() {
        conventions.push("- CI/CD workflows in `.github/workflows/`");
    }
    if conventions.is_empty() {
        conventions.push("- [Add important conventions here]");
    }
    format!("## Conventions & Gotchas\n\n{}\n", conventions.join("\n"))
}
