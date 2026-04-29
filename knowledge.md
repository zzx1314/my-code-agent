# My Code Agent — Project Knowledge

## What This Is
An interactive terminal-based AI coding assistant powered by **DeepSeek** (and other LLM providers) with tool-augmented capabilities (read/write/search/execute files). Written in Rust (edition 2024).

## LLM Providers
Supports multiple LLM providers configured via `config.toml`:

| Provider | Environment Variable | Default Models |
|----------|---------------------|----------------|
| DeepSeek | `DEEPSEEK_API_KEY` | `deepseek-reasoner`, `deepseek-chat` |
| OpenAI | `OPENAI_API_KEY` | `gpt-4o`, `gpt-4o-mini`, `gpt-4-turbo` |
| Anthropic | `ANTHROPIC_API_KEY` | `claude-3-5-sonnet-20241022`, `claude-3-haiku-20240307` |
| Cohere | `COHERE_API_KEY` | `command-r-plus`, `command-r` |
| Custom | (configurable) | Any OpenAI-compatible model |

**Configuration** (`config.toml`):
```toml
[llm]
provider = "deepseek"      # deepseek, openai, anthropic, cohere, custom
model = "deepseek-reasoner"  # leave empty for provider default
api_key_env = "DEEPSEEK_API_KEY"  # or OPENAI_API_KEY, etc.

# Custom OpenAI-compatible endpoint (LocalAI, Ollama, vLLM, etc.)
# base_url = "http://localhost:8080/v1"
```

**Custom Providers** — Use `provider = "custom"` with `base_url` to connect to any OpenAI-compatible API:
- LocalAI, Ollama, vLLM, LiteLLM, etc.
- Works with any model that accepts the OpenAI `/v1/chat/completions` format
- Set `api_key_env` if your endpoint requires authentication

Example for Ollama:
```toml
[llm]
provider = "custom"
model = "llama3"
base_url = "http://localhost:11434/v1"
api_key_env = ""  # Ollama doesn't need API key by default
```

## Banner (ASCII)
```
 _                               _   
  _ __ ___  _   _    ___ ___   __| | ___    __ _  __ _  ___ _ __ | |_ 
 | '_ ` _ \| | | |  / __/ _ \ / _` |/ _ \  / _` |/ _` |/ _ \ '_ \| __|
 | | | | | | |_| | | (_| (_) | (_| |  __/ | (_| | (_| |  __/ | | | |_ 
 |_| |_| |_|\__, |  \___\___/ \__,_|\___|  \__,_|\__, |\___|_| |_|\__|
            |___/                                |___/ 
```

## Project Structure
```
src/
├── main.rs           # CLI entry point, interactive loop, banner/help text
├── lib.rs           # Library crate root (re-exports core, ui, tools)
├── app/             # Application logic
│   ├── mod.rs       # Re-exports: app, event_handler, ui
│   ├── app.rs       # App state and initialization
│   ├── event_handler.rs # Event handling (keyboard, mouse, terminal)
│   └── ui.rs        # UI layout and rendering
├── core/            # Core functionality
│   ├── mod.rs      # Re-exports: config, connection, context, context_cache, context_manager, file_cache, plan_tracker, preamble, session, streaming, token_usage
│   ├── config.rs   # Configuration (TOML) with serde defaults
│   ├── connection.rs # LLM provider connection management
│   ├── context.rs  # @filepath parsing and inline file expansion
│   ├── context_cache.rs # Context caching: preamble cache, cache metrics
│   ├── context_manager.rs # Context pruning with sliding window, auto-compact
│   ├── file_cache.rs # File content cache with mtime invalidation
│   ├── plan_tracker.rs # Task plan tracking and confirmation
│   ├── preamble.rs # Agent builder, preamble template, knowledge loading, API key check
│   ├── session.rs  # Session persistence (save/load/resume conversation)
│   ├── streaming.rs# Streaming response handling
│   └── token_usage.rs # Token usage tracking and reporting
├── ui/              # Terminal UI
│   ├── mod.rs       # Re-exports: render, terminal
│   ├── render.rs   # Markdown renderer, reasoning tracker
│   └── terminal.rs # Terminal UI components (prompt, banner, help, commands)
└── tools/           # Tool implementations (13 tools)
    ├── mod.rs       # Tool registry — all_tools()
    ├── code_review.rs # Code review tool (file or directory)
    ├── code_search.rs # Code search using ripgrep
    ├── file_delete.rs # File/directory deletion with safety checks
    ├── file_read.rs # File reading with offset/limit support
    ├── file_update.rs # Targeted file edits (find and replace)
    ├── file_write.rs # File writing (create or overwrite)
    ├── git_commit.rs # Git commit with safety checks
    ├── git_diff.rs  # Git diff viewer
    ├── git_log.rs   # Git commit history
    ├── git_status.rs # Git repository status (structured output)
    ├── glob.rs      # File pattern matching (glob syntax)
    ├── list_dir.rs  # Directory listing with recursive depth
    ├── safety.rs   # Safety guardrails for destructive operations
    └── shell_exec.rs # Shell command execution
tests/               # Integration tests (one file per module/tool)
```

