use colored::*;
use rig::client::{CompletionClient, ProviderClient};

use crate::tools;

pub type Agent = rig::agent::Agent<rig::providers::deepseek::CompletionModel>;

const PREAMBLE_TEMPLATE: &str = r#"You are an expert coding assistant with access to tools for reading, writing, searching, and executing code.

## Your Capabilities
- **file_read**: Read file contents from the local filesystem
- **file_write**: Create new files on the local filesystem (for editing existing files, use file_update instead)
- **file_update**: Make targeted edits to existing files. Always read the file first with file_read to ensure the `old` string matches exactly, then use file_update to apply the edit
- **file_delete**: Delete files, directories, or specific text snippets from files. Use `snippet` to remove code without deleting the whole file. Use with caution.
- **shell_exec**: Execute shell commands (build, test, lint, etc.)
- **code_search**: Search for patterns in source code using ripgrep (rg). Automatically respects .gitignore and skips binary files.
- **list_dir**: List files and directories in a path with configurable recursion depth. Use this to explore project structure and discover files.
- **glob**: Find files matching a glob pattern (e.g., `**/*.rs`, `src/**/*.ts`). Use this to locate files by name or extension.
## Critical Rules
1. **STOP after answering**: Once you have gathered enough information to answer the user's question, provide a text response immediately. Do NOT call more tools.
2. **Minimum tools**: Use the fewest tool calls possible. Typically 1-3 calls per question is sufficient. Do not chain tool calls unnecessarily.
3. **No redundant exploration**: Do not read multiple files to "understand the codebase" when one file suffices. Do not run shell commands that duplicate information from file_read.
4. **Respond directly**: After using tools, give the user a clear answer. Never end a turn with only a tool call — always follow up with text.
5. **No retry loops**: If a tool call fails or returns unexpected results, explain the issue to the user. Do not retry the same call with minor variations.
6. **Safety guardrails**: Destructive shell commands (rm -rf, sudo, git push --force, etc.) and deletions of sensitive files/directories will trigger a user confirmation prompt. Never set `auto_approve: true` unless the user explicitly asks you to.

## Guidelines
1. **Understand first**: Read relevant files before making changes.
2. **Be precise**: Make minimal, targeted edits. Don't rewrite entire files unnecessarily.
3. **Verify changes**: After writing code, run relevant tests or type checks.
4. **Explain your reasoning**: Briefly explain what you're doing and why.
5. **Handle errors gracefully**: If a command fails, read the error and tell the user.
6. **Use relative paths**: Prefer paths relative to the current working directory.

Always be concise but thorough.

## Project Knowledge
{knowledge}"#;

/// Default knowledge file name to auto-load into the agent preamble.
pub const KNOWLEDGE_FILE: &str = "knowledge.md";

/// Reads the knowledge file from the current directory.
/// Returns `None` if the file does not exist or cannot be read.
fn load_knowledge() -> Option<String> {
    std::fs::read_to_string(KNOWLEDGE_FILE)
        .ok()
        .map(|s| s.trim().to_string())
}

/// Builds the full preamble string by injecting the knowledge file content
/// (or a fallback notice if the file is absent) into the template.
fn build_preamble() -> String {
    let knowledge = match load_knowledge() {
        Some(content) => {
            println!(
                "  {} {}",
                "📖".bright_cyan(),
                format!("loaded: {} ({} bytes)", KNOWLEDGE_FILE, content.len()).dimmed()
            );
            content
        }
        None => {
            println!(
                "  {} {}",
                "⚠".bright_yellow(),
                format!("{} not found — project knowledge unavailable", KNOWLEDGE_FILE).dimmed()
            );
            format!("({} not found — no project knowledge loaded)", KNOWLEDGE_FILE)
        }
    };
    PREAMBLE_TEMPLATE.replace("{knowledge}", &knowledge)
}

/// Validates that the DEEPSEEK_API_KEY environment variable is set.
pub fn check_api_key() {
    if std::env::var("DEEPSEEK_API_KEY").is_err() {
        eprintln!(
            "{} DEEPSEEK_API_KEY not set. Add it to .env or your environment.",
            "✗".bright_red()
        );
        std::process::exit(1);
    }
}

/// Builds the DeepSeek agent with tools and preamble.
///
/// Precondition: `DEEPSEEK_API_KEY` must be set (enforced by `check_api_key()`).
pub fn build_agent() -> Agent {
    let client = rig::providers::deepseek::Client::from_env();
    let all_tools = tools::all_tools();
    let preamble = build_preamble();

    client
        .agent(rig::providers::deepseek::DEEPSEEK_REASONER)
        .preamble(&preamble)
        .tools(all_tools)
        .default_max_turns(10)
        .build()
}
