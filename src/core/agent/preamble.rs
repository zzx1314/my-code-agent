use rig::client::CompletionClient;

use crate::core::config::Config;
use crate::tools;
use crate::tools::confirmation::ConfirmationHandle;
use rig::providers::openrouter;

use std::sync::OnceLock;
use std::time::Duration;

pub const PREAMBLE_TEMPLATE: &str = r#"You are an expert coding assistant with access to tools for reading, writing, searching, and executing code.

## Your Capabilities
- **file_outline**: Show the structure outline of a source file (functions, structs, enums, impls, traits, modules with line ranges). **ALWAYS use this BEFORE file_read** to understand the file structure and decide which parts to read. This saves tokens and helps you read only what's needed.
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

## ⚠️ Code Reading Rule (Highest Priority)
MANDATORY: Before reading any source file, you MUST use `file_outline` first to understand the file structure. Then use `file_read` with `offset` and `limit` to read ONLY the specific sections you need.
- **NEVER** read an entire file when `file_outline` can show you the structure first
- **NEVER** guess code content from partial reads — use `file_outline` to find exact line ranges, then read the full function/method span
- **Exception**: Files under 50 lines (e.g. config files, `mod.rs`) may be read directly

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

pub type OpenRouterAgent = rig::agent::Agent<openrouter::CompletionModel>;

/// Cache knowledge.md content so the preamble stays consistent throughout the session
/// This maximizes use of the LLM API's prefix caching (e.g., DeepSeek KV Cache)
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

fn build_preamble() -> String {
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

/// Agent type alias - uses OpenAI Completions API for compatibility with custom endpoints.
pub enum Agent {
    OpenAI(rig::agent::Agent<rig::providers::openai::CompletionModel>),
    OpenRouter(rig::agent::Agent<openrouter::CompletionModel>),
}
/// Builds an agent using the configured LLM provider.
/// Uses OpenAI Completions API client for compatibility with custom endpoints.
pub fn build_agent(config: &Config, mcp_tools: Vec<Box<dyn rig::tool::ToolDyn>>) -> Agent {
    build_agent_with_confirmation(config, mcp_tools, ConfirmationHandle::disabled())
}

/// Builds an agent with a confirmation handle for user interaction.
/// The handle allows tools to request user confirmation for dangerous operations.
pub fn build_agent_with_confirmation(
    config: &Config,
    mcp_tools: Vec<Box<dyn rig::tool::ToolDyn>>,
    confirmation_handle: ConfirmationHandle,
) -> Agent {
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
    let mut all_tools = tools::all_tools_with_handle(config, confirmation_handle);

    // Add MCP tools
    for tool in mcp_tools {
        all_tools.push(tool);
    }

    match provider {
        Provider::DeepSeek => {
            tracing::info!(model = %model, provider = "DeepSeek", "Model selected");
            create_openai_client(
                api_key_env,
                "https://api.deepseek.com/v1",
                &preamble,
                &model,
                all_tools,
                config.agent.max_turns,
                config.llm.timeout_secs,
            )
        }
        Provider::OpenRouter => {
            tracing::info!(model = %model, provider = "OpenRouter", "Model selected");
            let api_key = std::env::var(api_key_env).unwrap_or_default();

            // Create builder
            let mut builder = openrouter::Client::builder().api_key(&api_key);

            // Apply timeout if set
            if config.llm.timeout_secs > 0 {
                let reqwest_client = reqwest::Client::builder()
                    .timeout(Duration::from_secs(config.llm.timeout_secs))
                    .build()
                    .expect("Failed to create reqwest client");
                builder = builder.http_client(reqwest_client);
            }

            let client = builder.build().expect("Failed to create OpenRouter client");

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
                    tracing::error!("Custom provider requires base_url in config.toml");
                    tracing::error!(
                        "Example:\n  [llm]\n  provider = \"custom\"\n  model = \"llama3\"\n  base_url = \"http://localhost:11434/v1\""
                    );
                    std::process::exit(1);
                }
            };
            tracing::info!(model = %model, base_url = %base_url, provider = "Custom", "Model selected");
            create_openai_client(
                api_key_env,
                &base_url,
                &preamble,
                &model,
                all_tools,
                config.agent.max_turns,
                config.llm.timeout_secs,
            )
        }
        _ => {
            tracing::warn!(provider = %provider.display_name(), "Provider not fully implemented, using DeepSeek");
            create_openai_client(
                api_key_env,
                "https://api.deepseek.com/v1",
                &preamble,
                &model,
                all_tools,
                config.agent.max_turns,
                config.llm.timeout_secs,
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
    timeout_secs: u64,
) -> Agent {
    // Return your Agent enum
    let api_key = std::env::var(api_key_env).unwrap_or_default();

    // Start building the client
    let mut builder = rig::providers::openai::CompletionsClient::builder()
        .api_key(&api_key)
        .base_url(base_url);

    // Apply timeout if set
    if timeout_secs > 0 {
        let reqwest_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .build()
            .expect("Failed to create reqwest client");
        builder = builder.http_client(reqwest_client);
    }

    let client = builder.build().expect("Failed to create OpenAI client");

    Agent::OpenAI(
        // ← wrap into enum
        client
            .agent(model)
            .preamble(preamble)
            .tools(all_tools)
            .default_max_turns(max_turns)
            .build(),
    )
}

impl Agent {
    /// Send a prompt to the agent and get a response (synchronous)
    pub async fn prompt(&self, prompt: &str) -> Result<String, anyhow::Error> {
        match self {
            Agent::OpenAI(inner) => {
                use rig::completion::Prompt;
                let response = inner.prompt(prompt).await?;
                Ok(response)
            }
            Agent::OpenRouter(inner) => {
                use rig::completion::Prompt;
                let response = inner.prompt(prompt).await?;
                Ok(response)
            }
        }
    }
}
