# 🤖 My Code Agent

An interactive AI coding assistant powered by [DeepSeek](https://deepseek.com) with tool-augmented capabilities for reading, writing, searching, and executing code — all from your terminal.

## Features

- **💬 Interactive Chat** — Multi-turn conversation with streaming responses
- **🔧 Tool-Augmented** — The agent can read files, write files, search code, and run shell commands
- **📊 Token Usage Tracking** — Monitor token consumption per-turn and per-session
- **⚡ Interrupt Handling** — Ctrl+C to interrupt a response, double-press to quit
- **💾 Session Persistence** — Conversation auto-saves on exit and resumes on next launch
- **📎 File References** — Use `@<filepath>` to attach file contents directly into your message, with `@<filepath>:N` offset syntax for large files
- **🎨 Colored Output** — Rich terminal UI with syntax-highlighted tool calls and usage stats
- **💭 Collapsible Reasoning** — DeepSeek's reasoning (thinking) is collapsed into a one-line summary; use `think` to expand

## Tools

| Tool | Description |
|------|-------------|
| `file_read` | Read file contents with line numbers, offset, and limit support |
| `file_write` | Write or create files with optional parent directory creation |
| `file_update` | Make targeted edits to existing files (find & replace) |
| `file_delete` | Delete files or directories from the filesystem |
| `shell_exec` | Execute shell commands with timeout and working directory support |
| `code_search` | Search for text patterns in source code using ripgrep (respects .gitignore) |
| `list_dir` | List files and directories with configurable recursion depth |
| `glob` | Find files matching a glob pattern (`**/*.rs`, `src/**/*.ts`, etc.) |
| `web_search` | **(MCP)** Search the web using Brave Search for up-to-date information |

### MCP Web Search

The `web_search` tool is powered by the [Model Context Protocol (MCP)](https://modelcontextprotocol.io/) and the [Brave Search MCP Server](https://github.com/modelcontextprotocol/servers/tree/main/src/brave-search).

**Prerequisites:**
- Node.js and npx installed
- A [Brave Search API key](https://brave.com/search/api/)

**Configuration:**

Add to your `config.toml`:
```toml
[mcp]
enabled = true
brave_api_key = "your_brave_api_key_here"
```

Or set the `BRAVE_API_KEY` environment variable in your `.env` file.

**Usage:**
```
❯ Search the web for "latest Rust edition 2024 features"
❯ web_search: What is the current version of the rig-core crate?
```

The tool returns search results with titles, URLs, and snippets from the web.

## Getting Started

### Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) (edition 2024+)
- A [DeepSeek API key](https://platform.deepseek.com/)

### Installation

```bash
git clone <your-repo-url>
cd my-code-agent
cargo build --release
```

### Configuration

Create a `.env` file in the project root:

```env
DEEPSEEK_API_KEY=your_api_key_here
```

> ⚠️ The `.env` file is gitignored — never commit your API key.

### Running

```bash
cargo run --release
```

## Usage

Once started, you'll see the agent banner and a prompt:

```
 _                               _   
  _ __ ___  _   _    ___ ___   __| | ___    __ _  __ _  ___ _ __ | |_ 
 | '_ ` _ \| | | |  / __/ _ \ / _` |/ _ \  / _` |/ _` |/ _ \ '_ \| __|
 | | | | | | |_| | | (_| (_) | (_| |  __/ | (_| | (_| |  __/ | | | |_ 
 |_| |_| |_|\__, |  \___\___/ \__,_|\___|  \__,_|\__, |\___|_| |_|\__|
            |___/                                |___/ 

  🤖 My Code Agent v0.1.0 (reasoner)

  Tools: file_read · file_write · file_update · file_delete · shell_exec · code_search · list_dir · glob
  Type: your request to get started, 'help' for commands

❯ 
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
| `save` | Save conversation session to disk |
| `think` | Expand the last collapsed reasoning content |
| `clear` | Clear conversation history (also deletes saved session) |
| `quit` / `exit` / `q` | Exit the agent (auto-saves session) |

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

The agent automatically saves your conversation when you quit, so you can pick up where you left off:

- **Auto-save on exit**: When you quit (`quit`, Ctrl+C, Ctrl+D), the session is saved to `.session.json`
- **Auto-resume on start**: If a saved session exists, the agent restores your chat history, token usage, and reasoning state
- **`save` command**: Explicitly save the current session without quitting
- **`clear` command**: Clears history **and** deletes the saved session file, so it won't resume on next launch
- **Double Ctrl+C during streaming**: Saves prior conversation history (the interrupted turn is discarded, but earlier turns are preserved)

The session file is gitignored by default. You can configure the save path in `config.toml`:

```toml
[session]
save_file = ".my-session.json"   # default: .session.json
```

### Interrupting Responses

- Press **Ctrl+C** once to interrupt the current response
- Press **Ctrl+C** twice quickly to quit the agent
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
│   ├── render.rs   # Markdown renderer
│   └── terminal.rs # Banner, help, commands
└── tools/          # Tool implementations
    ├── mod.rs      # Tool registry (all_tools)
    ├── code_search.rs
    ├── file_read.rs
    ├── file_write.rs
    ├── file_update.rs
    ├── file_delete.rs
    ├── shell_exec.rs
    ├── list_dir.rs
    └── glob.rs
tests/               # Integration tests
```

## Running Tests

```bash
cargo test
```

## Dependencies

| Crate | Purpose |
|-------|---------|
| [rig-core](https://crates.io/crates/rig-core) | AI agent framework with tool support |
| [tokio](https://crates.io/crates/tokio) | Async runtime, process spawning, signal handling |
| [serde](https://crates.io/crates/serde) | Serialization for tool arguments/outputs |
| [colored](https://crates.io/crates/colored) | Terminal color output |
| [anyhow](https://crates.io/crates/anyhow) | Error handling |
| [thiserror](https://crates.io/crates/thiserror) | Derived error types |
| [dotenv](https://crates.io/crates/dotenv) | .env file loading |
| [futures](https://crates.io/crates/futures) | Stream utilities |
| [glob](https://crates.io/crates/glob) | File pattern matching for the glob tool |
| [termimad](https://crates.io/crates/termimad) | Markdown rendering in terminal |
| [toml](https://crates.io/crates/toml) | TOML configuration parsing |
| [crossterm](https://crates.io/crates/crossterm) | Cross-platform terminal features |
| [reedline](https://crates.io/crates/reedline) | Line editing and history |

## License

MIT
