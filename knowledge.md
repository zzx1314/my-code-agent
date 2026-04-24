# My Code Agent — Project Knowledge

## What This Is
An interactive terminal-based AI coding assistant powered by **DeepSeek** with tool-augmented capabilities (read/write/search/execute files). Written in Rust (edition 2024).

## Key Code Locations
| Path | Purpose |
|------|---------|
| `src/main.rs` | CLI entry point, interactive loop, banner/help text |
| `src/lib.rs` | Library crate root (re-exports modules) |
| `src/preamble.rs` | Agent builder, preamble template, knowledge loading, API key check |
| `src/context.rs` | `@filepath` parsing and inline file expansion |
| `src/token_usage.rs` | Token usage tracking and reporting |
| `src/streaming.rs` | Streaming response handling |
| `src/ui.rs` | Terminal UI components (prompt, spinner, etc.) |
| `src/tools/mod.rs` | Tool registry — `all_tools()` returns all tool implementations |
| `src/tools/*.rs` | Individual tool implementations (one file per tool) |
| `tests/*.rs` | Integration tests (one file per module/tool) |

## Tools (6 total)
`file_read` · `file_write` · `file_update` · `file_delete` · `shell_exec` · `code_search`

Each tool struct implements `rig::tool::Tool` with associated `Error`, `Args`, and `Output` types. Tool definitions use `serde_json::json!` for parameter schemas.

## Commands

| Command | What it does |
|---------|-------------|
| `cargo build --release` | Build optimized binary |
| `cargo run --release` | Run the agent (requires `DEEPSEEK_API_KEY` in `.env`) |
| `cargo test` | Run all tests (integration tests in `tests/`) |
| `cargo test <module>` | Run tests for a specific module (e.g., `cargo test file_delete`) |
| `cargo clippy` | Run linter (not configured in CI — run manually) |
| `cargo fmt` | Format code (not configured in CI — run manually) |

## Key Dependencies
- **rig-core 0.35** — AI agent framework with tool support (the core abstraction)
- **tokio** — Async runtime (macros, rt-multi-thread, process, signal)
- **serde / serde_json** — Serialization for tool args/outputs
- **thiserror 2** — Derived error enums per tool
- **colored 3** — Terminal color output
- **dotenv 0.15** — `.env` file loading
- **tempfile 3** — Dev dependency for integration tests

## Conventions & Gotchas
- **No inline `#[cfg(test)]` modules in tool source files.** All tests live in the `tests/` directory as integration tests. Inline test modules should be removed (tool structs are public but helper functions like `build_diff` may be private — test them indirectly via tool output).
- **Rust edition 2024** — Very new edition with changed lifetime capture rules, `impl Trait` in `dyn Trait` positions, etc. May affect dependency compatibility.
- **API key via `.env`** — `DEEPSEEK_API_KEY` must be set in a `.env` file at project root. The file is gitignored.
- **Tool registration** — New tools must be added to: `src/tools/mod.rs` (mod + pub use + `all_tools()`), `src/main.rs` (preamble, banner, help text), and `README.md`.
- **Error pattern** — Each tool has its own `thiserror`-derived error enum (e.g., `FileDeleteError`, `FileUpdateError`) with variants for IO, not-found, and domain-specific errors.
- **`@filepath` expansion** — Handled in `context.rs`; files >500 lines or 50KB are truncated.
- **Ctrl+C once** = interrupt response; **Ctrl+C twice** = quit agent.
