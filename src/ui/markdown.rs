//! Markdown-to-ratatui rendering (Codex-style: pulldown-cmark + fence unwrap).
//!
//! Agent messages use [`append_markdown_agent_with_cwd`] so `` ```markdown `` fences
//! containing tables are rendered as native tables, not fenced code.

use ratatui::text::Line;
use std::path::Path;

use crate::ui::md_render::agent_md::append_markdown_agent_with_cwd;

/// Render markdown for agent chat (unwraps `` ```md ``/`` ```markdown `` table fences).
pub fn render_markdown(text: &str, max_width: Option<usize>) -> Vec<Line<'static>> {
    let cwd = std::env::current_dir().ok();
    render_markdown_with_cwd(text, max_width, cwd.as_deref())
}

/// Render markdown with an explicit working directory for local file links.
pub fn render_markdown_with_cwd(
    text: &str,
    max_width: Option<usize>,
    cwd: Option<&Path>,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    append_markdown_agent_with_cwd(text, max_width, cwd, &mut lines);
    lines
}

/// Render streaming markdown (same pipeline; pulldown-cmark handles partial fences).
pub fn render_streaming_markdown(text: &str, max_width: Option<usize>) -> Vec<Line<'static>> {
    render_markdown(text, max_width)
}

/// Render complete markdown.
pub fn render_full_markdown(text: &str, max_width: Option<usize>) -> Vec<Line<'static>> {
    render_markdown(text, max_width)
}
