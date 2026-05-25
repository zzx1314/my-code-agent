//! Codex-style markdown rendering (pulldown-cmark + syntect).
//!
//! Adapted from OpenAI Codex TUI (`codex-rs/tui`), Apache-2.0.

// Vendored from Codex; many helpers are used by streaming/table paths we have not wired yet.
#![allow(dead_code)]

pub mod agent_md;
pub mod color;
pub mod markdown_render;
pub mod string_util;
pub mod table_detect;
pub mod terminal_palette;
pub mod wrapping;

pub mod render;

pub use agent_md::append_markdown_agent_with_cwd;
