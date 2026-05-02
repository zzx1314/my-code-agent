

⟳ *Tool Call:* `file_read`


⟳ *Tool Call:* `file_read`


⟳ *Tool Call:* `list_dir`


⟳ *Tool Call:* `list_dir`


⟳ *Tool Call:* `list_dir`


⟳ *Tool Call:* `file_read`

⟳ *Tool Call:* `list_dir`


⟳ *Tool Call:* `list_dir`

⟳ *Tool Call:* `list_dir`

```markdown
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

## Project Structure
```
src/
├── main.rs               # CLI entry point and interactive loop
├── lib.rs                # Library crate root
├── core/                 # Core functionality (12 files)
│   ├── mod.rs
│   ├── config.rs         # Configuration (TOML) with defaults
│   ├── connection.rs     # LLM connection management
│   ├── context.rs        # @filepath parsing and expansion
│   ├── context_cache.rs  # Context caching
│   ├── context_manager.rs# Context window management
│   ├── file_cache.rs     # File content caching
│   ├── plan_tracker.rs   # Task planning and tracking
│   ├── preamble.rs       # Agent builder, preamble template
│   ├── session.rs        # Session persistence (save/load/resume)
│   ├── streaming.rs      # Streaming response handling
│   └── token_usage.rs    # Token usage tracking
├── app/                  # Application layer (4 files)
│   ├── mod.rs
│   ├── conversion.rs     # Data conversion utilities
│   ├── event_handler.rs  # User input event handling
│   └── ui.rs             # Application UI logic
├── ui/                   # Terminal UI (3 files)
│   ├── mod.rs            # UI module root
│   ├── render.rs         # Markdown renderer
│   └── terminal.rs       # Banner, help, commands
├── tools/                # Tool implementations (15 files)
│   ├── mod.rs            # Tool registry (all_tools)
│   ├── code_review.rs
│   ├── code_search.rs
│   ├── file_read.rs
│   ├── file_write.rs
│   ├── file_update.rs
│   ├── file_delete.rs
│   ├── shell_exec.rs
│   ├── list_dir.rs
│   ├── glob.rs
│   ├── git_status.rs
│   ├── git_diff.rs
│   ├── git_log.rs
│   ├── git_commit.rs
│   └── safety.rs         # Dangerous command/file checks
└── mcp/                  # Model Context Protocol (4 files)
│   ├── mod.rs
│   ├── client.rs         # MCP client implementation
│   ├── types.rs          # MCP type definitions
│   └── web_search_tool.rs # Web search via Parallel Search MCP
```

## Key Dependencies

| Crate | Purpose |
|-------|---------|
| [rig-core](https://crates.io/crates/rig-core) | AI agent framework with tool support |
| [tokio](https://crates.io/crates/tokio) | Async runtime, process spawning, signal handling |
| [reqwest](https://crates.io/crates/reqwest) | HTTP client for API requests |
| [serde](https://crates.io/crates/serde) | Serialization for tool arguments/outputs |
| [serde_json](https://crates.io/crates/serde_json) | JSON serialization |
| [anyhow](https://crates.io/crates/anyhow) | Error handling |
| [thiserror](https://crates.io/crates/thiserror) | Derived error types |
| [dotenv](https://crates.io/crates/dotenv) | .env file loading |
| [futures](https://crates.io/crates/futures) | Stream utilities |
| [glob](https://crates.io/crates/glob) | File pattern matching for the glob tool |
| [toml](https://crates.io/crates/toml) | TOML configuration parsing |
| [crossterm](https://crates.io/crates/crossterm) | Cross-platform terminal features |
| [ratatui](https://crates.io/crates/ratatui) | Terminal UI rendering |
| [tui-textarea](https://crates.io/crates/tui-textarea) | Text input area widget |
| [tui-markdown](https://crates.io/crates/tui-markdown) | Markdown rendering in terminal |
| [async-process](https://crates.io/crates/async-process) | Process spawning for MCP servers |
| [async-trait](https://crates.io/crates/async-trait) | Async trait support |
| [tracing](https://crates.io/crates/tracing) | Application-level tracing |
| [tracing-subscriber](https://crates.io/crates/tracing-subscriber) | Tracing subscriber for logging |
| [unicode-width](https://crates.io/crates/unicode-width) | Unicode character width calculation |

## Conventions & Gotchas

- **Tool naming**: Tools use snake_case (e.g., `file_read`, `shell_exec`)
- **File attachments**: Use `@filepath` syntax to attach files inline; `@filepath:N` to offset (0-indexed)
- **Session files**: Saved sessions are gitignored (`.session.json` by default)
- **Token limits**: Default context window is 128K tokens; warnings at 75% and 90% thresholds
- **Reasoning display**: Collapsed by default; use `think` command to expand
- **Interrupt behavior**: Single Esc/Ctrl+C interrupts; double-tap to quit
- **MCP tools**: `web_search` and `web_fetch` require MCP to be enabled in config.toml
```
