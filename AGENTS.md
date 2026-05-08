# AGENTS.md — My Code Agent

## Quick Start

```bash
cargo build --release
cargo run --release

# Run a specific test
cargo test <test_name>
```

## Architecture

- **Entry point**: `src/main.rs` - CLI entry point and interactive loop
- **Library root**: `src/lib.rs` — exports `app`, `core`, `mcp`, `tools`, `ui` modules

### `src/app/` — Application Layer

- `mod.rs` — App struct, InitResult, PendingConfirmation
- `conversion.rs` — Data conversion utilities (rig ↔ app message types)
- `lifecycle.rs` — Application lifecycle management
- `event_handler/` — User input event handling, command dispatch
  - `init.rs` — Event handler initialization
  - `message.rs` — Message event processing
  - `streaming.rs` — Streaming event handling
  - `terminal.rs` — Terminal event handling
  - `command/` — Slash command implementations (15 commands):
    - `clear.rs`, `connect.rs`, `help.rs`, `init.rs`, `load.rs`, `model.rs`, `plan.rs`, `quit.rs`, `save.rs`, `shell.rs`, `status.rs`, `think.rs`, `tokens.rs`, `undo.rs`
  - `key_event/` — Key event handling
    - `completion.rs` — Tab completion logic
    - `input/` — Input key event handlers (`enter.rs`, `shell.rs`)
    - `picker/` — Picker widget handlers (`model.rs`, `provider.rs`, `session.rs`)
- `ui/` — Application UI rendering
  - `chat.rs` — Chat area rendering
  - `input.rs` — Input area rendering
  - `overlays.rs` — Overlay dialogs (picker, confirmation, etc.)
  - `status.rs` — Status bar rendering

### `src/core/` — Core Functionality

- `init.rs` — Core initialization
- `config/` — Configuration management
  - `mod.rs` — TOML config loader with defaults (Config, LLMConfig, FileConfig, ContextConfig, ShellConfig, AgentConfig, SessionConfig, McpConfig)
- `agent/` — LLM agent management
  - `connection.rs` — LLM connection management (ConnectionStatus, ConnectionState)
  - `preamble.rs` — Agent builder, preamble template, provider setup
  - `streaming.rs` — Streaming response handling (StreamResult, StreamEvent)
- `context/` — Context management
  - `file_ref.rs` — @filepath parsing and expansion (FileRef, ExpandResult)
  - `context_cache.rs` — Context caching (preamble_cache, CacheMetrics, ContextCache)
  - `context_manager.rs` — Context window management (ContextManager)
  - `file_cache.rs` — File content caching (FileCache, FileCacheEntry)
  - `token_usage.rs` — Token usage tracking (TokenUsage, ContextWarning)
- `parser/` — Parsing utilities
  - `mod.rs` — Structure info, smart read, parsed file (tree-sitter based, supports Rust)
- `session/` — Session persistence
  - `mod.rs` — Save/load/resume, SessionData, SessionInfo, search_sessions
- `paths/` — Path utilities
  - `mod.rs` — Path resolution helpers

### `src/tools/` — Tool Implementations (18 tools)

- `mod.rs` — Tool registry (`all_tools()`, `all_tools_with_handle()`, `create_mcp_tools()`)
- `code_review.rs` — Code review tool
- `code_search.rs` — Ripgrep-based code search
- `confirmation.rs` — User confirmation prompts
- `file_delete.rs` — File/directory deletion
- `file_outline.rs` — File structure outline (tree-sitter based)
- `file_read.rs` — File content reading
- `file_undo.rs` — Undo file changes
- `file_update.rs` — Targeted find & replace edits
- `file_write.rs` — File creation/writing
- `git_commit.rs` — Git commit creation
- `git_diff.rs` — Git diff display
- `git_log.rs` — Git log history
- `git_status.rs` — Git status display
- `glob.rs` — File pattern matching
- `list_dir.rs` — Directory listing
- `safety.rs` — Dangerous command/file checks
- `shell_exec.rs` — Shell command execution
- `undo_history.rs` — Undo history management (persistent .undo_history.json)

### `src/ui/` — Terminal UI

- `markdown.rs` — Custom markdown renderer (headings, code blocks, bold, lists, etc.)
- `render.rs` — Markdown rendering integration
- `terminal.rs` — Banner, help, startup text

