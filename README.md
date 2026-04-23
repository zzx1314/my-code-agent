# 🤖 My Code Agent

An interactive AI coding assistant powered by [DeepSeek](https://deepseek.com) with tool-augmented capabilities for reading, writing, searching, and executing code — all from your terminal.

## Features

- **💬 Interactive Chat** — Multi-turn conversation with streaming responses
- **🔧 Tool-Augmented** — The agent can read files, write files, search code, and run shell commands
- **📊 Token Usage Tracking** — Monitor token consumption per-turn and per-session
- **⚡ Interrupt Handling** — Ctrl+C to interrupt a response, double-press to quit
- **📎 File References** — Use `@<filepath>` to attach file contents directly into your message
- **🎨 Colored Output** — Rich terminal UI with syntax-highlighted tool calls and usage stats

## Tools

| Tool | Description |
|------|-------------|
| `file_read` | Read file contents with line numbers, offset, and limit support |
| `file_write` | Write or create files with optional parent directory creation |
| `shell_exec` | Execute shell commands with timeout and working directory support |
| `code_search` | Search for text patterns in source code using grep |

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
╔══════════════════════════════════════════╗
║     🤖  My Code Agent v0.1.0              ║
╚══════════════════════════════════════════╝

  Tools: file_read · file_write · shell_exec · code_search
  Type: your request to get started, 'quit' to exit

❯ 
```

### File References (`@filepath`)

Prefix a file path with `@` to attach its contents directly into your message — the agent will see the file inline without needing a separate `file_read` tool call.

```
❯ explain @src/main.rs
❯ compare @src/main.rs and @src/lib.rs
❯ refactor @tools/shell_exec.rs to add retry logic
```

**Details:**
- Works with relative or absolute paths
- Supports multiple `@refs` in a single message
- Trailing punctuation (`:`, `,`, `;`, `!`, `?`) is stripped from the path
- Files over 500 lines or 50 KB are truncated with a notice
- Works inside brackets: `(@src/main.rs)`
- Email addresses like `user@example.com` are ignored

### Built-in Commands

| Command | Description |
|---------|-------------|
| `help` | Show available tools and commands |
| `usage` | Display session token usage statistics |
| `clear` | Clear conversation history |
| `quit` / `exit` / `q` | Exit the agent |

### Examples

```
❯ Read the file src/main.rs and explain how it works
❯ explain @src/main.rs                          # same as above, but file is attached inline
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

### Interrupting Responses

- Press **Ctrl+C** once to interrupt the current response
- Press **Ctrl+C** twice quickly to quit the agent
- Press **Ctrl+D** to quit via EOF

## Project Structure

```
src/
├── main.rs           # CLI entry point and interactive loop
├── lib.rs            # Library crate root
├── context.rs        # @filepath parsing and expansion
├── token_usage.rs    # Token usage tracking and reporting
└── tools/
    ├── mod.rs         # Tool registry (all_tools)
    ├── code_search.rs # Grep-based code search tool
    ├── file_read.rs   # File reading tool
    ├── file_write.rs  # File writing tool
    └── shell_exec.rs  # Shell command execution tool
tests/
├── context.rs        # Integration tests for @filepath expansion
├── code_search.rs    # Integration tests for code_search
├── file_read.rs      # Integration tests for file_read
├── file_write.rs     # Integration tests for file_write
├── shell_exec.rs     # Integration tests for shell_exec
├── token_usage.rs    # Integration tests for token_usage
└── tools.rs          # Integration tests for tool registry
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

## License

MIT
