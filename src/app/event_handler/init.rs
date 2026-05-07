use crate::app::InitResult;
use crate::core::preamble::Agent;
use crate::tools::create_mcp_tools;

/// Build the LLM prompt for `/init` command
pub fn build_init_prompt(is_update: bool) -> String {
    if is_update {
        let existing_content =
            std::fs::read_to_string(crate::core::preamble::KNOWLEDGE_FILE).unwrap_or_default();
        format!(
            r#"You are a technical documentation expert. Your task is to UPDATE the project knowledge document.

Current knowledge document:
```markdown
{}
```

## Instructions
1. Use the available tools (list_dir, glob, file_read, code_search) to explore the current project structure
2. Check the project root for README.md, Cargo.toml, package.json, or similar config files
3. Look at the source directory structure to understand the codebase layout
4. Update the knowledge document to accurately reflect the current state of the project
5. Keep the existing Markdown structure but update all content
6. Add any new important files, modules, or patterns you discover
7. Remove references to files or features that no longer exist

## Output Rules
- Respond ONLY with the complete updated Markdown content
- Do NOT include any explanation, commentary, or wrapping text
- Do NOT use code fences around your response
- The response should be valid Markdown that can be directly saved as a file"#,
            existing_content
        )
    } else {
        r#"You are a technical documentation expert. Your task is to CREATE a comprehensive project knowledge document.

## Instructions
1. Use the available tools (list_dir, glob, file_read, code_search) to thoroughly explore the project
2. Read README.md, Cargo.toml/package.json, and other config files in the project root
3. Explore the source directory structure (src/, lib/, etc.)
4. Identify the project type, key dependencies, architecture patterns, and conventions
5. Create a well-structured Markdown knowledge document

## Document Structure
The document should include these sections:
- **## What This Is** — Brief project description (from README or code)
- **## Features** — Key features and capabilities
- **## Project Structure** — Directory/file layout with descriptions
- **## Key Dependencies** — Major libraries and their purposes
- **## Configuration** — How the project is configured
- **## Conventions & Gotchas** — Important patterns, naming conventions, things to know

## Output Rules
- Respond ONLY with the complete Markdown content
- Do NOT include any explanation, commentary, or wrapping text
- Do NOT use code fences around your response
- The response should be valid Markdown that can be directly saved as a file"#.to_string()
    }
}

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
    use crate::core::preamble::build_agent;
    let mcp_tools = futures::executor::block_on(create_mcp_tools(config));
    Ok(build_agent(config, mcp_tools))
}

