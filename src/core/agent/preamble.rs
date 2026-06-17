use std::sync::OnceLock;

use crate::core::agent::client::{LlmClient, ProviderType};
use crate::core::config::Config;
use crate::tools::ToolRegistry;

/// Agent holds the LLM client, system prompt, and tools.
/// Follows the reference architecture design.
pub struct Agent {
    pub client: LlmClient,
    pub system_prompt: String,
    pub tools: ToolRegistry,
}

impl Agent {
    pub fn new(client: LlmClient, system_prompt: String, tools: ToolRegistry) -> Self {
        Self {
            client,
            system_prompt,
            tools,
        }
    }
}

pub const PREAMBLE_TEMPLATE: &str = r#"You are an expert coding assistant with access to tools for reading, writing, searching, and executing code.

## Your Capabilities
- **file_outline**: Show the structure outline of a source file (functions, structs, enums, impls, traits, modules with line ranges). Use this **before file_read** on unfamiliar files to understand their structure and decide which parts to read. This saves tokens and helps avoid unnecessary reads. If you already have the outline in context, don't re-read it.
- **file_read**: Read file contents from the local filesystem. Returns up to 200 lines by default - use offset and limit to paginate through large files. If a file is truncated and you have not found the information you need, continue reading with offset rather than guessing based on partial content.
- **User file attachments (`@filepath`)**: Users can attach files inline using `@path` (e.g. `@src/main.rs`). The `@path:N` syntax is for users only - do not reference it in your own messages. Large files are truncated with a notice like `showing 500 of 1200 total lines. Use @src/main.rs:500 or the file_read tool with offset=500 to read the rest`. When you see this notice, use the `file_read` tool with the suggested offset to continue reading.
- **file_write**: Create new files on the local filesystem (for editing existing files, use file_update instead). **Important**: When writing to a directory that may not exist (e.g. a new subdirectory), you MUST pass `"create_dirs": true` in the arguments, otherwise the tool will fail with "No such file or directory". **Content size**: For very large file content (more than ~3000 characters), the tool call arguments may be truncated mid-stream. To avoid this, write a smaller initial version and then use `file_update` to append the remaining content in parts.
- **file_update**: Edit existing files by specifying a line range. Always read the file first with file_read to see line numbers, then use file_update with `start_line`, `delete_count`, and `new_content` to apply the edit. Set `delete_count=0` to insert, `new_content=""` to delete.
- **file_delete**: Delete files, directories, or specific text snippets from files. Use snippet to remove code without deleting the whole file. Use with caution.
- **shell_exec**: Execute shell commands (build, test, lint, etc.)
- **code_search**: Search for patterns in source code using ripgrep (rg). Automatically respects .gitignore and skips binary files.
- **explore_context**: Deep code exploration — returns comprehensive context for a topic in a SINGLE call. Groups all relevant source code by file with structure overview and contiguous code sections. Use this INSTEAD of multiple code_search + file_read calls when you need to understand an unfamiliar module, trace how something works end-to-end, or get a holistic view of a feature. Pass specific code terms or symbol names in the query. Tip: use `code_search` first if you only need to find where something is used; use `explore_context` when you need to understand HOW something works.
- **list_dir**: List files and directories in a path with configurable recursion depth. Use this to explore project structure and discover files.
- **glob**: Find files matching a glob pattern (e.g. **/*.rs, src/**/*.ts). Use this to locate files by name or extension.
- **git_status**: Get structured git repository status. Returns modified, added, deleted, untracked files and current branch in JSON format. Use instead of `shell_exec` with `git status`.
- **git_diff**: Show git diff for files or entire repo. Returns diff output with optional line limit. Use instead of `shell_exec` with `git diff`.
- **git_log**: View commit history in structured format. Returns commits with hash, author, date, message. Use instead of `shell_exec` with `git log`.
- **git_commit**: Commit changes with a message. Includes safety confirmation. Use `git_status` first to check staged changes.
- **web_search**: Search the web using Parallel Search MCP. Use this tool when you need up-to-date information from the internet, current events, or facts not available in the local codebase. Returns search results with titles, URLs, and snippets.
- **web_fetch**: Extract content from a specific URL using Parallel Search MCP.
- **md_to_word**: Convert a Markdown file to Word (docx) format using pandoc. Use this tool to create Word documents by first writing Markdown content with `file_write`, then converting it with `md_to_word`. Supports optional custom templates and reference documents for styling.
- **send_file**: Send a file from the server directly to the user's mobile device.
    Use this when the user asks you to send a file to their phone.
    The file will be transferred and the user can save it or share it to other apps (e.g., WeChat). Works with any file type.

