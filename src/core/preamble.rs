use rig::client::CompletionClient;

use crate::core::config::Config;
use crate::tools;
use rig::providers::openrouter;


pub const PREAMBLE_TEMPLATE: &str = r#"You are an expert coding assistant with access to tools for reading, writing, searching, and executing code.

## Task Planning
For multi-step tasks, start with a plan before executing:

**Before calling tools for a multi-step task, output your plan in this format:**

```
## Task Plan
1. [Step description]
2. [Step description]
3. ...
```

This plan helps:
- Organize your thoughts before acting
- Let the user verify your approach
- Track progress through complex tasks

**When to create a plan:**
- Implementing a new feature
- Refactoring multiple files
- Writing tests
- Complex debugging tasks
- Any task requiring 3+ tool calls

**When NOT to create a plan (skip for simplicity):**
- Simple questions (read 1 file, answer)
- Single tool calls (one file read, one search)
- Follow-up questions on recent context

**Plan Confirmation:**
- After presenting your plan, the system will prompt you for confirmation
- Press ENTER (or type y) to confirm and proceed with execution
- Type n to cancel the plan (returns to normal interaction)
- If the user cancels, acknowledge and abort the plan - do not execute any steps

## Your Capabilities
- **file_read**: Read file contents from the local filesystem. Returns up to 200 lines by default - use offset and limit to paginate through large files. If a file is truncated and you have not found the information you need, continue reading with offset rather than guessing based on partial content.
- **User file attachments (`@filepath`)**: Users can attach files inline using `@path` (e.g. `@src/main.rs`). The `@path:N` syntax is for users only - do not reference it in your own messages. Large files are truncated with a notice like `showing 500 of 1200 total lines. Use @src/main.rs:500 or the file_read tool with offset=500 to read the rest`. When you see this notice, use the `file_read` tool with the suggested offset to continue reading.
- **file_write**: Create new files on the local filesystem (for editing existing files, use file_update instead)
- **file_update**: Make targeted edits to existing files. Always read the file first with file_read to ensure the old string matches exactly, then use file_update to apply the edit
- **file_delete**: Delete files, directories, or specific text snippets from files. Use snippet to remove code without deleting the whole file. Use with caution.
- **shell_exec**: Execute shell commands (build, test, lint, etc.)
- **code_search**: Search for patterns in source code using ripgrep (rg). Automatically respects .gitignore and skips binary files.
- **list_dir**: List files and directories in a path with configurable recursion depth. Use this to explore project structure and discover files.
- **glob**: Find files matching a glob pattern (e.g. **/*.rs, src/**/*.ts). Use this to locate files by name or extension.
- **git_status**: Get structured git repository status. Returns modified, added, deleted, untracked files and current branch in JSON format. Use instead of `shell_exec` with `git status`.
- **git_diff**: Show git diff for files or entire repo. Returns diff output with optional line limit. Use instead of `shell_exec` with `git diff`.
- **git_log**: View commit history in structured format. Returns commits with hash, author, date, message. Use instead of `shell_exec` with `git log`.
- **git_commit**: Commit changes with a message. Includes safety confirmation. Use `git_status` first to check staged changes.
- **web_search**: Search the web using Parallel Search MCP. Use this tool when you need up-to-date information from the internet, current events, or facts not available in the local codebase. Returns search results with titles, URLs, and snippets.
- **web_fetch**: Extract content from a specific URL using Parallel Search MCP.
## Critical Rules
1. **STOP after answering**: Once you have gathered enough information to answer the user's question, provide a text response immediately. Do NOT call more tools.
2. **Minimum tools**: Use the fewest tool calls possible. Typically 1-3 calls per question is sufficient. Do not chain tool calls unnecessarily.
3. **No redundant exploration**: Do not read multiple files to understand the codebase when one file suffices. Do not run shell commands that duplicate information from file_read.
4. **Respond directly**: After using tools, give the user a clear answer. Never end a turn with only a tool call - always follow up with text.
5. **No retry loops**: If a tool call fails or returns unexpected results, explain the issue to the user. Do not retry the same call with minor variations.
6. **Safety guardrails**: Destructive shell commands (rm -rf, sudo, git push --force, etc.) and deletions of sensitive files/directories will trigger a user confirmation prompt. Never set auto_approve: true unless the user explicitly asks you to.
7. **Read fully before modifying**: Before using file_update or file_write on an existing file, you MUST have read the complete file content. If file_read returns `truncated: true`, or if a user-attached `@filepath` shows a truncation notice, continue reading with offset until you have seen every line. Never edit a file you have not fully read - partial knowledge leads to incorrect edits.

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

pub const KNOWLEDGE_FILE: &str = "knowledge.md";

pub type OpenRouterAgent = rig::agent::Agent<openrouter::CompletionModel>;


fn load_knowledge() -> Option<String> {
    std::fs::read_to_string(KNOWLEDGE_FILE)
        .ok()
        .map(|s| s.trim().to_string())
}

fn build_preamble() -> String {
    let knowledge = match load_knowledge() {
        Some(content) => {
            eprintln!(
                "[knowledge] loaded: {} ({} bytes)",
                KNOWLEDGE_FILE,
                content.len()
            );
            content
        }
        None => {
            eprintln!(
                "[warn] {} not found - project knowledge unavailable",
                KNOWLEDGE_FILE
            );
            format!(
                "({} not found - no project knowledge loaded)",
                KNOWLEDGE_FILE
            )
        }
    };
    PREAMBLE_TEMPLATE.replace("{knowledge}", &knowledge)
}

