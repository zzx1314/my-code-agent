//! Knowledge file generation and management for the `/init` command.
//!
//! This module contains pure business logic for:
//! - Generating project knowledge content locally (without LLM)
//! - Cleaning LLM responses (strip code fences, strip preamble)
//! - Writing knowledge files and rebuilding the agent

use crate::app::InitResult;
use crate::core::agent::preamble::Agent;
use crate::tools::create_mcp_tools;
use std::path::Path;

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
                    action,
                    knowledge_file,
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
    use crate::tools::ToolRegistry;
    let client = build_client(config);
    let system_prompt = build_preamble();
    let mut tools = ToolRegistry::from_config(config);
    let mcp_tools = futures::executor::block_on(create_mcp_tools(config));
    for tool in mcp_tools {
        tools.register_boxed(tool);
    }
    Ok(Agent::new(client, system_prompt, tools))
}

// ── Project type detection ───────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
enum ProjectType {
    Rust,
    JavaScript,
    Python,
    Unknown,
}

fn detect_project_type() -> ProjectType {
    if Path::new("Cargo.toml").exists() {
        ProjectType::Rust
    } else if Path::new("package.json").exists() {
        ProjectType::JavaScript
    } else if Path::new("pyproject.toml").exists() || Path::new("requirements.txt").exists() {
        ProjectType::Python
    } else {
        ProjectType::Unknown
    }
}

fn project_type_label(pt: ProjectType) -> &'static str {
    match pt {
        ProjectType::Rust => "Rust",
        ProjectType::JavaScript => "JavaScript / TypeScript",
        ProjectType::Python => "Python",
        ProjectType::Unknown => "Unknown",
    }
}

fn source_globs(pt: ProjectType) -> &'static [&'static str] {
    match pt {
        ProjectType::Rust => &["**/*.rs"],
        ProjectType::JavaScript => &["**/*.ts", "**/*.tsx", "**/*.js", "**/*.jsx"],
        ProjectType::Python => &["**/*.py"],
        ProjectType::Unknown => &[
            "**/*.rs",
            "**/*.ts",
            "**/*.tsx",
            "**/*.js",
            "**/*.jsx",
            "**/*.py",
            "**/*.go",
            "**/*.java",
            "**/*.kt",
        ],
    }
}

fn test_globs(pt: ProjectType) -> &'static [&'static str] {
    match pt {
        ProjectType::Rust => &["tests/**/*.rs"],
        ProjectType::JavaScript => &[
            "**/*.test.ts",
            "**/*.test.tsx",
            "**/*.test.js",
            "**/*.test.jsx",
            "**/__tests__/**/*",
        ],
        ProjectType::Python => &["tests/**/*.py", "**/test_*.py"],
        ProjectType::Unknown => &[
            "tests/**/*.rs",
            "tests/**/*.py",
            "**/*.test.ts",
            "**/*.test.js",
            "**/test_*.py",
        ],
    }
}

// ── Local knowledge generation (no LLM fallback) ────────────────────────────

/// Local fallback for knowledge generation (no LLM)
pub fn generate_knowledge_content_local() -> String {
    let pt = detect_project_type();
    let cargo_toml = std::fs::read_to_string("Cargo.toml")
        .ok()
        .and_then(|c| c.parse::<toml::Value>().ok());
    let package_json = std::fs::read_to_string("package.json")
        .ok()
        .and_then(|c| serde_json::from_str::<serde_json::Value>(&c).ok());
    let pyproject_toml = std::fs::read_to_string("pyproject.toml")
        .ok()
        .and_then(|c| c.parse::<toml::Value>().ok());

    let mut content = String::new();
    content.push_str("# Project Knowledge\n\n");

    content.push_str(&section_what_this_is());
    content.push_str(&section_features(pt));
    content.push_str(&section_project_structure(pt));
    content.push_str(&section_entry_points());
    content.push_str(&section_dependencies(
        cargo_toml.as_ref(),
        package_json.as_ref(),
        pyproject_toml.as_ref(),
    ));
    content.push_str(&section_configuration());
    content.push_str(&section_test_files(pt));
    content.push_str(&section_conventions());

    content
}