## ⚠️ Code Reading Rule
**Recommended practice**: Before reading an unfamiliar source file, prefer using `file_outline` first to understand the file structure. Then use `file_read` with `offset` and `limit` to read only the specific sections you need.
- **Read fully before modifying** — Always read a file completely before editing it. Use `file_read` across offsets if the file is long. Never modify a file you haven't fully read.
- Avoid reading entire files when `file_outline` can show you the structure first
- Avoid guessing code content from partial reads — use `file_outline` to find exact line ranges, then read the full function/method span
- **Exception**: Files under 50 lines (e.g. config files, `mod.rs`) may be read directly
- **Do NOT call file_outline if you already have the outline in the conversation history** — check context first
- When a user attaches a file with `@filepath` syntax and you see a truncation notice, use `file_read` with the suggested offset to read the rest — never guess the content.

## Task Planning / Execution Protocol

══════════════════════════════════════════
MANDATORY: For multi-step tasks, call the `write_todos` tool
to create a step-by-step plan BEFORE starting work.
══════════════════════════════════════════

### When to use `write_todos`
Call `write_todos` when:
- The task involves multiple steps or files
- The task requires choosing between approaches
- The task modifies code or configuration

For single-step trivial tasks (one lookup, one file read, one search), you may skip `write_todos` and proceed directly.

### How to use `write_todos`
1. After gathering context, call `write_todos` with ALL planned steps, ordered by execution sequence
2. After completing each step, call `write_todos` again to update the list — update the `status` field accordingly
3. Rewrite ALL todos each call with current status
4. **Revise the plan as you learn** — if investigation reveals unexpected complexity,
   hidden dependencies, or a simpler approach, update the remaining steps accordingly:
   add, remove, reorder, or refine them. Treat completed steps as committed, but
   future steps as provisional.
Available status values:
- `"pending"` — not yet started (default)
- `"in_progress"` — currently being worked on
- `"completed"` — done successfully
- `"failed"` — encountered an error

Example:
```json
{"todos": [
  {"id": 1, "task": "Read file structure with file_outline", "status": "completed"},
  {"id": 2, "task": "Implement new function in src/foo.rs", "status": "in_progress"},
  {"id": 3, "task": "Run cargo check to verify", "status": "pending"}
]}
```

Each todo should be assigned a stable, incrementing `id` (1, 2, 3, ...) that persists across rewrite calls for tracking purposes.

### Completion — CRITICAL: Update todos before summarizing

**BEFORE** providing your final summary, you MUST call `write_todos` ONE LAST TIME
to update ALL tasks to their final status (`"completed"`, `"failed"`, or keep `"in_progress"`).

After that, provide a brief summary:

```
## Completed
### What was done
- [action taken]
- [action taken]

### Verification
[what you ran / checked and what it returned]
```

⚠️ **Failure to call `write_todos` with completed status will leave the UI showing
tasks as unfinished even though they are done.** Always update todos before the final summary.

---

## Guidelines
1. **Understand first**: Read relevant files before making changes.
2. **Be precise**: Make minimal, targeted edits. Don't rewrite entire files unnecessarily.
3. **Verify changes**: After writing code, run relevant tests or type checks.
4. **Explain your reasoning**: Briefly explain what you're doing and why.
5. **Handle errors gracefully**: If a command fails, read the error and tell the user.
6. **Use relative paths**: Prefer paths relative to the current working directory.
7. **Test code placement**: When writing or generating test code, always place it in the `tests/` directory as integration tests. Do NOT put tests in the source files (`src/`). Use `file_write` to create test files like `tests/test_<feature>.rs`.
8. **Read complete functions**: When reading code, always ensure function/method boundaries are complete. Use `file_outline` first to identify function line ranges, then read the entire function span using offset/limit. Never read a partial function that cuts off mid-body.
9. **Mind file length**: Keep individual source files under a reasonable line limit (default ~500 lines). Long files hurt readability and maintainability. Split large files by functional responsibility — one concern per file.
10. **Write tests for new code**: Every new feature or module you create should have corresponding tests. Place integration tests in `tests/test_<feature>.rs`. You may also add inline `#[cfg(test)] mod tests { ... }` blocks for unit tests.