/// Local fallback for knowledge generation (no LLM)
pub fn generate_knowledge_content_local() -> String {
    let mut content = String::new();
    content.push_str("# Project Knowledge\n\n");

    // === What This Is ===
    content.push_str("## What This Is\n");
    if let Ok(readme) = std::fs::read_to_string("README.md") {
        // Try to extract the first meaningful paragraph after the title
        let meaningful: String = readme
            .lines()
            .skip_while(|line| line.starts_with('#') || line.trim().is_empty())
            .take_while(|line| !line.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        if !meaningful.is_empty() {
            content.push_str(&meaningful);
            content.push_str("\n\n");
        } else {
            content.push_str("[Project description from README.md]\n\n");
        }
    } else {
        content.push_str("[Describe your project here]\n\n");
    }

    // === Project Structure ===
    content.push_str("## Project Structure\n\n");
    content.push_str("```\n");
    if let Ok(entries) = glob::glob("**/*.rs") {
        let mut files: Vec<String> = entries
            .filter_map(|e| e.ok())
            .filter(|e| !e.to_string_lossy().contains("target/"))
            .map(|e| e.to_string_lossy().to_string())
            .collect();
        files.sort();
        files.dedup();
        for file in files.iter().take(40) {
            content.push_str(&format!("{}\n", file));
        }
        if files.len() > 40 {
            content.push_str(&format!("... ({} more files)\n", files.len() - 40));
        }
    }
    content.push_str("```\n\n");

    // === Entry Points ===
    content.push_str("## Entry Points\n\n");
    let entry_files = ["src/main.rs", "src/lib.rs", "src/index.rs", "src/app.rs"];
    for entry in &entry_files {
        if std::path::Path::new(entry).exists() {
            content.push_str(&format!("- `{}`\n", entry));
        }
    }
    content.push_str("\n");

    // === Key Dependencies ===
    content.push_str("## Key Dependencies\n\n");
    if let Ok(cargo_content) = std::fs::read_to_string("Cargo.toml") {
        if let Ok(cargo_toml) = cargo_content.parse::<toml::Value>() {
            // Main dependencies
            if let Some(deps) = cargo_toml.get("dependencies").and_then(|d| d.as_table()) {
                let mut dep_list: Vec<String> = deps
                    .iter()
                    .filter(|(name, _)| !name.starts_with('_')) // skip internal path deps
                    .map(|(name, value)| {
                        let version = match value {
                            toml::Value::String(v) => v.clone(),
                            toml::Value::Table(t) => {
                                let ver = t.get("version").and_then(|v| v.as_str()).unwrap_or("*");
                                let features = t
                                    .get("features")
                                    .and_then(|f| f.as_array())
                                    .map(|arr| {
                                        let feats: Vec<&str> =
                                            arr.iter().filter_map(|v| v.as_str()).collect();
                                        format!(" (features: {})", feats.join(", "))
                                    })
                                    .unwrap_or_default();
                                format!("{}{}", ver, features)
                            }
                            _ => "*".to_string(),
                        };
                        format!("- **{}** v{}", name, version)
                    })
                    .collect();
                dep_list.sort();
                content.push_str(&dep_list.join("\n"));
                content.push_str("\n\n");
            }
            // Dev dependencies
            if let Some(dev_deps) = cargo_toml
                .get("dev-dependencies")
                .and_then(|d| d.as_table())
            {
                if !dev_deps.is_empty() {
                    content.push_str("### Dev Dependencies\n\n");
                    let mut dev_list: Vec<String> = dev_deps
                        .iter()
                        .map(|(name, value)| {
                            let version = match value {
                                toml::Value::String(v) => v.clone(),
                                _ => "*".to_string(),
                            };
                            format!("- **{}** v{}", name, version)
                        })
                        .collect();
                    dev_list.sort();
                    content.push_str(&dev_list.join("\n"));
                    content.push_str("\n\n");
                }
            }
        }
    }

    // === Rust Edition & Features ===
    if let Ok(cargo_content) = std::fs::read_to_string("Cargo.toml") {
        if let Ok(cargo_toml) = cargo_content.parse::<toml::Value>() {
            if let Some(package) = cargo_toml.get("package") {
                let mut meta_parts = Vec::new();
                if let Some(edition) = package.get("edition").and_then(|e| e.as_str()) {
                    meta_parts.push(format!("Rust edition: {}", edition));
                }
                if let Some(name) = package.get("name").and_then(|n| n.as_str()) {
                    meta_parts.push(format!("Crate name: {}", name));
                }
                if !meta_parts.is_empty() {
                    content.push_str("## Project Metadata\n\n");
                    for part in &meta_parts {
                        content.push_str(&format!("- {}\n", part));
                    }
                    content.push_str("\n");
                }
            }
        }
    }

    // === Test Files ===
    content.push_str("## Tests\n\n");
    if let Ok(entries) = glob::glob("tests/**/*.rs") {
        let mut test_files: Vec<String> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.to_string_lossy().to_string())
            .collect();
        test_files.sort();
        if test_files.is_empty() {
            content.push_str("[No test files found in tests/]\n\n");
        } else {
            for file in test_files.iter().take(15) {
                content.push_str(&format!("- `{}`\n", file));
            }
            if test_files.len() > 15 {
                content.push_str(&format!(
                    "... ({} more test files)\n",
                    test_files.len() - 15
                ));
            }
            content.push_str("\n");
        }
    }

    // === Conventions ===
    content.push_str("## Conventions & Gotchas\n\n");
    // Auto-detect some conventions
    let mut conventions = Vec::new();
    if std::path::Path::new(".gitignore").exists() {
        conventions.push("- Project uses .gitignore for version control");
    }
    if std::path::Path::new("clippy.toml").exists() || std::path::Path::new("rustfmt.toml").exists()
    {
        conventions.push("- Clippy/rustfmt configuration present — follow formatting rules");
    }
    if std::path::Path::new(".github/workflows").exists() {
        conventions.push("- CI/CD workflows in `.github/workflows/`");
    }
    if conventions.is_empty() {
        conventions.push("- [Add important conventions here]");
    }
    for conv in &conventions {
        content.push_str(&format!("{}\n", conv));
    }

    content
}