// ── Section builders ─────────────────────────────────────────────────────────

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

/// Extract features description from README, or list based on project type
fn section_features(pt: ProjectType) -> String {
    let mut s = "## Features\n".to_string();

    // Try to find a "## Features" section in README.md
    if let Ok(readme) = std::fs::read_to_string("README.md") {
        let mut in_features = false;
        let mut features_lines: Vec<&str> = Vec::new();
        for line in readme.lines() {
            if line.trim().eq_ignore_ascii_case("## Features")
                || line.trim().eq_ignore_ascii_case("## Feature")
            {
                in_features = true;
                continue;
            }
            if in_features {
                if line.starts_with("## ") && !features_lines.is_empty() {
                    break;
                }
                features_lines.push(line);
            }
        }
        if !features_lines.is_empty() {
            let content: String = features_lines.join("\n").trim().to_string();
            s.push_str(&content);
            s.push_str("\n\n");
            return s;
        }
    }

    // Fallback: project-type-based generic description
    match pt {
        ProjectType::Rust => {
            s.push_str("- Built with Rust — systems programming language\n");
            if Path::new("src/bin/").exists() {
                s.push_str("- Binary crate with multiple entry points\n");
            }
            if Path::new("src/lib.rs").exists() {
                s.push_str("- Library crate providing reusable components\n");
            }
        }
        ProjectType::JavaScript => {
            s.push_str("- Built with JavaScript/TypeScript\n");
            if let Ok(pkg) = std::fs::read_to_string("package.json") {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&pkg) {
                    if let Some(desc) = v.get("description").and_then(|d| d.as_str()) {
                        if !desc.is_empty() {
                            s.push_str(&format!("{}\n\n", desc));
                            return s;
                        }
                    }
                }
            }
            s.push_str("- [Describe key features here]\n");
        }
        ProjectType::Python => {
            s.push_str("- Built with Python\n");
            s.push_str("- [Describe key features here]\n");
        }
        ProjectType::Unknown => {
            s.push_str("[Describe key features here]\n");
        }
    }
    s.push('\n');
    s
}

/// Glob source files with a limit and overflow message
fn format_glob_files(patterns: &[&str], limit: usize, exclude_target: bool) -> Vec<String> {
    let mut all_files: Vec<String> = Vec::new();
    for pattern in patterns {
        let Ok(entries) = glob::glob(pattern) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.to_string_lossy().to_string();
            if exclude_target && path.contains("target/") {
                continue;
            }
            if !all_files.contains(&path) {
                all_files.push(path);
            }
        }
    }
    all_files.sort();
    let total = all_files.len();
    all_files.truncate(limit);
    if total > limit {
        all_files.push(format!("... ({} more files)", total - limit));
    }
    all_files
}

/// Project directory structure
fn section_project_structure(pt: ProjectType) -> String {
    let mut s = format!("## Project Structure\n\n");
    let label = project_type_label(pt);
    s.push_str(&format!("Project type: **{}**\n\n", label));
    s.push_str("```\n");

    let patterns = source_globs(pt);
    for file in format_glob_files(patterns, 50, true) {
        s.push_str(&format!("{}\n", file));
    }

    // Add config files at root
    let config_files = [
        "Cargo.toml",
        "package.json",
        "pyproject.toml",
        "requirements.txt",
        "config.toml",
        ".env",
        "tsconfig.json",
        ".gitignore",
    ];
    for f in &config_files {
        if Path::new(f).exists() {
            s.push_str(&format!("{}\n", f));
        }
    }

    s.push_str("```\n\n");
    s
}