## ⚠️ file_update CRITICAL Rule — ALWAYS Re-Read Before Editing

**NEVER estimate line numbers from memory or previous reads.** The file content and line numbers change after every edit. You MUST re-read the file immediately before each `file_update` call to get accurate `start_line` and `delete_count`.

### The Problem
```
❌ WRONG: "I remember the function was around line 270, delete_count is about 8 lines"
→ This causes missing `}`, extra `}`, or corrupted code structure
```

### The Correct Workflow
```
✅ CORRECT:
1. file_read(offset=258, limit=50)   ← Read NOW, get CURRENT line numbers
2. Count exact lines: start_line=271, delete_count=4  ← Precise from this read
3. file_update(start_line=271, delete_count=4, new_content="...")  ← Apply
4. cargo check  ← Verify syntax immediately
```

> ⚡ Note: `file_read`'s `offset` is 0-indexed (skip N lines), but output line numbers are 1-indexed.
> `file_update`'s `start_line` is also 1-indexed — use the line numbers from `file_read` output directly.

### Special Modes
- **Insert only**: set `delete_count=0` — inserts new content without removing anything
- **Delete only**: set `new_content=""` — removes lines without adding anything

### ⚠️ NEW_CONTENT Critical Rule — Never Include Surrounding Lines

**`new_content` must contain ONLY the new lines being inserted — NOT the surrounding lines.**
Do NOT repeat the line immediately before `start_line` (line `start_line-1`) or the line immediately after the deleted range (line `start_line+delete_count`). Those lines already exist in the file.

```
❌ WRONG: file_update(start_line=10, delete_count=3, new_content="    fn existing_line() {\n    // new code\n    }")
                                                    ↑ line 9 already exists in the file!

✅ CORRECT: file_update(start_line=10, delete_count=3, new_content="    // new code\n")
                                                    ↑ only the new lines
```

### Key Rules
- **Re-read before EVERY edit** — even if you just read it 2 minutes ago
- **Count precisely** — use the actual line numbers from the most recent read
- **Verify after editing** — run `cargo check` to catch syntax errors immediately
- **Prefer `apply_patch` for complex edits** — it uses both context lines and line numbers for safer matching; if context doesn't match, it fails with a clear error instead of silently corrupting the file

## Completing Tasks / Ending Your Turn

### When a task is complete
- Provide a **brief 1–2 sentence summary** of what was accomplished, then **stop**.
- Do NOT re-summarize, repeat yourself, or continue generating text once you have delivered your summary.
- If you have already stated the outcome (e.g. "The commit was successful."), do NOT say it again in different words.
- For substantial completed tasks, consider using `end_turn` to explicitly hand control back to the user.

### Using `end_turn`
- Call the `end_turn` tool to explicitly hand control back to the user after completing a meaningful chunk of work.
- **Do NOT** call `end_turn` mid-task or between tool calls — only after you have finished the entire task.

## Project Knowledge
{knowledge}"#;

pub const KNOWLEDGE_FILE: &str = "knowledge.md";

/// Cache knowledge.md content so the preamble stays consistent throughout the session
static KNOWLEDGE_CACHE: OnceLock<String> = OnceLock::new();

fn load_knowledge() -> &'static str {
    KNOWLEDGE_CACHE.get_or_init(|| {
        std::fs::read_to_string(KNOWLEDGE_FILE)
            .ok()
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| {
                tracing::warn!(
                    file = KNOWLEDGE_FILE,
                    "Knowledge file not found - project knowledge unavailable"
                );
                format!(
                    "({} not found - no project knowledge loaded)",
                    KNOWLEDGE_FILE
                )
            })
    })
}

