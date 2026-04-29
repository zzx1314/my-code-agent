# 🤖 My Code Agent

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
| `file_read` | Read file contents with line numbers, offset, and limit support |
| `file_write` | Write or create files with optional parent directory creation |
| `file_update` | Make targeted edits to existing files (find & replace) |
| `file_delete` | Delete files or directories from the filesystem |
| `shell_exec` | Execute shell commands with timeout and working directory support |
| `code_search` | Search for text patterns in source code using ripgrep (respects .gitignore) |
| `code_review` | Review code and provide improvement suggestions |
| `list_dir` | List files and directories with configurable recursion depth |
| `glob` | Find files matching a glob pattern (`**/*.rs`, `src/**/*.ts`, etc.) |
| `git_status` | Show working tree status |
| `git_diff` | Show changes between commits or working tree |
| `git_log` | Show commit history |
| `git_commit` | Create a commit with staged changes |
| `web_search` | **(MCP)** Search the web using Parallel Search for up-to-date information |
| `web_fetch` | **(MCP)** Extract content from a specific URL |

### MCP Web Search

The `web_search` and `web_fetch` tools are powered by [Parallel Search MCP](https://docs.parallel.ai/integrations/mcp/search-mcp).

**Optional: API Key**
- Without API key: Free anonymous usage (rate limited)
- With API key: Higher rate limits
- Get your key at: https://platform.parallel.ai

**Configuration:**

Enable in `config.toml`:
```toml
[mcp]
enabled = true
```
Optional API key (in config.toml or `.env`):
```toml
[mcp]
enabled = true
parallel_api_key = "your_key_here"
```
Or set environment variable:
```env
PARALLEL_API_KEY=your_key_here
```

**Usage:**
```
❯ Search the web for "latest Rust edition 2024 features"
❯ web_fetch: https://rust-lang.org
```

The tool returns search results with titles, URLs, and snippets from the web.

## Getting Started

### Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) (edition 2024+)
- A [DeepSeek API key](https://platform.deepseek.com/)

### Installation

#### Option 1: Build from Source

```bash
git clone <your-repo-url>
cd my-code-agent
cargo build --release
```

#### Option 2: Download Pre-built Binary

1. Download the pre-built binary from the [Releases](../../releases) page
2. Extract the archive and move the binary to your PATH:

```bash
# Move to a directory in your PATH
sudo mv my-code-agent-linux /usr/local/bin/
```

#### Configuration

Create a `.env` file in the directory where you will run the agent:

```env
DEEPSEEK_API_KEY=your_api_key_here
```

> ⚠️ The `.env` file is gitignored — never commit your API key.

### Running

```bash
./my-code-agent      # If running from current directory
# or
my-code-agent        # If installed to PATH
```

## Usage

Once started, you'll see the agent banner and a prompt:

```
  __  __       ____          _
|  \/  |_   _/ ___|___   __| | ___
| |\/| | | | | |   / _ \ / _` |/ _ \
| |  | | |_| | |__| (_) | (_| |  __/
|_|  |_|\__, |\____\___/ \__,_|\___|
        |___/

My Code Agent
  Interactive AI Coding Assistant

Type your message below to start chatting.
Commands: /help  /connect  /model  /save  /load  /new  /think
```

### File References (`@filepath`)

Prefix a file path with `@` to attach its contents directly into your message — the agent will see the file inline without needing a separate `file_read` tool call.

```
❯ explain @src/main.rs
❯ compare @src/main.rs and @src/lib.rs
❯ refactor @tools/shell_exec.rs to add retry logic
```

#### Offset Syntax (`@filepath:N`)

Append `:N` to a file path to start reading from a specific line (0-indexed). This is useful for continuing to read a truncated file:

```
❯ explain @src/main.rs:100       # start reading from line 100
❯ read @src/main.rs:500         # continue reading where truncation left off
```

When a file is truncated, the notice shows the offset to use for the next chunk:

```
... (file truncated: showing 500 of 1200 total lines. Use @src/main.rs:500 or the file_read tool with offset=500 to read the rest)
```

The agent is instructed to use `file_read` with the suggested offset when it encounters a truncated attachment and needs more information.

**Details:**
- Works with relative or absolute paths
- Supports multiple `@refs` in a single message
- `@path:N` offset syntax starts reading from line N (0-indexed)
- Trailing punctuation (`,`, `;`, `!`, `?`) is stripped from the path; bare `:` (no digits) is also stripped
- Files over 500 lines or 50 KB are truncated with a continuation hint
- Works inside brackets: `(@src/main.rs)`
- Email addresses like `user@example.com` are ignored

### Built-in Commands

| Command | Description |
|---------|-------------|
| `help` | Show available tools and commands |
| `usage` | Display session token usage statistics |
| `connect` | Switch or configure LLM provider |
| `model` | Change the current model |
| `save` | Save conversation session to disk |
| `load` | Load a previously saved session |
| `new` | Start a fresh session (clears current history) |
|   `think` | Expand the last collapsed reasoning content |
| `clear` | Clear conversation history (also deletes saved session) |
| `quit` / `exit` / `q` | Exit the agent |

### Examples

```
❯ Read the file src/main.rs and explain how it works
❯ explain @src/main.rs                          # same as above, but file is attached inline
❯ explain @src/main.rs:200                     # attach starting from line 200
❯ Search for all usages of the TokenUsage struct
❯ Write a new module src/utils.rs with helper functions
❯ Run cargo test and show me the results
❯ Refactor the stream_response function to be shorter
❯ compare @src/main.rs and @src/lib.rs           # attach multiple files
```

### Token Usage

After each response, a brief token summary is displayed:

```
  📊 in: 1,234 · out: 567 · total: 1,801
```

Use the `usage` command for a detailed session report:

```
  ──────── Token Usage ────────
  → Input tokens:              5,432
  ← Output tokens:             2,100
  Σ Total tokens:              7,532
  ────────────────────────────
```

### Collapsible Reasoning

The agent uses DeepSeek's reasoner model, which produces internal **reasoning** (chain-of-thought) before responding. To keep the terminal clean, reasoning is **collapsed** by default:

```
  💭 I need to check the file structure first... (142 chars, 3 lines) [type 'think' to expand]
```

To view the full reasoning content, type `think`:

```
❯ think

  💭 Reasoning:
  ─────────────────────────────────────────
  I need to check the file structure first to understand the module layout.
  Then I'll look at the specific function that needs refactoring.
  ─────────────────────────────────────────
```

- The `think` command shows the **most recent** reasoning from the last response
- `clear` also clears the stored reasoning

### Session Persistence

The agent supports session persistence (save/load/resume). By default, persistence is **disabled** — enable it in `config.toml`:

```toml
[session]
enabled = true
save_file = ".session.json"   # default
```

When enabled:
- **Auto-save on exit**: When you quit (`quit`, Ctrl+C, Ctrl+D), the session is saved to `.session.json`
- **Auto-resume on start**: If a saved session exists, the agent restores your chat history, token usage, and reasoning state
- **`save` command**: Explicitly save the current session without quitting
- **`load` command**: Load a previously saved session
- **`new` command**: Start a fresh session (clears current history)
- **`clear` command**: Clears history **and** deletes the saved session file, so it won't resume on next launch
- **Double Ctrl+C during streaming**: Saves prior conversation history (the interrupted turn is discarded, but earlier turns are preserved)

The session file is gitignored by default.

### Interrupting Responses

- Press **Esc** or **Ctrl+C** once to interrupt the current response
- Press **Esc** twice quickly, or **Ctrl+C** twice quickly, to quit the agent
- Press **Ctrl+D** to quit via EOF

## Project Structure

```
src/
├── main.rs           # CLI entry point and interactive loop
├── lib.rs            # Library crate root
├── core/            # Core functionality
│   ├── config.rs   # Configuration (TOML) with defaults
│   ├── context.rs  # @filepath parsing and expansion
│   ├── preamble.rs # Agent builder, preamble template
│   ├── session.rs  # Session persistence (save/load/resume)
│   ├── streaming.rs# Streaming response handling
│   └── token_usage.rs # Token usage tracking
├── ui/              # Terminal UI
│   ├── mod.rs      # UI module root
│   ├── render.rs   # Markdown renderer
│   └── terminal.rs # Banner, help, commands
└── tools/          # Tool implementations
    ├── mod.rs      # Tool registry (all_tools)
    ├── code_review.rs
    ├── code_search.rs
    ├── file_read.rs
    ├── file_write.rs
    ├── file_update.rs
    ├── file_delete.rs
    ├── shell_exec.rs
    ├── list_dir.rs
    ├── glob.rs
    ├── git_status.rs
    ├── git_diff.rs
    ├── git_log.rs
    ├── git_commit.rs
    └── safety.rs   # Dangerous command/file checks
tests/               # Integration tests (21 test files)
```

## Configuration

`config.toml` (optional, placed in the working directory):

```toml
[llm]
provider = "deepseek"           # deepseek, openai, anthropic, cohere, openrouter, custom
model = "deepseek-reasoner"      # model name
api_key_env = "DEEPSEEK_API_KEY"
base_url = "http://localhost:8080/v1"  # custom endpoint

[context]
window_size = 131072            # 128K tokens
warn_threshold_percent = 75
critical_threshold_percent = 90

[shell]
default_timeout_secs = 30

[agent]
max_turns = 100
thinking_display = "collapsed"  # "streaming" | "collapsed" | "hidden"
think_command = true
thinking_display_height = 5

[session]
enabled = false                 # set to true to enable session persistence
save_file = ".session.json"     # default

[mcp]
enabled = false
parallel_api_key = "your_key_here"
```

All fields are optional — sensible defaults are used when omitted.

## Running Tests

```bash
# All tests
cargo test

# Single test file
cargo test --test file_read

# With output
cargo test -- --nocapture
```

## Dependencies

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

## License

MIT
