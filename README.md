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

The project includes a comprehensive multi-agent system with two complementary mechanisms: **parallel sub-agents** (`spawn_agents`) and an **automated code review pipeline** (`AgentOrchestrator` + `ReviewAgent`).

#### 1. Parallel Sub-Agents (`spawn_agents`)

The `spawn_agents` tool enables parallel execution of multiple specialized sub-agents, each with its own role and prompt. All agents run concurrently using `tokio` tasks and their results are combined in a single response.

**Built-in agent types:**

| Agent Type | Role | Best For |
|------------|------|----------|
| `reviewer` | Code review specialist | Analyzing code for bugs, security issues, performance problems, and suggesting improvements |
| `researcher` | Technical research specialist | Providing comprehensive, well-structured answers with best practices and examples |
| `coder` | Senior software engineer | Writing clean, well-structured code with error handling, comments, and tests |
| `summarizer` | Technical summarization specialist | Condensing complex information into clear, concise summaries |
| `security` | Security specialist | Analyzing code for SQL injection, XSS, CSRF, auth flaws, and insecure dependencies |
| *(custom)* | Generic assistant | Any custom type uses a generic assistant prompt for flexible use cases |

**Constraints:** Max 10 agents per spawn request. Each agent runs as an independent LLM call with its own system prompt.

**Usage:**
```
# Parallel code review + security audit
❯ Run a code review and security audit in parallel on @src/main.rs

# Multi-perspective analysis
❯ Have a coder write the implementation, a reviewer check it, and a summarizer combine feedback

# Technical research
❯ Spawn a researcher to analyze Rust async patterns and a summarizer to create a cheat sheet
```

#### 2. Automated Code Review (`/review` + Auto-Review)

The `AgentOrchestrator` coordinates collaboration between the **main agent** (handles tasks) and a dedicated **`ReviewAgent`** (performs code review). This is not a simple tool call — it's a full orchestration pipeline.

**Key capabilities:**
- **Auto-detection** — Automatically detects changed files from tool execution history (`file_write`, `file_update`, `file_delete`, `apply_patch`)
- **Phased multi-category review** — Structured analysis across functional completeness, security, logic, error handling, API misuse, performance, and concurrency
- **Fix→Review loop** — When auto-review finds issues, the main agent can automatically fix them and re-review (up to `max_review_iterations`)
- **Structured JSON output** — Returns issues with file, line, severity, category, title, description, suggestion, and fix examples
- **Event streaming** — Review progress (phase completions, reasoning) streams to the UI in real-time

**The `/review` command:**

| Syntax | Description |
|--------|-------------|
| `/review` | Review code changes in the current conversation |
| `/review <path>` | Review code at the specified file or directory |
| `/review --auto` | Toggle auto-review mode on/off |
| `/review --help` | Show review command help |

**How it works:**

1. Main agent completes code changes (e.g., `file_write`, `file_update`)
2. `AgentOrchestrator` detects changed files from tool output
3. `ReviewAgent` sends diffs directly to the LLM (no tools registered) with a structured system prompt
4. LLM returns JSON with issues and a verdict (`approved` or `needs_revision`)
5. If `needs_revision` and auto-review is enabled, the main agent receives the review feedback and attempts fixes
6. Loop repeats up to `max_review_iterations` (default: 3)

**Example — review report output:**
```json
{
  "issues": [
    {
      "file": "src/tools/shell_exec.rs",
      "line": 42,
      "end_line": 50,
      "severity": "high",
      "category": "security",
      "title": "Missing input validation",
      "description": "Shell command input is not sanitized before execution",
      "suggestion": "Add input validation and escape special characters",
      "code_snippet": "let cmd = args.command;",
      "fix_example": "let cmd = sanitize_shell_input(&args.command);"
    }
  ],
  "summary": {
    "overall_score": 75,
    "verdict": "needs_revision"
  }
}
```

**Review categories:** `bug_risk`, `security`, `functional_completeness`, `performance`, `error_handling`, `style`, `maintainability`

**Severity levels:** `critical` > `high` > `medium` > `low` > `info`

**Configuration:**

