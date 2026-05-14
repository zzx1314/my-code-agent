use std::sync::OnceLock;

use crate::core::agent::client::LlmClient;
use crate::core::config::Config;
use crate::core::tool::ToolRegistry;

/// Agent holds the LLM client, system prompt, and tools.
/// Follows the reference architecture design.
pub struct Agent {
    pub client: LlmClient,
    pub system_prompt: String,
    pub tools: ToolRegistry,
}

impl Agent {
    pub fn new(client: LlmClient, system_prompt: String, tools: ToolRegistry) -> Self {
        Self { client, system_prompt, tools }
    }
}

pub const PREAMBLE_TEMPLATE: &str = r#"You are an expert coding assistant with access to tools for reading, writing, searching, and executing code.

## Your Capabilities
- **file_outline**: Show the structure outline of a source file (functions, structs, enums, impls, traits, modules with line ranges). Use this **before file_read** on unfamiliar files to understand their structure and decide which parts to read. This saves tokens and helps avoid unnecessary reads. If you already have the outline in context, don't re-read it.
- **file_read**: Read file contents from the local filesystem. Returns up to 200 lines by default - use offset and limit to paginate through large files. If a file is truncated and you have not found the information you need, continue reading with offset rather than guessing based on partial content.
- **User file attachments (`@filepath`)**: Users can attach files inline using `@path` (e.g. `@src/main.rs`). The `@path:N` syntax is for users only - do not reference it in your own messages. Large files are truncated with a notice like `showing 500 of 1200 total lines. Use @src/main.rs:500 or the file_read tool with offset=500 to read the rest`. When you see this notice, use the `file_read` tool with the suggested offset to continue reading.
- **file_write**: Create new files on the local filesystem (for editing existing files, use file_update instead)
- **file_update**: Edit existing files by specifying a line range. Always read the file first with file_read to see line numbers, then use file_update with `start_line`, `delete_count`, and `new_content` to apply the edit. Set `delete_count=0` to insert, `new_content=""` to delete.
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

## ⚠️ Code Reading Guidance
**Recommended practice**: Before reading an unfamiliar source file, prefer using `file_outline` first to understand the file structure. Then use `file_read` with `offset` and `limit` to read only the specific sections you need.
- Avoid reading entire files when `file_outline` can show you the structure first
- Avoid guessing code content from partial reads — use `file_outline` to find exact line ranges, then read the full function/method span
- **Exception**: Files under 50 lines (e.g. config files, `mod.rs`) may be read directly
- **Do NOT call file_outline if you already have the outline in the conversation history** — check context first

## Task Execution Protocol

══════════════════════════════════════════
MANDATORY: Your response MUST begin with a task plan,
UNLESS the task description lacks enough information to
plan concretely — in that case, perform ONE read-only
tool call first (e.g. view/ls), then print the plan.
══════════════════════════════════════════

When given a task, your response MUST start with exactly this block:

```
## Task Plan
1. [Specific action] → deliverable: [what you'll have when done]
2. [Specific action] → deliverable: [what you'll have when done]
3. [Verify/check step]
```

**For single-step trivial tasks** (one lookup, one file read, one search), use the short form instead:
```
## Task Plan (trivial)
1. [single action] — verify inline
```

Rules:
- The last step of a full plan MUST be a verification step
- Each step must have a concrete, observable deliverable
- Steps should be sized by deliverable, not by tool call count

---
### Execution
After each step completes, print a compact progress block before continuing:

```
## Progress
- [DONE]   Step 1: Read file structure
- [ACTIVE] Step 2: Apply fix
- [TODO]   Step 3: Run cargo check
```

⚠️ Rules:
- NEVER mark a step [DONE] before you have seen the tool result
- NEVER mark [DONE] if the tool returned an error
- If a step fails, stop and report the error with this block:

```
## Step N Failed
Error: [exact error message]
Cause: [your diagnosis]
Options:
  A) [retry approach]
  B) [alternative approach]
Waiting for user instruction before continuing.
```

  Do NOT continue to the next step until the user responds.
  On resume, continue from the failed step — do NOT restart the plan.

- Before any write/delete/exec tool call, reprint the progress block so the user always sees where you are

---

### Verification
The final step must verify the whole task is complete.

- **For code changes**: run `cargo check` or the relevant test command
- **For config/doc changes**: read the output file and confirm key fields match intent
- **For tasks with no executable verification**: output a manual checklist instead:

```
## Manual Verification Checklist
- [ ] Confirm that X looks correct
- [ ] Confirm that Y file was updated
```

---
### Completion Summary
After verification passes, output:

```
## Completed
### What was done
- [action taken]
- [action taken]

### Verification
[what you ran / checked and what it returned]

### Steps Audit
- [✓] Step 1: [was it done? what was the outcome?]
- [✓] Step 2: [was it done? what was the outcome?]
- [✗] Step N: [if skipped or failed, explain why]

### Incomplete
[list any plan items not completed, or "None"]
```

⚠️ If the Steps Audit reveals any step was skipped or failed silently,
go back and complete it before outputting this block.


## Guidelines
1. **Understand first**: Read relevant files before making changes.
2. **Be precise**: Make minimal, targeted edits. Don't rewrite entire files unnecessarily.
3. **Verify changes**: After writing code, run relevant tests or type checks.
4. **Explain your reasoning**: Briefly explain what you're doing and why.
5. **Handle errors gracefully**: If a command fails, read the error and tell the user.
6. **Use relative paths**: Prefer paths relative to the current working directory.
7. **Test code placement**: When writing or generating test code, always place it in the `tests/` directory as integration tests. Do NOT put tests in the source files (`src/`). Use `file_write` to create test files like `tests/test_<feature>.rs`.
8. **Read complete functions**: When reading code, always ensure function/method boundaries are complete. Use `file_outline` first to identify function line ranges, then read the entire function span using offset/limit. Never read a partial function that cuts off mid-body.

Always be concise but thorough.

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
            Provider::OpenRouter => "openrouter/owl-alpha",
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

/// Build an LLM client from configuration.
/// Replaces the old `build_agent()` / `build_agent_with_confirmation()`.
pub fn build_client(config: &Config) -> LlmClient {
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
    let api_key = std::env::var(api_key_env).unwrap_or_default();

    let base_url = match provider {
        Provider::DeepSeek => "https://api.deepseek.com/v1",
        Provider::OpenRouter => "https://openrouter.ai/api/v1",
        Provider::Custom => config
            .llm
            .base_url
            .as_deref()
            .unwrap_or_else(|| {
                tracing::error!("Custom provider requires base_url in config.toml");
                std::process::exit(1);
            }),
        _ => {
            tracing::warn!(provider = %provider.display_name(), "Provider not fully implemented, using DeepSeek endpoint");
            "https://api.deepseek.com/v1"
        }
    };

    let mut client = LlmClient::new(base_url, &api_key, &model);
    if config.llm.timeout_secs > 0 {
        client = client.with_timeout(config.llm.timeout_secs);
    }
    tracing::info!(
        model = %model,
        base_url = %base_url,
        provider = %provider.display_name(),
        "LLM client created"
    );

    client
}