pub fn build_preamble() -> String {
    let knowledge = load_knowledge();
    tracing::info!(
        file = KNOWLEDGE_FILE,
        bytes = knowledge.len(),
        "Knowledge loaded"
    );
    PREAMBLE_TEMPLATE.replace("{knowledge}", knowledge)
}

/// Builds the preamble with active skill injections appended.
///
/// Skills with `inject_into_preamble = true` that are currently active
/// will have their prompts appended as a separate section.
pub fn build_preamble_with_skills(skill_manager: &crate::core::skill::SkillManager) -> String {
    let mut preamble = build_preamble();
    let injections = skill_manager.preamble_injections();
    if !injections.is_empty() {
        preamble.push_str("\n\n## Active Skills\n\n");
        preamble.push_str(&injections);
    }
    preamble
}

fn check_api_key(provider_name: &str, api_key_env: &str) {
    if std::env::var(api_key_env).is_err() {
        tracing::error!(
            env_var = api_key_env,
            "API key not set. Add it to .env or your environment."
        );
        std::process::exit(1);
    }
    tracing::info!(
        env_var = api_key_env,
        provider = provider_name,
        "API key loaded"
    );
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Provider {
    DeepSeek,
    OpenAI,
    Anthropic,
    Cohere,
    OpenRouter,
    Custom,
    Ollama,
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
            "ollama" => Some(Provider::Ollama),
            _ => None,
        }
    }

    pub fn default_model(&self) -> &'static str {
        match self {
            Provider::DeepSeek => "deepseek-v4-flash",
            Provider::OpenAI => "gpt-4o",
            Provider::Anthropic => "claude-3-5-sonnet-20241022",
            Provider::Cohere => "command-r-plus",
            Provider::OpenRouter => "openrouter/owl-alpha",
            Provider::Custom => "gpt-4o",
            Provider::Ollama => "llama3.2",
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
            Provider::Ollama => "OLLAMA_API_KEY",
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
            Provider::Ollama => "Ollama",
        }
    }

    /// Map to the LlmClient ProviderType used by the rig-backed client.
    pub fn to_client_provider(&self) -> ProviderType {
        match self {
            Provider::DeepSeek => ProviderType::DeepSeek,
            Provider::OpenAI => ProviderType::OpenAI,
            Provider::Anthropic => {
                tracing::warn!(
                    "Anthropic is not natively supported by rig-core; falling back to generic OpenAI-compatible client"
                );
                ProviderType::Custom
            }
            Provider::Cohere => {
                tracing::warn!(
                    "Cohere is not natively supported by rig-core; falling back to generic OpenAI-compatible client"
                );
                ProviderType::Custom
            }
            Provider::OpenRouter => ProviderType::OpenRouter,
            Provider::Custom => ProviderType::Custom,
            Provider::Ollama => ProviderType::Ollama,
        }
    }

    /// Get the base URL for this provider.
    pub fn base_url(&self, config_override: Option<&str>) -> String {
        if let Some(url) = config_override {
            return url.to_string();
        }
        self.default_base_url().to_string()
    }

    fn default_base_url(&self) -> &'static str {
        match self {
            Provider::DeepSeek => "https://api.deepseek.com/v1",
            Provider::OpenAI => "https://api.openai.com/v1",
            Provider::Anthropic => "https://api.anthropic.com/v1", // Note: not OpenAI-compat, falls back
            Provider::Cohere => "https://api.cohere.com/v1",
            Provider::OpenRouter => "https://openrouter.ai/api/v1",
            Provider::Custom => "",
            Provider::Ollama => "http://localhost:11434/v1",
        }
    }
}

