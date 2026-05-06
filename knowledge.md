
# Project Knowledge

## What This Is
An interactive AI coding assistant powered by [DeepSeek](https://deepseek.com) with tool-augmented capabilities for reading, writing, searching, and executing code — all from your terminal.

## Features

- **💬 Interactive Chat** — Multi-turn conversation with streaming responses
- **🔧 Tool-Augmented** — The agent can read files, write files, search code, and run shell commands
- **📊 Token Usage Tracking** — Monitor token consumption per-turn and per-session
- **⚡ Interrupt Handling** — Esc or Ctrl+C to interrupt a response, double-press to quit
- **💾 Session Persistence** — Save/load conversation sessions (enable in config)
- **📎 File References** — Use `@<filepath>` to attach file contents directly into your message, with `@<filepath>:N` offset syntax for large files
- **🎨 Colored Output** — Rich terminal UI with syntax-highlighted tool calls and usage stats
- **💭 Collapsible Reasoning** — DeepSeek's reasoning (thinking) is collapsed into a one-line summary; use `think` to expand
- **🛡️ Tool Safety** — Built-in checks for dangerous file deletions and shell commands

## Tools

| Tool | Description |
|------|-------------|
| `file_outline` | Show the structure outline of a source file (functions, structs, enums, impls, traits, modules with line ranges) |
| `file_read` | Read file contents with line numbers, offset, and limit support (default limit: 200 lines) |
| `file_write` | Write or create files with optional parent directory creation |
| `file_update` | Make targeted edits to existing files (find & replace) |
| `file_delete` | Delete files or directories; `snippet` parameter removes specific text from a file |
| `file_undo` | Undo recent file changes from `file_write`, `file_update`, or `file_delete` (configurable step count) |
| `shell_exec` | Execute shell commands with timeout and working directory support |
| `code_search` | Search for text patterns in source code using ripgrep (respects .gitignore, filters by file type) |
| `code_review` | Review code files or directories for quality, potential issues, and improvements |
| `list_dir` | List files and directories with configurable recursion depth (`max_depth`) |
| `glob` | Find files matching a glob pattern (`**/*.rs`, `src/**/*.ts`, etc.) |
| `git_status` | Show working tree status in structured JSON format |
| `git_diff` | Show changes between commits or working tree (supports file-specific and cached diffs) |
| `git_log` | Show commit history with hash, author, date, message |
| `git_commit` | Create a commit with staged changes (includes safety confirmation) |
| `web_search` | **(MCP)** Search the web using Parallel Search for up-to-date information |
| `web_fetch` | **(MCP)** Extract content from a specific URL in markdown format |

## Project Structure

```
src/
├── main.rs               # CLI entry point and interactive loop
├── lib.rs                # Library crate root (module declarations)
├── core/                 # Core functionality (12 files)
│   ├── mod.rs
│   ├── config.rs         # Configuration (TOML) with defaults
│   ├── connection.rs     # LLM connection management
│   ├── context.rs        # @filepath parsing and expansion
│   ├── context_cache.rs  # Context caching
│   ├── context_manager.rs# Context window management
│   ├── file_cache.rs     # File content caching
│   ├── parser.rs         # General parsing utilities
│   ├── plan_tracker.rs   # Task planning and tracking
│   ├── preamble.rs       # Agent builder, preamble template
│   ├── session.rs        # Session persistence (save/load/resume)
│   ├── streaming.rs      # Streaming response handling
│   └── token_usage.rs    # Token usage tracking
├── app/                  # Application layer (3 submodules + main struct)
│   ├── mod.rs            # App struct, InitResult, PendingConfirmation
│   ├── conversion.rs     # Data conversion utilities
│   ├── event_handler.rs  # User input event handling
│   └── ui.rs             # Application UI logic
├── ui/                   # Terminal UI (3 files)
│   ├── mod.rs            # UI module root
│   ├── render.rs         # Markdown renderer
│   └── terminal.rs       # Banner, help, commands
├── tools/                # Tool implementations (19 files)
│   ├── mod.rs            # Tool registry (all_tools, all_tools_with_handle, create_mcp_tools)
│   ├── code_review.rs    # Code review tool
│   ├── code_search.rs    # Ripgrep-based code search
│   ├── confirmation.rs   # User confirmation prompts
│   ├── file_delete.rs    # File/directory deletion
│   ├── file_outline.rs   # File structure outline (functions, structs, etc.)
│   ├── file_read.rs      # File content reading
│   ├── file_undo.rs      # Undo file changes
│   ├── file_update.rs    # Targeted find & replace edits
│   ├── file_write.rs     # File creation/writing
│   ├── git_commit.rs     # Git commit creation
│   ├── git_diff.rs       # Git diff display
│   ├── git_log.rs        # Git log history
│   ├── git_status.rs     # Git status display
│   ├── glob.rs           # File pattern matching
│   ├── list_dir.rs       # Directory listing
│   ├── safety.rs         # Dangerous command/file checks
│   ├── shell_exec.rs     # Shell command execution
│   └── undo_history.rs   # Undo history management
└── mcp/                  # Model Context Protocol (4 files)
    ├── mod.rs
    ├── client.rs         # MCP client implementation
    ├── types.rs          # MCP type definitions
    └── web_search_tool.rs # Web search via Parallel Search MCP

tests/                    # Integration tests (23 test files)
.github/workflows/        # CI/CD (release.yml)
```

## Key Dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| [rig-core](https://crates.io/crates/rig-core) | 0.35 | AI agent framework with tool support |
| [tokio](https://crates.io/crates/tokio) | 1 | Async runtime, process spawning, signal handling |
| [reqwest](https://crates.io/crates/reqwest) | 0.13 | HTTP client for API requests |
| [serde](https://crates.io/crates/serde) | 1 | Serialization for tool arguments/outputs |
| [serde_json](https://crates.io/crates/serde_json) | 1 | JSON serialization |
| [anyhow](https://crates.io/crates/anyhow) | 1 | Error handling |
| [thiserror](https://crates.io/crates/thiserror) | 2 | Derived error types |
| [dotenv](https://crates.io/crates/dotenv) | 0.15 | .env file loading |
| [futures](https://crates.io/crates/futures) | 0.3 | Stream utilities |
| [glob](https://crates.io/crates/glob) | 0.3.3 | File pattern matching for the glob tool |
| [toml](https://crates.io/crates/toml) | 1.1.2 | TOML configuration parsing |
| [crossterm](https://crates.io/crates/crossterm) | 0.28 | Cross-platform terminal features |
| [ratatui](https://crates.io/crates/ratatui) | 0.28.1 | Terminal UI rendering |
| [tui-textarea](https://crates.io/crates/tui-textarea) | 0.6 | Text input area widget |
| [tui-markdown](https://crates.io/crates/tui-markdown) | 0.2 | Markdown rendering in terminal |
| [async-process](https://crates.io/crates/async-process) | 2 | Process spawning for MCP servers |
| [async-trait](https://crates.io/crates/async-trait) | 0.1 | Async trait support |
| [tracing](https://crates.io/crates/tracing) | 0.1 | Application-level tracing |
| [tracing-subscriber](https://crates.io/crates/tracing-subscriber) | 0.3 | Tracing subscriber for logging |
| [unicode-width](https://crates.io/crates/unicode-width) | 0.1 | Unicode character width calculation |
| [tree-sitter](https://crates.io/crates/tree-sitter) | 0.26 | Source code parsing for file_outline |
| [tree-sitter-rust](https://crates.io/crates/tree-sitter-rust) | 0.24 | Rust grammar for tree-sitter |

**Dev Dependencies:**

| Crate | Version | Purpose |
|-------|---------|---------|
| [tempfile](https://crates.io/crates/tempfile) | 3 | Temporary files/directories for tests |

## Configuration

`config.toml` (optional, placed in the working directory):

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

[mcp]
enabled = false
parallel_api_key = "your_key_here"
```

All fields are optional — sensible defaults are used when omitted.

## Conventions & Gotchas

- **Tool naming**: Tools use snake_case (e.g., `file_read`, `shell_exec`)
- **File attachments**: Use `@filepath` syntax to attach files inline; `@filepath:N` to offset (0-indexed). Files over 500 lines or 50 KB are truncated with a continuation hint
- **Session files**: Saved sessions are gitignored (`.session.json` by default)
- **Token limits**: Default context window is 128K tokens; warnings at 75% and 90% thresholds
- **Reasoning display**: Collapsed by default; use `think` command to expand. Configurable via `thinking_display` setting
- **Interrupt behavior**: Single Esc/Ctrl+C interrupts; double-tap to quit; Ctrl+D for EOF quit
- **MCP tools**: `web_search` and `web_fetch` require MCP to be enabled in `config.toml`
- **Undo support**: `file_undo` tool can revert recent `file_write`, `file_update`, or `file_delete` operations (uses `undo_history.rs`)
- **File outline**: Uses tree-sitter for structured code parsing (currently supports Rust via `tree-sitter-rust`)
- **Tool safety**: Destructive shell commands and file deletions trigger a user confirmation prompt via `confirmation.rs`
- **Test files**: All test cases should be written in the `tests/` directory as integration tests
- **Rust edition**: Project uses Rust edition 2024