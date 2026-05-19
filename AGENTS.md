# AGENTS.md — My Code Agent

## ⚠️ Top Priority: Code Reading Workflow

**MANDATORY** — This rule supersedes all other instructions.

### Rule: outline-first, read-precise

When reading any source code file, you **MUST** follow this two-step workflow:

1. **Step 1: `file_outline`** — Always call `file_outline` **BEFORE** `file_read` to understand the file's structure (functions, structs, enums, impls, traits, modules with line ranges).
2. **Step 2: `file_read` (targeted)** — Use the outline to identify the **exact line range** you need, then read **only that range** using `offset` + `limit`. **NEVER** read an entire file when you only need a specific function or section.

### Why

- Saves tokens and context window budget
- Prevents information overload and missed details
- Enables precise, surgical code understanding

### Examples

✅ **Correct**:
1. `file_outline("src/tools/fs/file_read.rs")` → sees `fn execute()` at lines 45-120
2. `file_read("src/tools/fs/file_read.rs", offset=44, limit=76)` → reads only `execute()`

❌ **Wrong**:
- `file_read("src/tools/fs/file_read.rs")` without calling `file_outline` first
- Reading the entire 300-line file when you only need one function

### Exceptions

- Files **under 50 lines** — you may read directly without outline
- `AGENTS.md`, `Cargo.toml`, `config.toml` — config/doc files, read directly
- `@filepath` attachments from user — already loaded, don't re-read

---

## Quick Start

```bash
cargo build --release
cargo run --release

# Run a specific test
cargo test <test_name>
```

## Architecture

- **Entry point**: `src/main.rs` — delegates to `app::lifecycle::run_app`
- **Library root**: `src/lib.rs` — exports `app`, `core`, `mcp`, `tools`, `ui`
- **Runtime data dir**: `<cwd>/.mycode/` (or `MY_CODE_AGENT_HOME` if set) — config, logs, sessions, undo history, todos

### `src/app/` — Application Layer

- `mod.rs` — `App` struct, `ChatEntry`, `PendingConfirmation`, streaming UI state
- `bootstrap/` — `init_app()`: load config, build `Agent`, MCP tools, session resume
  - `knowledge.rs` — inject `knowledge.md` into system context
- `lifecycle.rs` — ratatui event loop; drains `StreamEvent` each frame
- `terminal.rs` — terminal setup/teardown
- `commands/` — Slash commands (`/help`, `/clear`, `/compact`, `/plan`, `/review`, …)
- `event_handler/` — keyboard, mouse, paste, picker widgets
  - `key_event/` — input, completion (`@`, `/`), shell mode
  - `picker.rs` — model / provider / session pickers

### `src/core/` — Core Functionality

- `types/` — `Message`, `ToolCall`, `StreamChunk`, `ToolDefinition`, review types (no external agent framework)
- `config/` — TOML loader (`Config`, `LLMConfig`, `ReviewConfig`, `TranslationConfig`, …)
- `paths/` — `app_dir()` → `.mycode/` (or `MY_CODE_AGENT_HOME`)
- `agent/` — LLM client and agent loop
  - `client.rs` — `LlmClient`, `ChatStream` (SSE parse → `StreamChunk`)
  - `stream_response.rs` — `stream_response()`, `process_sse_stream()`, `StreamEvent`, tool-turn loop
  - `stream/` — `spawn_llm_stream`, `process_streaming_events`, `check_stream_result`, compact/review hooks
  - `preamble.rs` — `Agent`, system prompt template, `build_client()`
  - `orchestrator.rs` — main agent ↔ `ReviewAgent` coordination, auto-review
  - `review_agent.rs` — phased code review pipeline
  - `connection.rs` — `ConnectionState` for UI status
- `context/` — `@filepath` expansion, `ContextManager`, caches, token usage, tool dedup
- `parser/` — tree-sitter smart read / `file_outline` (Rust, JS/TS, Java, HTML, Vue, Python)
- `session/` — save/load, `.sessions/` timestamped archives
- `translate.rs` — optional Chinese→English input translation

