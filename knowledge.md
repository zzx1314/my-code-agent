# My Code Agent — Project Knowledge

## What This Is
An interactive terminal-based AI coding assistant powered by **DeepSeek** with tool-augmented capabilities (read/write/search/execute files). Written in Rust (edition 2024).

## Banner (ASCII)
```
 _                               _   
  _ __ ___  _   _    ___ ___   __| | ___    __ _  __ _  ___ _ __ | |_ 
 | '_ ` _ \| | | |  / __/ _ \ / _` |/ _ \  / _` |/ _` |/ _ \ '_ \| __|
 | | | | | | |_| | | (_| (_) | (_| |  __/ | (_| | (_| |  __/ | | | |_ 
 |_| |_| |_|\__, |  \___\___/ \__,_|\___|  \__,_|\__, |\___|_| |_|\__|
            |___/                                |___/ 
```

## Project Structure
```
src/
├── main.rs           # CLI entry point, interactive loop, banner/help text
├── lib.rs           # Library crate root (re-exports core, ui, tools)
├── core/            # Core functionality
│   ├── mod.rs      # Re-exports: context, preamble, streaming, token_usage
│   ├── context.rs  # @filepath parsing and inline file expansion
│   ├── preamble.rs # Agent builder, preamble template, knowledge loading, API key check
│   ├── streaming.rs# Streaming response handling
│   └── token_usage.rs # Token usage tracking and reporting
├── ui/              # Terminal UI
│   ├── mod.rs       # Re-exports: render, ui
│   ├── render.rs   # Markdown renderer, reasoning tracker
│   └── ui.rs       # Terminal UI components (prompt, banner, help, commands)
└── tools/           # Tool implementations
    ├── mod.rs       # Tool registry — all_tools()
    └── *.rs        # Individual tool implementations
tests/               # Integration tests (one file per module/tool)
```

## Key Code Locations
| Path | Purpose |
|------|---------|
| `src/main.rs` | CLI entry point, interactive loop |
| `src/lib.rs` | Library crate root (re-exports modules) |
| `src/core/preamble.rs` | Agent builder, preamble template, API key check |
| `src/core/context.rs` | `@filepath` parsing and inline file expansion |
| `src/core/token_usage.rs` | Token usage tracking and reporting |
| `src/core/streaming.rs` | Streaming response handling |
| `src/ui/ui.rs` | Terminal UI components (banner, help, commands) |
| `src/ui/render.rs` | Markdown rendering, reasoning tracking |
| `src/tools/mod.rs` | Tool registry — `all_tools()` |
| `src/tools/*.rs` | Individual tool implementations |
| `src/tools/list_dir.rs` | Directory listing with recursive depth |
| `src/tools/glob.rs` | File pattern matching (glob syntax) |

## Tools (8 total)
`file_read` · `file_write` · `file_update` · `file_delete` · `shell_exec` · `code_search` (ripgrep) · `list_dir` · `glob`

## Commands

| Command | What it does |
|---------|-------------|
| `cargo build --release` | Build optimized binary |
| `cargo run --release` | Run the agent (requires `DEEPSEEK_API_KEY` in `.env`) |
| `cargo test` | Run all tests |
| `cargo test <module>` | Run tests for a specific module |
| `cargo clippy` | Run linter |
| `cargo fmt` | Format code |

## Key Dependencies
- **rig-core 0.35** — AI agent framework
- **tokio** — Async runtime
- **serde / serde_json** — Serialization
- **thiserror 2** — Derived error enums
- **colored 3** — Terminal color output
- **dotenv 0.15** — `.env` file loading

## Conventions & Gotchas
- **No inline `#[cfg(test)]` modules** — Tests live in `tests/` directory
- **Rust edition 2024** — Very new edition
- **API key via `.env`** — `DEEPSEEK_API_KEY` must be set
- **Tool registration** — Add to `src/tools/mod.rs`, then update README
- **`@filepath` expansion** — Handled in `src/core/context.rs`; files >500 lines or 50KB truncated
- **Ctrl+C once** = interrupt response; **Ctrl+C twice** = quit
