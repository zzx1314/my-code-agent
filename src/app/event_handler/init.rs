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