### `src/tools/` — Tool Implementations

Tools implement the local `Tool` trait (`name`, `definition`, `call`) and are registered in `ToolRegistry`.

| Module | Tools |
|--------|--------|
| `fs/` | `file_read`, `file_outline`, `file_write`, `file_update`, `apply_patch`, `file_delete`, `propose_str_replace`, `list_dir`, `glob`, `file_undo` |
| `exec/` | `shell_exec`, `spawn_agents`, `end_turn`, `confirmation`, `safety` |
| `git/` | `git_status`, `git_diff`, `git_log`, `git_commit` |
| `search/` | `code_search`, `code_review`, `explore_context`, `web_search`, `web_fetch` (MCP) |
| `infra/` | `write_todos`, `undo_history` |

- `mod.rs` — `Tool`, `ToolRegistry`, `from_config_and_handle()`, `create_mcp_tools()`
- `spawn_agents` is registered at runtime in `stream/spawn.rs` (needs `LlmClient` clone)

### `src/ui/` — Terminal UI

- `mod.rs` — root layout, dispatches chat / input / overlays / status
- `chat.rs`, `input.rs`, `status.rs`, `overlays.rs` — widgets
- `markdown.rs`, `render.rs` — markdown + reasoning display (`ReasoningTracker`, `StatefulTagStripper`)
- `terminal.rs` — banner, help text

### `src/mcp/` — Model Context Protocol

- `client.rs` — stdio JSON-RPC and HTTP transports
- `types.rs` — MCP protocol types
- Product integration today: Parallel Search HTTP (`web_search`, `web_fetch`) when `[mcp] enabled = true`

## Streaming Pipeline

1. **`LlmClient::stream_chat`** — OpenAI-compatible SSE from `reqwest` byte stream
2. **`ChatStream::next`** — split on `\n\n`, extract `data:` lines, `serde` → `StreamChunk`
3. **`process_sse_stream`** — map `delta.content` / `reasoning_content` / `tool_calls` → `StreamEvent` via unbounded channel
4. **`process_streaming_events`** (UI loop) — update `App.streaming_text`, `current_tool_call`, reasoning state
5. **`stream_response` tool loop** — on `finish_reason: tool_calls`, execute tools, append results, re-stream until stop or `max_turns`

## Key Dependencies

- **reqwest** — HTTP client; SSE streaming for chat completions
- **tokio** — async runtime, signals, processes
- **ratatui** + **tui-textarea-2** — terminal UI
- **serde/serde_json** — config, messages, tool args
- **anyhow/thiserror** — error handling
- **dotenv** — `.env` in `app_dir()` or project root
- **futures** — stream utilities
- **toml** — configuration
- **tree-sitter** (+ language grammars) — `file_outline` / smart read
- **async-process**, **async-trait** — MCP / tool async
- **tracing/tracing-subscriber** — logging to `.mycode/.my-code-agent.log`
- **unicode-width** — terminal width for CJK / emoji

## Configuration

`config.toml` is loaded from `app_dir()` (default: `.mycode/config.toml`). See `src/core/config/mod.rs` for all fields.

```toml
[llm]
provider = "deepseek"           # deepseek, openai, anthropic, cohere, openrouter, custom
model = "deepseek-v4-pro"
api_key_env = "DEEPSEEK_API_KEY"
base_url = "http://localhost:8080/v1"  # for "custom" provider
timeout_secs = 60               # 0 to disable

[files]
default_read_limit = 200
attach_max_lines = 500
attach_max_bytes = 51200        # 50 KB

[context]
window_size = 1048576           # 1M tokens
warn_threshold_percent = 75
critical_threshold_percent = 90
compact_retain_percent = 30     # /compact retention

[shell]
default_timeout_secs = 30

[agent]
max_turns = 100
thinking_display = "collapsed"  # "streaming" | "collapsed" | "hidden"
think_command = true
thinking_display_height = 5
show_tool_calls = false
show_tool_details = true

[session]
enabled = false                 # auto-save/resume .session.json on exit/start
save_file = ".session.json"     # relative to app_dir()
cleanup_undo_history = false

[mcp]
enabled = false
parallel_api_key = ""           # or PARALLEL_API_KEY env var

[review]
enabled = true
auto_review = true
threshold_lines = 5
max_review_iterations = 3

[translation]
enabled = true                  # auto-translate Chinese input (if configured)
model = ""                      # empty = provider default flash model
```