## Key Code Locations
| Path | Purpose |
|------|---------|
| `src/main.rs` | CLI entry point, app initialization |
| `src/lib.rs` | Library crate root (re-exports modules) |
| `src/app/app.rs` | App state and initialization |
| `src/app/event_handler.rs` | Event handling (keyboard, mouse, terminal) |
| `src/app/ui.rs` | UI layout and rendering |
| `src/core/connection.rs` | LLM provider connection management |
| `src/core/preamble.rs` | Agent builder, preamble template, API key check |
| `src/core/context.rs` | `@filepath` parsing and inline file expansion |
| `src/core/context_cache.rs` | Context caching: preamble cache, cache metrics |
| `src/core/context_manager.rs` | Context pruning with sliding window, auto-compact |
| `src/core/file_cache.rs` | File content cache with mtime invalidation |
| `src/core/plan_tracker.rs` | Task plan tracking and confirmation |
| `src/core/token_usage.rs` | Token usage tracking and reporting |
| `src/core/session.rs` | Session persistence (save/load/delete/resume) |
| `src/core/streaming.rs` | Streaming response handling |
| `src/ui/terminal.rs` | Terminal UI components (banner, help, commands) |
| `src/ui/render.rs` | Markdown rendering, reasoning tracking |
| `src/tools/mod.rs` | Tool registry — `all_tools()` |
| `src/tools/code_review.rs` | Code review tool (file or directory) |
| `src/tools/code_search.rs` | Code search using ripgrep |
| `src/tools/safety.rs` | Safety guardrails for destructive operations |

## Tools (13 total)
`file_read` · `file_write` · `file_update` · `file_delete` · `shell_exec` · `code_search` (ripgrep) · `code_review` · `list_dir` · `glob` · `git_status` · `git_diff` · `git_log` · `git_commit`

## Commands

| Command | What it does |
|---------|-------------|
| `cargo build --release` | Build optimized binary |
| `cargo run --release` | Run the agent (requires `DEEPSEEK_API_KEY` in `.env`) |
| `cargo test` | Run all tests |
| `cargo test <module>` | Run tests for a specific module |
| `cargo clippy` | Run linter |
| `cargo fmt` | Format code |

## Key Dependencies
- **rig-core 0.35** — AI agent framework
- **tokio** — Async runtime
- **serde / serde_json** — Serialization
- **thiserror 2** — Derived error enums
- **colored 3** — Terminal color output
- **dotenv 0.15** — `.env` file loading
- **termimad** — Markdown rendering in terminal
- **crossterm** — Cross-platform terminal features
- **reedline** — Line editing and history
- **toml** — TOML configuration parsing
- **anyhow** — Error handling
- **futures** — Stream utilities
- **glob** — File pattern matching

## Conventions & Gotchas
- **No inline `#[cfg(test)]` modules** — Tests live in `tests/` directory
- **Rust edition 2024** — Very new edition
- **API key via `.env`** — `DEEPSEEK_API_KEY` must be set
- **Tool registration** — Add to `src/tools/mod.rs`, then update README and knowledge.md
- **`@filepath` expansion** — Handled in `src/core/context.rs`; supports `@path:offset` syntax (e.g. `@src/main.rs:50` reads from line 50); files >500 lines or 50KB truncated, with a notice suggesting `@path:N` or `file_read` with offset to continue
- **Session persistence** — Session auto-saves to `.session.json` on quit/interrupt; auto-resumes on next start; `save` command for explicit save; `clear` also deletes the session file. Configurable via `config.toml` `session.save_file`.
- **Esc / Ctrl+C once** = interrupt response; **Esc / Ctrl+C twice** = quit (auto-saves session)
- **App module** — Terminal UI logic split into `app/` directory: `app.rs` (state), `event_handler.rs` (input), `ui.rs` (layout)
- **Plan tracker** — Multi-step tasks require a plan (`## Task Plan` format); user can confirm (ENTER/y) or cancel (n)
- **Code review tool** — `code_review` can review a single file or entire directory recursively