/// Common entry-point files
fn section_entry_points() -> String {
    let mut s = "## Entry Points\n\n".to_string();
    let entries = [
        "src/main.rs",
        "src/lib.rs",
        "src/index.ts",
        "src/index.js",
        "src/app.rs",
        "src/app.ts",
        "app.py",
        "main.py",
        "src/index.rs",
    ];
    let found: Vec<&str> = entries
        .iter()
        .filter(|e| Path::new(e).exists())
        .copied()
        .collect();
    if found.is_empty() {
        s.push_str("[No entry points detected]\n\n");
        return s;
    }
    for entry in &found {
        s.push_str(&format!("- `{}`\n", entry));
    }
    s.push('\n');
    s
}

/// Parse a single dependency value from Cargo.toml into a display version string
fn parse_cargo_dep_version(value: &toml::Value) -> String {
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

/// Format a Cargo dependency table as a sorted Markdown list
fn format_cargo_dep_table(deps: &toml::value::Table, skip_underscore: bool) -> String {
    let mut list: Vec<String> = deps
        .iter()
        .filter(|(name, _)| !(skip_underscore && name.starts_with('_')))
        .map(|(name, value)| format!("- **{}** v{}", name, parse_cargo_dep_version(value)))
        .collect();
    list.sort();
    list.join("\n")
}

/// Key dependencies and dev-dependencies
fn section_dependencies(
    cargo_toml: Option<&toml::Value>,
    package_json: Option<&serde_json::Value>,
    pyproject_toml: Option<&toml::Value>,
) -> String {
    let mut s = "## Key Dependencies\n\n".to_string();

    // Rust: Cargo.toml
    if let Some(toml) = cargo_toml {
        if let Some(deps) = toml.get("dependencies").and_then(|d| d.as_table()) {
            let list = format_cargo_dep_table(deps, true);
            if !list.is_empty() {
                s.push_str(&list);
                s.push_str("\n\n");
            }
        }
        if let Some(dev_deps) = toml.get("dev-dependencies").and_then(|d| d.as_table()) {
            if !dev_deps.is_empty() {
                s.push_str("### Dev Dependencies\n\n");
                s.push_str(&format_cargo_dep_table(dev_deps, false));
                s.push_str("\n\n");
            }
        }
        return s;
    }

    // JavaScript: package.json
    if let Some(pkg) = package_json {
        let mut deps_list: Vec<String> = Vec::new();
        if let Some(deps) = pkg.get("dependencies").and_then(|d| d.as_object()) {
            for (name, version) in deps {
                if let Some(ver) = version.as_str() {
                    deps_list.push(format!("- **{}**: \"{}\"", name, ver));
                }
            }
        }
        if !deps_list.is_empty() {
            deps_list.sort();
            s.push_str(&deps_list.join("\n"));
            s.push_str("\n\n");
        }

        let mut dev_list: Vec<String> = Vec::new();
        if let Some(dd) = pkg.get("devDependencies").and_then(|d| d.as_object()) {
            for (name, version) in dd {
                if let Some(ver) = version.as_str() {
                    dev_list.push(format!("- **{}**: \"{}\"", name, ver));
                }
            }
        }
        if !dev_list.is_empty() {
            dev_list.sort();
            s.push_str("### Dev Dependencies\n\n");
            s.push_str(&dev_list.join("\n"));
            s.push_str("\n\n");
        }
        return s;
    }

    // Python: pyproject.toml
    if let Some(pyproject) = pyproject_toml {
        let mut deps_list: Vec<String> = Vec::new();
        // PEP 621: [project] dependencies
        if let Some(deps) = pyproject
            .get("project")
            .and_then(|p| p.get("dependencies"))
            .and_then(|d| d.as_array())
        {
            for dep in deps {
                if let Some(d) = dep.as_str() {
                    deps_list.push(format!("- {}", d));
                }
            }
        }
        // [tool.poetry.dependencies]
        if deps_list.is_empty() {
            if let Some(deps) = pyproject
                .get("tool")
                .and_then(|t| t.get("poetry"))
                .and_then(|p| p.get("dependencies"))
                .and_then(|d| d.as_table())
            {
                for (name, value) in deps {
                    let ver = match value {
                        toml::Value::String(v) => v.clone(),
                        toml::Value::Table(t) => t
                            .get("version")
                            .and_then(|v| v.as_str())
                            .unwrap_or("*")
                            .to_string(),
                        _ => "*".to_string(),
                    };
                    deps_list.push(format!("- **{}**: {}", name, ver));
                }
            }
        }
        if !deps_list.is_empty() {
            deps_list.sort();
            s.push_str(&deps_list.join("\n"));
            s.push_str("\n\n");
        }
        return s;
    }

    s.push_str("[No dependency information detected]\n\n");
    s
}

/// Detect configuration files in the project
fn section_configuration() -> String {
    let mut s = "## Configuration\n\n".to_string();
    let config_patterns = [
        "config.toml",
        "config.json",
        "config.yaml",
        "config.yml",
        ".env",
        ".env.example",
        "tsconfig.json",
        "eslint.config.js",
        ".prettierrc",
        "rustfmt.toml",
        "clippy.toml",
        "mypy.ini",
        "pytest.ini",
        "setup.cfg",
        "docker-compose.yml",
        "docker-compose.yaml",
    ];
    let found: Vec<&str> = config_patterns
        .iter()
        .filter(|p| Path::new(p).exists())
        .copied()
        .collect();
    if found.is_empty() {
        s.push_str("[No configuration files detected]\n\n");
        return s;
    }
    for f in &found {
        s.push_str(&format!("- `{}`\n", f));
    }
    s.push_str("\n");
    s
}

/// Test file listing (multi-language)
fn section_test_files(pt: ProjectType) -> String {
    let mut s = "## Tests\n\n".to_string();
    let patterns = test_globs(pt);
    let mut test_files: Vec<String> = Vec::new();
    for pattern in patterns {
        let Ok(entries) = glob::glob(pattern) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.to_string_lossy().to_string();
            if path.contains("target/") {
                continue;
            }
            if !test_files.contains(&path) {
                test_files.push(path);
            }
        }
    }
    test_files.sort();

    if test_files.is_empty() {
        s.push_str("[No test files found]\n\n");
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
    let pt = detect_project_type();

    if Path::new(".gitignore").exists() {
        conventions.push("- Project uses `.gitignore` for version control");
    }
    if Path::new(".github/workflows").exists() {
        conventions.push("- CI/CD workflows in `.github/workflows/`");
    }
    if Path::new("Dockerfile").exists() {
        conventions.push("- Dockerized — `Dockerfile` present");
    }
    if Path::new("rustfmt.toml").exists() || Path::new(".rustfmt.toml").exists() {
        conventions.push("- Rust formatting rules defined");
    }
    if Path::new("clippy.toml").exists() {
        conventions.push("- Clippy lint rules configured");
    }
    if Path::new(".prettierrc").exists() || Path::new("eslint.config.js").exists() {
        conventions.push("- JavaScript/TypeScript formatting and linting configured");
    }

    // Language-specific conventions
    match pt {
        ProjectType::Rust => {
            if Path::new("src/lib.rs").exists() {
                conventions.push("- Library crate with `src/lib.rs` as root");
            }
            if Path::new("tests/").is_dir() {
                conventions.push("- Integration tests in `tests/` directory");
            }
        }
        ProjectType::JavaScript => {
            if Path::new("tsconfig.json").exists() {
                conventions.push("- TypeScript configuration via `tsconfig.json`");
            }
        }
        ProjectType::Python => {
            if Path::new("pyproject.toml").exists() {
                conventions.push("- Python project metadata in `pyproject.toml`");
            }
        }
        ProjectType::Unknown => {}
    }

    if conventions.is_empty() {
        conventions.push("- [Add important conventions here]");
    }
    format!("## Conventions & Gotchas\n\n{}\n", conventions.join("\n"))
}
