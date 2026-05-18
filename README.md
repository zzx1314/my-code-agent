# 🤖 My Code Agent

An interactive AI coding assistant powered by [DeepSeek](https://deepseek.com) with tool-augmented capabilities for reading, writing, searching, and executing code — all from your terminal.

## Features

- **💬 Interactive Chat** — Multi-turn conversation with streaming responses
- **🔧 Tool-Augmented** — The agent can read files, write files, search code, run shell commands, and search the web
- **📊 Token Usage Tracking** — Monitor token consumption per-turn and per-session
- **⚡ Interrupt Handling** — Esc or Ctrl+C to interrupt a response, double-press to quit
- **💾 Session Persistence** — Save/load conversation sessions (enable in config)
- **📎 File References** — Use `@<filepath>` to attach file contents directly into your message, with `@<filepath>:N` offset syntax for large files
- **🎨 Colored Output** — Rich terminal UI with syntax-highlighted tool calls and usage stats
- **💭 Collapsible Reasoning** — DeepSeek's reasoning (thinking) is collapsed into a one-line summary; use `think` to expand
- **🛡️ Tool Safety** — Built-in checks for dangerous file deletions and shell commands
- **🌐 Web Search** — Search the web and fetch URL content via Parallel Search MCP
- **🖱️ Mouse & Paste Support** — Mouse click handling and terminal paste events
- **📚 Knowledge Bootstrapping** — Automatic project knowledge injection from `knowledge.md`
- **👥 Multi-Agent Collaboration** — Spawn multiple specialized sub-agents (reviewer, coder, researcher, security, summarizer) to run tasks in parallel instantly
- **↩️ Undo Support** — Undo the last file write/update/delete operation

## Tools

| Tool | Description |
|------|-------------|
| `file_outline` | Show the structure outline of a source file (functions, structs, enums, impls, traits, modules with line ranges) |
| `file_read` | Read file contents with line numbers, offset, and limit support |
| `file_write` | Write or create files with optional parent directory creation |
| `file_update` | Make targeted edits to existing files (find & replace) |
| `file_delete` | Delete files or directories from the filesystem |
| `file_undo` | Undo the last file write/update/delete operation |
| `shell_exec` | Execute shell commands with timeout and working directory, confirmation, and safety checks |
| `code_search` | Search for text patterns in source code using ripgrep (respects .gitignore) |
| `spawn_agents` | Spawn multiple specialized sub-agents (reviewer, researcher, coder, summarizer, security) to run tasks in parallel and combine results |
| `code_review` | Review code and provide improvement suggestions |
| `list_dir` | List files and directories with configurable recursion depth |
| `glob` | Find files matching a glob pattern (`**/*.rs`, `src/**/*.ts`, etc.) |
| `git_status` | Show working tree status |
| `git_diff` | Show changes between commits or working tree |
| `git_log` | Show commit history |
| `git_commit` | Create a commit with staged changes |
| `web_search` | Search the web using Parallel Search for up-to-date information (MCP-powered) |
| `web_fetch` | Extract content from a specific URL (MCP-powered) |

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

### Multi-Agent Collaboration

The `spawn_agents` tool enables parallel execution of multiple specialized sub-agents, each with its own role and prompt. All agents run concurrently and their results are combined in a single response.

**Key Features:**
- **Parallel Execution** — All agents run concurrently using `tokio` tasks, dramatically reducing turnaround time
- **Specialized Roles** — Each agent type has a tailored system prompt for optimal performance
- **Custom Agents** — Use any custom agent type for flexible, domain-specific tasks
- **Error Resilience** — Individual agent failures don't block other agents; errors are reported per-agent

**Built-in agent types:**

| Agent Type | Role | Best For |
|------------|------|----------|
| `reviewer` | Code review specialist | Analyzing code for bugs, security issues, performance problems, and suggesting improvements |
| `researcher` | Technical research specialist | Providing comprehensive, well-structured answers with best practices and examples |
| `coder` | Senior software engineer | Writing clean, well-structured code with error handling, comments, and tests |
| `summarizer` | Technical summarization specialist | Condensing complex information into clear, concise summaries |
| `security` | Security specialist | Analyzing code for SQL injection, XSS, CSRF, auth flaws, and insecure dependencies |
| *(custom)* | Generic assistant | Any custom type uses a generic assistant prompt for flexible use cases |

**Constraints:**
- **Maximum 10 agents** per spawn request (to prevent resource exhaustion)
- Each agent runs as an independent LLM call with its own system prompt
- Results are combined into a single JSON response

**Usage:**

```bash
# Basic parallel review
❯ spawn multiple agents to review this PR from different perspectives

# Code review + security audit
❯ Run a code review and security audit in parallel on @src/main.rs

# Multi-perspective analysis
❯ Have a coder write the implementation, a reviewer check it, and a summarizer combine feedback

# Technical research
❯ Spawn a researcher to analyze Rust async patterns and a summarizer to create a cheat sheet
```

**Example — parallel code review and security audit:**

The agent can orchestrate multiple sub-agents in a single turn. For instance, when asked to review a pull request, it might spawn:

1. **reviewer** — Analyze code quality, style, and logic
2. **security** — Scan for vulnerabilities
3. **summarizer** — Combine both reports into a structured response

All three run concurrently, dramatically reducing turnaround time for complex multi-perspective analyses.

**Example — custom agent types:**

You can use any custom agent type for domain-specific tasks. Custom types use a **generic assistant prompt** (unless explicitly configured), which is suitable for flexible use cases:

```bash
# Custom domain expert (uses generic assistant prompt)
❯ Spawn a "database" agent to analyze our schema and a "performance" agent to identify bottlenecks

# Specialized reviewer (uses generic assistant prompt)
❯ Have a "frontend" agent review our React components and a "backend" agent review the API
```

> **Note:** Custom agent types (e.g., `database`, `performance`, `frontend`, `backend`) use a generic assistant prompt by default. For specialized behavior, use the built-in agent types (`reviewer`, `researcher`, `coder`, `summarizer`, `security`) which have tailored system prompts.

**Configuration:**

No special configuration needed — `spawn_agents` is available by default. It uses the same LLM provider configured in `config.toml` to power all sub-agents.

**Error Handling:**

If an individual agent fails, its error is included in the response without affecting other agents:

```json
[
  {
    "agent_type": "reviewer",
    "prompt": "Review this code...",
    "content": "The code looks good overall...",
    "error": null
  },
  {
    "agent_type": "security",
    "prompt": "Check for vulnerabilities...",
    "content": "",
    "error": "Rate limit exceeded"
  }
]
```
## Getting Started

### Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) (edition 2024+)
- A [DeepSeek API key](https://platform.deepseek.com/)

### Installation

#### Option 1: `cargo install` (recommended)

```bash
cargo install --path .
```

This builds a release binary and installs it to `~/.cargo/bin/`, so you can run `my-code-agent` from anywhere (make sure `~/.cargo/bin` is in your `PATH`).

#### Option 2: Build from Source

```bash
git clone <your-repo-url>
cd my-code-agent
cargo build --release
```

The binary will be at `target/release/my-code-agent`.

#### Option 3: Download Pre-built Binary

1. Download the pre-built binary from the [Releases](../../releases) page
2. Extract the archive and move the binary to your PATH:

```bash
# Move to a directory in your PATH
sudo mv my-code-agent-linux /usr/local/bin/
```

#### API Key Setup

Create a `.env` file in the directory where you will run the agent:

```env
DEEPSEEK_API_KEY=your_api_key_here
```

> ⚠️ The `.env` file is gitignored — never commit your API key.

See the [Configuration Reference](#configuration-reference) section below for the full `config.toml` reference.

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
| `think` | Expand the last collapsed reasoning content |
| `clear` | Clear conversation history (also deletes saved session) |
| `init` | Re-initialize the agent (reconnect LLM) |
| `plan` | Create or refine a task plan before execution |
| `shell` | Toggle to shell command mode |
| `status` | Show agent status (provider, model, costs) |
| `tokens` | Show detailed token usage report |
| `undo` | Undo the last file write/update/delete |
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

## Configuration Reference

Below is the full `config.toml` (optional, placed in the working directory):

```toml
[llm]
provider = "deepseek"           # deepseek, openai, anthropic, cohere, openrouter, custom
model = "deepseek-reasoner"      # model name
api_key_env = "DEEPSEEK_API_KEY"
base_url = "http://localhost:8080/v1"  # custom endpoint

[context]
window_size = 1048576           # 1M tokens
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

## License

MIT