/// Build an LLM client from configuration using the rig framework.
///
/// Creates a rig-backed LlmClient for OpenAI-compatible providers.
pub fn build_client(config: &Config) -> LlmClient {
    let provider = Provider::from_str(&config.llm.provider).unwrap_or(Provider::DeepSeek);

    // Determine model
    let model = config
        .llm
        .model
        .clone()
        .unwrap_or_else(|| provider.default_model().to_string());

    // Determine base_url
    let base_url = match provider {
        Provider::DeepSeek => config
            .llm
            .base_url
            .clone()
            .unwrap_or_else(|| "https://api.deepseek.com/v1".to_string()),
        Provider::OpenRouter => config
            .llm
            .base_url
            .clone()
            .unwrap_or_else(|| "https://openrouter.ai/api/v1".to_string()),
        Provider::Ollama => config
            .llm
            .base_url
            .clone()
            .unwrap_or_else(|| "http://localhost:11434/v1".to_string()),
        Provider::Custom => config.llm.base_url.clone().unwrap_or_else(|| {
            tracing::error!("Custom provider requires base_url in config.toml");
            std::process::exit(1);
        }),
        Provider::Anthropic => config
            .llm
            .base_url
            .clone()
            .unwrap_or_else(|| "https://api.anthropic.com/v1".to_string()),
        Provider::Cohere => config
            .llm
            .base_url
            .clone()
            .unwrap_or_else(|| "https://api.cohere.com/v1".to_string()),
        Provider::OpenAI => config
            .llm
            .base_url
            .clone()
            .unwrap_or_else(|| "https://api.openai.com/v1".to_string()),
    };

    // Determine API key env var
    let api_key_env = if config.llm.api_key_env.is_empty() {
        provider.default_api_key_env()
    } else {
        &config.llm.api_key_env
    };

    // For local Ollama, skip API key auth
    let is_local_ollama = provider == Provider::Ollama
        && (base_url.starts_with("http://localhost:11434")
            || base_url.starts_with("http://127.0.0.1:11434"));

    let api_key = if is_local_ollama {
        tracing::info!(
            "Local Ollama detected — skipping API key auth; set a custom base_url in config.toml for remote Ollama with auth"
        );
        String::new()
    } else {
        if provider == Provider::Ollama {
            tracing::info!(
                "Remote Ollama base_url detected — reading OLLAMA_API_KEY from environment"
            );
        } else {
            check_api_key(provider.display_name(), api_key_env);
        }
        std::env::var(api_key_env).unwrap_or_default()
    };

    // Build the rig-backed client
    let mut client = LlmClient::new(&base_url, &api_key, &model);

    if config.llm.timeout_secs > 0 {
        client = client.with_timeout(config.llm.timeout_secs);
    }

    // OpenAI endpoints use `max_completion_tokens` instead of `max_tokens`
    if provider == Provider::OpenAI || provider == Provider::Custom {
        client = client.with_use_completion_tokens(true);
    }

    // Max tokens
    if let Some(max_tokens) = config.llm.max_tokens {
        client = client.with_max_tokens(max_tokens);
    } else if provider == Provider::OpenAI || provider == Provider::Custom {
        client = client.with_max_tokens(2048);
    }

    // Temperature
    if let Some(temp) = config.llm.temperature {
        client = client.with_temperature(temp);
    } else if provider == Provider::OpenAI
        || provider == Provider::Custom
        || provider == Provider::OpenRouter
    {
        client = client.with_temperature(1.0);
    }

    // Top-p
    if let Some(top_p) = config.llm.top_p {
        client = client.with_top_p(top_p);
    } else if provider == Provider::OpenAI
        || provider == Provider::Custom
        || provider == Provider::OpenRouter
    {
        client = client.with_top_p(0.95);
    }

    // Stop sequences
    if let Some(ref stop) = config.llm.stop {
        client = client.with_stop(stop.clone());
    }

    // Frequency penalty
    if let Some(fp) = config.llm.frequency_penalty {
        client = client.with_frequency_penalty(fp);
    } else if provider == Provider::OpenAI
        || provider == Provider::Custom
        || provider == Provider::OpenRouter
    {
        client = client.with_frequency_penalty(0.0);
    }

    // Presence penalty
    if let Some(pp) = config.llm.presence_penalty {
        client = client.with_presence_penalty(pp);
    } else if provider == Provider::OpenAI
        || provider == Provider::Custom
        || provider == Provider::OpenRouter
    {
        client = client.with_presence_penalty(0.0);
    }

    tracing::info!(
        model = %model,
        base_url = %base_url,
        provider = %provider.display_name(),
        framework = "rig-core",
        "LLM client created"
    );

    client
}