### `src/mcp/` — Model Context Protocol

- `client.rs` — MCP client implementation
- `types.rs` — MCP type definitions
- `web_search_tool.rs` — Web search via Parallel Search MCP

## Key Dependencies

- **rig-core 0.35** — AI agent framework (defines `ToolDyn`, `Message`, `CompletionClient`)
- **tokio** — async runtime, signals
- **ratatui** — Terminal UI rendering (with tui-textarea)
- **crossterm** — Terminal features
- **reqwest** — HTTP client for API requests
- **serde/serde_json** — Serialization
- **anyhow/thiserror** — Error handling
- **dotenv** — `.env` loading
- **futures** — Stream utilities
- **glob** — File pattern matching
- **toml** — TOML config parsing
- **tree-sitter/tree-sitter-rust** — Source code parsing for file_outline
- **async-process** — Process spawning for MCP servers
- **async-trait** — Async trait support
- **tracing/tracing-subscriber** — Application-level logging
- **unicode-width** — Unicode character width calculation

## Configuration

`config.toml` (optional):

```toml
[llm]
provider = "deepseek"           # deepseek, openai, anthropic, cohere, openrouter, custom
model = "deepseek-reasoner"     # model name
api_key_env = "DEEPSEEK_API_KEY"
base_url = "http://localhost:8080/v1"  # custom endpoint (for "custom" provider)
timeout_secs = 60               # LLM API request timeout (0 to disable)

[files]
default_read_limit = 200        # max lines returned by file_read
attach_max_lines = 500          # max lines per @filepath attachment
attach_max_bytes = 51200        # max bytes (50 KB) per @filepath attachment

[context]
window_size = 1048576           # 1M tokens
warn_threshold_percent = 75
critical_threshold_percent = 90

[shell]
default_timeout_secs = 30

[agent]
max_turns = 100                 # max tool-call turns per response
thinking_display = "collapsed"  # "streaming" | "collapsed" | "hidden"
think_command = true            # enable /think command
thinking_display_height = 5     # terminal lines for reasoning display

[session]
enabled = false                 # set to true to enable session persistence
save_file = ".session.json"     # default
cleanup_undo_history = false    # clean up undo history entries on session exit

[mcp]
enabled = false                 # enable MCP web search tools
parallel_api_key = ""           # or set PARALLEL_API_KEY env var (optional, works without for free usage)
```

## Important Patterns

### Adding a New Tool

1. Create `src/tools/<tool_name>.rs` with struct implementing `Tool` trait from rig-core
2. Export in `src/tools/mod.rs`
3. Add to `all_tools()` in `tools/mod.rs`
4. Add tests in `tests/<tool_name>.rs`

### Adding a New Command

1. Create `src/app/event_handler/command/<command_name>.rs`
2. Export in `src/app/event_handler/command/mod.rs`
3. Wire up in event handler dispatch

### Tool Safety

`src/tools/safety.rs` provides:
- `is_dangerous_deletion()` — checks risky paths (/, ~, etc.)
- `is_dangerous_shell_command()` — blocks `rm -rf`, `> file`, etc.
- `is_dangerous_snippet_deletion()` — blocks `/**/` or `#![deny(*)]`

Review safety module before allowing destructive operations.

### Session Persistence

- Auto-saves on quit to `.session.json` (gitignored)
- `/save <name>` and `/load <name>` commands
- `/sessions` lists available sessions
- Sessions stored in `.sessions/` directory as timestamped JSON files

## Testing

```bash
# All tests
cargo test

# Single test file
cargo test --test file_read

# With output
cargo test -- --nocapture
```

## Gotchas

- **Rust edition 2024** — requires recent nightly/beta: `rustup update`
- **.env required** — API key in project root (gitignored)
- **No clippy/rustfmt config** — defaults used
- **rig-core version matters** — breaking changes possible between minor versions
- **Tool naming**: Tools use snake_case (e.g., `file_read`, `shell_exec`)
- **Context caching**: The system caches preamble content and file reads for performance
- **Undo support**: `file_undo` tool can revert recent file operations; history persisted in `.undo_history.json`

## File References (`@filepath`)

- `@path` attaches file inline
- `@path:N` starts at line N (0-indexed)
- Truncates at 500 lines / 50 KB
- Supports tab completion with `@` prefix