```toml
[review]
enabled = true               # Enable review functionality (default: true)
auto_review = true            # Auto-review after main agent completes (default: true)
threshold_lines = 5           # Min lines changed to trigger auto-review (default: 5)
max_issues = 50               # Max issues to report per review (default: 50)
severity_threshold = "low"    # Min severity to report (default: "low")
on_file_write = true          # Trigger review on file_write (default: true)
on_file_update = true         # Trigger review on file_update (default: true)
max_review_iterations = 3     # Max fix→review loop iterations (default: 3)
```

#### Error Handling

Both systems handle errors gracefully:
- **`spawn_agents`**: Individual agent failures don't block others; errors are reported per-agent in the JSON response
- **`ReviewAgent`**: Review failures are reported without crashing the main session; the orchestrator falls back gracefully

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

The agent supports a **two-tier** session persistence system:

1. **📁 Timestamped Sessions** (always on) — automatically saves a named session snapshot to the `.sessions/` directory on exit, keeping the 5 most recent sessions.
2. **🔄 Auto-Resume Session** (opt-in) — saves to a default file that auto-loads on next startup for seamless continuation.

#### Configuration

```toml
[session]
enabled = false                  # Enable auto-resume (save/load on start/exit)
save_file = ".session.json"      # Default session file for auto-resume
cleanup_undo_history = false     # Clean up undo history entries on exit
```

| Setting | Default | Description |
|---------|---------|-------------|
| `enabled` | `false` | Enable auto-save on exit **and** auto-resume on startup |
| `save_file` | `".session.json"` | File path for the auto-resume session (resolved in app data directory) |
| `cleanup_undo_history` | `false` | Remove undo history entries for the current session on exit |

#### How It Works

**On every exit** (quit, Ctrl+C, Ctrl+D):  
Always saves a timestamped session file to `<app_data>/.sessions/<timestamp>.json` — this is unconditional (even if `session.enabled = false`). Old sessions are pruned to keep only the **5 most recent** ones.

**When `session.enabled = true`**:  
In addition to the timestamped snapshot, the agent saves a **default session** file (e.g. `.session.json`). On next startup, if this file exists, the agent automatically restores your chat history, token usage, and reasoning state — you pick up right where you left off.

#### Commands

| Command | Description |
|---------|-------------|
| `/save` | Explicitly save the current session with a timestamp name to `.sessions/` |
| `/load` | Open an interactive picker showing up to 5 most recent sessions to restore |
| `/new` | Start a fresh session (clears current in-memory history) |
| `/clear` | Clear history **and** delete the default session file (prevents auto-resume) |

#### Session Storage

- **Named sessions**: `<app_data>/.sessions/<name>.json` — created by `/save`, `/load`, and auto-save on exit
- **Default session**: `<app_data>/.session.json` (or custom `save_file` path) — used for auto-resume only
- **Pruning**: Only the 5 most recent timestamped sessions are kept; older ones are auto-removed
- The session files are gitignored by default

#### Auto-Save During Streaming

- **Double Ctrl+C during streaming**: Saves the conversation history **before** the interrupted turn, so earlier turns are preserved
- The interrupted (incomplete) turn is discarded, but all prior messages, token usage, and reasoning state are safely persisted

#### Session Data

Each saved session includes:

- Complete message history (preserving roles, content, tool calls, and reasoning)
- Cumulative token usage statistics
- Last reasoning output (for DeepSeek reasoning models)
- Unix timestamp of when the session was saved
- Optional human-readable name

### Interrupting Responses

- Press **Esc** or **Ctrl+C** once to interrupt the current response
- Press **Esc** twice quickly, or **Ctrl+C** twice quickly, to quit the agent
- Press **Ctrl+D** to quit via EOF

## Configuration Reference

Below is the full `config.toml` (optional, placed in the working directory):

```toml
[llm]
provider = "deepseek"           # deepseek, openai, anthropic, cohere, openrouter, custom
model = "deepseek-v4-pro"      # model name
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
enabled = false                 # Enable auto-resume (save default session, restore on start)
save_file = ".session.json"     # Default session file path for auto-resume
cleanup_undo_history = false    # Clean up undo history entries for this session on exit

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
