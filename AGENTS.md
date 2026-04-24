# Agents.md

## Build & Run

```bash
cargo build --release     # Build binary
cargo run --release       # Run (requires DEEPSEEK_API_KEY in .env)
cargo test                # Run all tests
cargo test <module>       # Run tests for specific module (e.g., file_delete)
cargo clippy              # Linter
cargo fmt                # Format code
```

## Setup

- **API key**: `DEEPSEEK_API_KEY` must exist in `.env` at project root (gitignored)
- **Rust edition**: 2024 — newer edition with changed lifetime capture rules

## Architecture

- **Single crate**: No workspace, `src/main.rs` → binary, `src/lib.rs` → library root
- **Entry point**: `src/main.rs` — interactive loop, Ctrl+C handling, streaming
- **Tool registry**: `src/tools/mod.rs` — `all_tools()` function returns `Vec<Box<dyn ToolDyn>>`
- **@filepath expansion**: `src/context.rs` — truncates at **500 lines** or **50 KB**

## Adding a New Tool

### Files to update (4 places):
1. `src/tools/` — create `new_tool.rs`
2. `src/tools/mod.rs` — add `pub mod new_tool;`, `pub use new_tool::NewTool;`, add to `all_tools()`
3. `src/main.rs` — add to preamble, banner, help text
4. `README.md` — add to tools table

### Pattern (see `src/tools/file_read.rs`):
```rust
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum NewToolError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Deserialize, Serialize)]
pub struct NewToolArgs {
    pub param: String,
}

#[derive(Deserialize, Serialize)]
pub struct NewToolOutput {
    pub result: String,
}

#[derive(Debug, Clone, Default)]
pub struct NewTool;

impl Tool for NewTool {
    const NAME: &'static str = "new_tool";
    type Error = NewToolError;
    type Args = NewToolArgs;
    type Output = NewToolOutput;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "...".to_string(),
            parameters: json!({...}),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        // implementation
    }
}
```

## Testing

- **Primary**: Integration tests in `tests/` directory (9 files)
- **Exception**: `src/context.rs` has inline `#[cfg(test)]` module with unit tests
- **Pattern**: Tests call tool directly: `ToolStruct.call(ToolArgs {...}).await`
- **Fixtures**: Use `tempfile::tempdir()` for file operations; direct commands for shell tests
- **Helper pattern** (from `tests/shell_exec.rs`):
```rust
async fn exec_cmd(command: &str, timeout_secs: u64, cwd: Option<&str>) -> ShellExecOutput {
    ShellExec.call(ShellExecArgs { ... }).await.unwrap()
}
```

## Key Dependencies

| Crate | Purpose |
|-------|---------|
| `rig-core 0.35` | AI agent framework (core abstraction) |
| `tokio` | Async runtime, process spawning, signals |
| `thiserror 2` | Per-tool error enums |
| `colored 3` | Terminal colors |
| `termimad` | Markdown rendering in terminal |

## Gotchas

- **No CI/pre-commit config** — run `cargo clippy` and `cargo fmt` manually before commits
- **Tool errors**: Each tool has its own `thiserror`-derived error enum (e.g., `FileReadError`, `FileUpdateError`)
- **Ctrl+C once** = interrupt response; **Ctrl+C twice** = quit agent