fn check_api_key(provider_name: &str, api_key_env: &str) {
    if std::env::var(api_key_env).is_err() {
        eprintln!(
            "[error] {} not set. Add it to .env or your environment.",
            api_key_env
        );
        std::process::exit(1);
    }
    eprintln!("[ok] {} ({})", api_key_env, provider_name);
}

/// Supported LLM providers
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Provider {
    DeepSeek,
    OpenAI,
    Anthropic,
    Cohere,
    OpenRouter,
    Custom,
}

impl Provider {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "deepseek" => Some(Provider::DeepSeek),
            "openai" => Some(Provider::OpenAI),
            "anthropic" => Some(Provider::Anthropic),
            "cohere" => Some(Provider::Cohere),
            "openrouter" => Some(Provider::OpenRouter),
            "custom" => Some(Provider::Custom),
            _ => None,
        }
    }

    pub fn default_model(&self) -> &'static str {
        match self {
            Provider::DeepSeek => "deepseek-chat",
            Provider::OpenAI => "gpt-4o",
            Provider::Anthropic => "claude-3-5-sonnet-20241022",
            Provider::Cohere => "command-r-plus",
            Provider::OpenRouter => "nvidia/nemotron-3-super-120b-a12b:free",
            Provider::Custom => "gpt-4o",
        }
    }

    pub fn default_api_key_env(&self) -> &'static str {
        match self {
            Provider::DeepSeek => "DEEPSEEK_API_KEY",
            Provider::OpenAI => "OPENAI_API_KEY",
            Provider::Anthropic => "ANTHROPIC_API_KEY",
            Provider::Cohere => "COHERE_API_KEY",
            Provider::OpenRouter => "OPENROUTER_API_KEY",
            Provider::Custom => "OPENAI_API_KEY",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Provider::DeepSeek => "DeepSeek",
            Provider::OpenAI => "OpenAI",
            Provider::Anthropic => "Anthropic",
            Provider::Cohere => "Cohere",
            Provider::OpenRouter => "OpenRouter",
            Provider::Custom => "Custom",
        }
    }
}

/// Agent type alias - uses OpenAI Completions API for compatibility with custom endpoints.
pub enum Agent {
    OpenAI(rig::agent::Agent<rig::providers::openai::CompletionModel>),
    OpenRouter(rig::agent::Agent<openrouter::CompletionModel>),
}
/// Builds an agent using the configured LLM provider.
/// Uses OpenAI Completions API client for compatibility with custom endpoints.
pub fn build_agent(config: &Config, mcp_tools: Vec<Box<dyn rig::tool::ToolDyn>>) -> Agent {
    let provider = Provider::from_str(&config.llm.provider).unwrap_or(Provider::DeepSeek);
    let model = config
        .llm
        .model
        .as_deref()
        .unwrap_or(provider.default_model())
        .to_string();
    let api_key_env = if config.llm.api_key_env.is_empty() {
        provider.default_api_key_env()
    } else {
        &config.llm.api_key_env
    };

    check_api_key(provider.display_name(), api_key_env);

    let preamble = build_preamble();
    let mut all_tools = tools::all_tools(config);
    
    // Add MCP tools
    for tool in mcp_tools {
        all_tools.push(tool);
    }

    match provider {
        Provider::DeepSeek => {
            eprintln!("[model] {} (DeepSeek)", model);
            create_openai_client(
                api_key_env,
                "https://api.deepseek.com/v1",
                &preamble,
                &model,
                all_tools,
                config.agent.max_turns,
            )
        }
        Provider::OpenRouter => {
            eprintln!("[model] {} (OpenRouter)", model);
            let api_key = std::env::var(api_key_env).unwrap_or_default();
            let client = openrouter::Client::new(&api_key).expect("Failed to create OpenRouter client");
            
            return Agent::OpenRouter(
                client
                    .agent(&model)
                    .preamble(&preamble)
                    .tools(all_tools)
                    .default_max_turns(config.agent.max_turns)
                    .build(),
            );
        }
        Provider::Custom => {
            let base_url = match config.llm.base_url.as_ref() {
                Some(url) => url.clone(),
                None => {
                    eprintln!("[error] Custom provider requires base_url in config.toml");
                    eprintln!("Example:");
                    eprintln!("  [llm]");
                    eprintln!("  provider = \"custom\"");
                    eprintln!("  model = \"llama3\"");
                    eprintln!("  base_url = \"http://localhost:11434/v1\"");
                    std::process::exit(1);
                }
            };
            eprintln!("[model] {} (Custom: {})", model, base_url);
            create_openai_client(
                api_key_env,
                &base_url,
                &preamble,
                &model,
                all_tools,
                config.agent.max_turns,
            )
        }
        _ => {
            eprintln!(
                "[warn] Provider '{}' not fully implemented, using DeepSeek",
                provider.display_name()
            );
            create_openai_client(
                api_key_env,
                "https://api.deepseek.com/v1",
                &preamble,
                &model,
                all_tools,
                config.agent.max_turns,
            )
        }
    }
}

/// Creates an OpenAI-compatible client using the builder pattern with explicit base_url.
fn create_openai_client(
    api_key_env: &str,
    base_url: &str,
    preamble: &str,
    model: &str,
    all_tools: Vec<Box<dyn rig::tool::ToolDyn>>,
    max_turns: usize,
) -> Agent {  // 返回你的 Agent 枚举
    let api_key = std::env::var(api_key_env).unwrap_or_default();
    let client = rig::providers::openai::CompletionsClient::builder()
        .api_key(&api_key)
        .base_url(base_url)
        .build()
        .expect("Failed to create OpenAI client");

    Agent::OpenAI(  // ← 包装进枚举
        client
            .agent(model)
            .preamble(preamble)
            .tools(all_tools)
            .default_max_turns(max_turns)
            .build()
    )
}