**Environment**

- `MY_CODE_AGENT_HOME` — override runtime directory (instead of `<cwd>/.mycode`)

## Important Patterns

### Adding a New Tool

1. Add implementation under the appropriate `src/tools/{fs,exec,git,search,infra}/` module
2. Implement `Tool` in that file (`async_trait::async_trait` + `Tool` from `tools/mod.rs`)
3. Register in `ToolRegistry::from_config_and_handle()` in `src/tools/mod.rs` (or register dynamically like `SpawnAgents`)
4. Document the tool in `PREAMBLE_TEMPLATE` in `src/core/agent/preamble.rs`
5. Add tests under `tests/` (e.g. `tests/tools/`, `tests/file_ops/`)

### Adding a New Slash Command

1. Create `src/app/commands/<name>.rs`
2. Export in `src/app/commands/mod.rs` and add to `handle_command()` match
3. Add to completion list in `src/app/event_handler/key_event/completion.rs` if needed
4. Document in `src/app/commands/help.rs`

### Tool Safety

`src/tools/exec/safety.rs` provides:

- `is_dangerous_deletion()` — risky paths (/, ~, etc.)
- `is_dangerous_shell_command()` — blocks `rm -rf`, redirects, etc.
- `is_dangerous_git_command()` — destructive git operations
- `is_dangerous_snippet_deletion()` — blocks `/**/` or `#![deny(*)]`

Destructive ops may also require UI confirmation via `ConfirmationHandle` (`shell_exec`, `git_commit`, `file_delete`).

### Session & Undo Persistence

- **Session**: `.mycode/.session.json` when `[session] enabled = true`; named saves under `.mycode/.sessions/`
- **Undo**: `.mycode/.undo_history.json` — `file_undo` / `/undo`; optional cleanup on exit via `cleanup_undo_history`
- **Todos**: `.mycode/.todos.json` via `write_todos` tool

### Code Review

- **`/review`** — manual review via `ReviewAgent`
- **Auto-review** — `AgentOrchestrator` after file-changing tools (`[review]` config)
- Fix→review loop up to `max_review_iterations`

## Testing

```bash
cargo test
cargo test --test agent
cargo test --test file_ops
cargo test -- --nocapture
```

Test layout: `tests/agent/`, `tests/core/`, `tests/file_ops/`, `tests/tools/`, `tests/search/`, `tests/config_ctx/`.

## Gotchas

- **Rust edition 2024** — use a recent toolchain: `rustup update`
- **API key** — `.env` in `app_dir()` or project root (gitignored)
- **No rig / external agent SDK** — custom `LlmClient` + `Tool` trait; types in `src/core/types/`
- **Tool naming**: snake_case (`file_read`, `shell_exec`)
- **`/plan` is prompt-only** — does not disable tools at the registry level; model may still call tools unless you add mode gating
- **Context**: preamble and file reads are cached; compaction triggers on window pressure or `/compact`
- **MCP stdio client exists** but only Parallel Search HTTP tools are wired in `create_mcp_tools()` today

## File References (`@filepath`)

- `@path` attaches file inline (expanded in `core/context/file_ref.rs`)
- `@path:N` starts at line N (0-indexed) — user syntax only
- Truncates at `attach_max_lines` / `attach_max_bytes` (defaults 500 lines / 50 KB)
- Tab completion with `@` prefix in the input area
