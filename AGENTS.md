# AGENTS.md — My Code Agent

## Quick Start

```bash
cargo build --release
cargo run --release

# Run a specific test
cargo test <test_name>
```

## Architecture

- **Entry point**: `src/main.rs` - CLI REPL loop using reedline
- **Library root**: `src/lib.rs` — exports `core`, `tools`, `ui` modules
- **Core modules** (`src/core/`):
  - `config.rs` — TOML config loader with defaults
  - `preamble.rs` — Agent builder, sets up rig-core
  - `streaming.rs` — Response streaming, tool execution
  - `session.rs` — Session persistence (save/load)
  - `token_usage.rs` — Token tracking
  - `context.rs` — `@filepath` parsing and expansion
- **Tools** (`src/tools/`): 8 tools registered in `mod.rs`

## Key Dependencies

- **rig-core 0.35** — AI agent framework (defines `ToolDyn`, `Message`, `CompletionClient`)
- **tokio** — async runtime, signals
- **reedline** — Line editing, completion menu
- **termimad** — Markdown rendering
- **crossterm** — Terminal features
- **dotenv** — `.env` loading

## Configuration

`config.toml` (optional):

```toml
[llm]
provider = "deepseek"           # deepseek, openai, anthropic, cohere, custom
model = "deepseek-reasoner"      # model name
api_key_env = "DEEPSEEK_API_KEY"
base_url = "http://localhost:8080/v1"  # custom endpoint

[context]
window_size = 1048576           # 1M tokens
warn_threshold_percent = 75

[shell]
default_timeout_secs = 30

[session]
save_file = ".session.json"      # default
```

## Important Patterns

### Adding a New Tool

1. Create `src/tools/<tool_name>.rs` with struct implementing `Tool` trait from rig-core
2. Export in `src/tools/mod.rs`
3. Add to `all_tools()` in `tools/mod.rs`
4. Add tests in `tests/<tool_name>.rs`

### Tool Safety

`src/tools/safety.rs` provides:
- `is_dangerous_deletion()` — checks risky paths (/, ~, etc.)
- `is_dangerous_shell_command()` — blocks `rm -rf`, `> file`, etc.
- `is_dangerous_snippet_deletion()` — blocks `/**/` or `#![deny(*)]

Review safety module before allowing destructive operations.

### Session Persistence

- Auto-saves on quit to `.session.json` (gitignored)
- `/save <name>` and `/load <name>` commands
- `/sessions` lists available sessions

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

## File References (`@filepath`)

- `@path` attaches file inline
- `@path:N` starts at line N (0-indexed)
- Truncates at 500 lines / 50 KB
- Supports tab completion with `@` prefix