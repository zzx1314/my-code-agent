use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph, Wrap},
};
use unicode_width::UnicodeWidthStr;

use crate::app::{App, ChatEntry};
use crate::ui::render::render_full;

use super::reasoning::render_reasoning_inline;
use super::results::{
    count_diff_stats, render_collapsible_block, try_render_file_outline,
    try_render_file_tool_result, try_render_todos,
};
use super::word_wrap_text;

/// Render a user message with full-row background styling.
fn render_user_message(
    lines: &mut Vec<ratatui::text::Line<'static>>,
    entry: &ChatEntry,
    app: &App,
    area_width: u16,
) {
    // User message display with full-row background:
    // - Full-width background color across the entire terminal
    // - Top margin spacer (1 line) with background
    // - "› " prefix (bold, dim) on first line
    // - "  " continuation indent on wrapped/subsequent lines
    // - Each content line is padded with spaces to fill the full terminal width
    // - Bottom margin spacer (1 line) with background
    let user_bg = app.user_message_bg;
    let body_style = Style::default().fg(Color::Rgb(220, 220, 240)).bg(user_bg);
    let prefix_style = Style::default()
        .add_modifier(Modifier::BOLD)
        .add_modifier(Modifier::DIM)
        .bg(user_bg);
    let full_bg = Style::default().bg(user_bg);
    let area_w = area_width as usize;
    // Top margin spacer — full-width with background
    lines.push(Line::from(vec![Span::styled(" ".repeat(area_w), full_bg)]));
    if !entry.content.is_empty() {
        let wrap_width = area_width.saturating_sub(3).max(8) as usize;
        let message = entry.content.trim_end_matches(['\r', '\n']);
        let wrapped = word_wrap_text(message, wrap_width);
        for (i, line_text) in wrapped.iter().enumerate() {
            let prefix = if i == 0 { "› " } else { "  " };
            let padding = area_w.saturating_sub(prefix.width() + line_text.width());
            let padded_body = format!("{}{}", line_text, " ".repeat(padding));
            lines.push(Line::from(vec![
                Span::styled(prefix.to_string(), prefix_style),
                Span::styled(padded_body, body_style),
            ]));
        }
    }
    // Bottom margin spacer — full-width with background
    lines.push(Line::from(vec![Span::styled(" ".repeat(area_w), full_bg)]));
}

/// Render tool calls (⚙️ name + args) for an assistant message.
fn render_tool_calls(
    lines: &mut Vec<ratatui::text::Line<'static>>,
    tool_calls: &[crate::core::types::ToolCall],
    show_tool_details: bool,
) {
    for tc in tool_calls {
        let args: serde_json::Value =
            serde_json::from_str(&tc.function.arguments).unwrap_or(serde_json::Value::Null);
        lines.push(Line::from(vec![
            Span::styled("⚙️ ", Style::default().fg(Color::Yellow)),
            Span::styled(
                tc.function.name.clone(),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
        if show_tool_details {
            if let Some(cmd) = args.get("command").and_then(|c| c.as_str()) {
                lines.push(Line::from(format!("  {}", cmd)));
            } else {
                lines.push(Line::from(format!("  {}", args)));
            }
        }
    }
}

/// Render cached markdown content for an assistant message.
fn render_assistant_content(
    lines: &mut Vec<ratatui::text::Line<'static>>,
    entry: &ChatEntry,
    app: &mut App,
    max_width: Option<usize>,
) {
    if entry.content.is_empty() {
        return;
    }
    let cache_key = format!("{}|{}", entry.content, max_width.map_or(0, |w| w as isize));
    let md = if let Some(cached) = app.rendered_cache.get(&cache_key) {
        cached.clone()
    } else {
        let rendered = render_full(&entry.content, max_width);
        app.rendered_cache.insert(cache_key, rendered.clone());
        rendered
    };
    lines.extend(md);
}

/// Render an assistant message with tool calls, reasoning, and markdown content.
fn render_assistant_message(
    lines: &mut Vec<ratatui::text::Line<'static>>,
    entry: &ChatEntry,
    entry_idx: usize,
    app: &mut App,
    max_width: Option<usize>,
    area_width: u16,
    show_tool_calls: bool,
    show_tool_details: bool,
) {
    // Display tool calls (e.g. shell_exec) if present and config allows
    if show_tool_calls {
        if let Some(ref tool_calls) = entry.tool_calls {
            render_tool_calls(lines, tool_calls, show_tool_details);
            if !entry.content.is_empty() {
                lines.push(Line::default());
            }
        }
    }
    // Display reasoning inline (Codex style) if present
    if let Some(ref reasoning) = entry.reasoning_content {
        let section_id = format!("reason_{}", entry_idx);
        render_reasoning_inline(lines, reasoning, app, &section_id, area_width);
        // Add a blank line between reasoning and content to match streaming behavior
        if !entry.content.is_empty() {
            lines.push(Line::default());
        }
    }
    // Display normal content (cached to avoid re-parsing markdown every frame)
    render_assistant_content(lines, entry, app, max_width);
}

/// Render a ShellExec tool result (command, exit code, stdout/stderr).
/// Returns true if the content was successfully parsed as a ShellExec result.
fn render_shell_exec_result(
    lines: &mut Vec<ratatui::text::Line<'static>>,
    output: &serde_json::Value,
    entry_idx: usize,
    app: &mut App,
    area_width: u16,
) -> bool {
    let cmd = match output.get("command").and_then(|c| c.as_str()) {
        Some(cmd) => cmd,
        None => return false,
    };

    lines.push(Line::from(vec![
        Span::styled("⚙️ ", Style::default().fg(Color::Yellow)),
        Span::styled(
            "Shell Exec",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
    ]));
    lines.push(Line::from(format!("  Command: {}", cmd)));

    if let Some(exit_code) = output.get("exit_code") {
        let color = if exit_code.as_i64() == Some(0) {
            Color::Green
        } else {
            Color::Red
        };
        lines.push(Line::from(vec![
            Span::styled("  Exit Code: ", Style::default()),
            Span::styled(format!("{}", exit_code), Style::default().fg(color)),
        ]));
    }

    if let Some(timed_out) = output.get("timed_out").and_then(|t| t.as_bool()) {
        if timed_out {
            lines.push(Line::from(Span::styled(
                "  ⚠ Timed out",
                Style::default().fg(Color::Red),
            )));
        }
    }

    if let Some(stdout) = output.get("stdout").and_then(|s| s.as_str()) {
        if !stdout.is_empty() {
            lines.push(Line::from(Span::styled(
                "  ─── stdout ───",
                Style::default().fg(Color::DarkGray),
            )));
            let stdout_lines: Vec<Line> = stdout
                .lines()
                .map(|l| Line::from(format!("  {}", l)))
                .collect();
            let section_id = format!("so_{}", entry_idx);
            render_collapsible_block(lines, app, &section_id, stdout_lines, area_width);
        }
    }

    if let Some(stderr) = output.get("stderr").and_then(|s| s.as_str()) {
        if !stderr.is_empty() {
            lines.push(Line::from(Span::styled(
                "  ─── stderr ───",
                Style::default().fg(Color::Red).add_modifier(Modifier::DIM),
            )));
            let stderr_lines: Vec<Line> = stderr
                .lines()
                .map(|l| Line::from(format!("  {}", l)))
                .collect();
            let section_id = format!("se_{}", entry_idx);
            render_collapsible_block(lines, app, &section_id, stderr_lines, area_width);
        }
    }

    true
}

/// Render a tool message — file ops, todos, shell exec, file outline, or fallback.
/// Returns `true` if any content was rendered (visible to the user), `false` if
/// the message was silently hidden (e.g. non-file tool calls with show_tool_calls disabled).
fn render_tool_message(
    lines: &mut Vec<ratatui::text::Line<'static>>,
    entry: &ChatEntry,
    entry_idx: usize,
    app: &mut App,
    max_width: Option<usize>,
    area_width: u16,
    show_tool_calls: bool,
    show_tool_details: bool,
) -> bool {
    // File tool results (file_write, file_update, file_delete) with git_diff
    // are ALWAYS shown — they contain substantive code changes.
    if try_render_file_tool_result(lines, &entry.content, entry_idx, app, true, area_width)
        .is_some()
    {
        return true;
    }

    // Todos results are ALWAYS shown — they contain planning progress.
    if try_render_todos(lines, &entry.content, max_width).is_some() {
        return true;
    }

    // Other tool results are only shown when show_tool_calls is enabled
    if !(show_tool_calls && show_tool_details) {
        return false;
    }

    // Parse the tool result (ShellExecOutput JSON) for nice display
    if let Ok(output) = serde_json::from_str::<serde_json::Value>(&entry.content) {
        if render_shell_exec_result(lines, &output, entry_idx, app, area_width) {
            return true;
        }
    }

    // Check if it's a file_outline result
    if try_render_file_outline(lines, &entry.content, entry_idx, app, area_width).is_some() {
        return true;
    }

    // Fallback: show raw content for non-shell tool results
    if !entry.content.is_empty() {
        lines.push(Line::from(vec![Span::styled(
            "🔧 Tool Result:",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )]));
        lines.push(Line::from(entry.content.to_string()));
        return true;
    }

    false
}

/// Render a single message with role-based styling.
/// Returns `true` if content was actually rendered (visible to the user),
/// `false` if the message was silently skipped (e.g. hidden tool calls).
pub(super) fn render_message(
    lines: &mut Vec<ratatui::text::Line<'static>>,
    entry: &ChatEntry,
    entry_idx: usize,
    app: &mut App,
    max_width: Option<usize>,
    show_tool_calls: bool,
    show_tool_details: bool,
) -> bool {
    let area_width = max_width.unwrap_or(80) as u16;
    match entry.role.as_str() {
        "user" => {
            render_user_message(lines, entry, app, area_width);
            true
        }
        "assistant" => {
            render_assistant_message(
                lines,
                entry,
                entry_idx,
                app,
                max_width,
                area_width,
                show_tool_calls,
                show_tool_details,
            );
            true
        }
        "tool" => {
            let rendered = render_tool_message(
                lines,
                entry,
                entry_idx,
                app,
                max_width,
                area_width,
                show_tool_calls,
                show_tool_details,
            );
            // Only mark as rendered if the tool message actually produced
            // visible output — hidden tool messages (show_tool_calls=false)
            // should not update prev_role, otherwise consecutive hidden
            // tool entries between two assistant messages create double
            // blank lines (one for assistant→tool, one for tool→assistant).
            rendered
        }
        _ => {
            lines.push(Line::from(format!("{}: {}", entry.role, entry.content)));
            true
        }
    }
}

/// Render chat with reasoning placed before the last assistant message.
/// Uses the new inline reasoning style (Codex-inspired: `• ` prefix, dim/italic).
pub(super) fn render_chat_with_reasoning(
    lines: &mut Vec<ratatui::text::Line<'static>>,
    app: &mut App,
    max_width: Option<usize>,
) {
    let last_assistant_idx = app
        .chat_history
        .iter()
        .rposition(|entry| entry.role == "assistant");
    let split_idx = last_assistant_idx.unwrap_or(app.chat_history.len());

    let show_tool_calls_in_history = app.config.agent.show_tool_calls_in_history;
    let mut last_visible_role: Option<String> = None;

    // Clone entries before the last assistant to avoid borrow conflict with &mut App
    let before: Vec<(usize, ChatEntry)> = app.chat_history[..split_idx]
        .iter()
        .enumerate()
        .map(|(i, e)| (i, e.clone()))
        .collect();
    for (i, entry) in &before {
        if entry.role != "tool" {
            if last_visible_role.is_some() {
                lines.push(Line::default());
            }
        }
        let rendered = render_message(
            lines,
            entry,
            *i,
            app,
            max_width,
            show_tool_calls_in_history,
            app.config.agent.show_tool_details,
        );
        if rendered {
            last_visible_role = Some(entry.role.clone());
        }
    }

    // Render the last assistant message inline — its reasoning_content will be
    // rendered by render_message via the new inline reasoning helper.
    if let Some(idx) = last_assistant_idx {
        if last_visible_role.is_some() {
            lines.push(Line::default());
        }
        let entry = app.chat_history[idx].clone();
        render_message(
            lines,
            &entry,
            idx,
            app,
            max_width,
            show_tool_calls_in_history,
            app.config.agent.show_tool_details,
        );
    }
}

/// Render all chat messages in order.
pub(super) fn render_chat_messages(
    lines: &mut Vec<ratatui::text::Line<'static>>,
    app: &mut App,
    max_width: Option<usize>,
) {
    let show_tool_calls_in_history = app.config.agent.show_tool_calls_in_history;
    // Clone entries to avoid borrow conflict with &mut App
    let entries: Vec<(usize, ChatEntry)> = app
        .chat_history
        .iter()
        .enumerate()
        .map(|(i, e)| (i, e.clone()))
        .collect();
    let mut last_visible_role: Option<String> = None;
    for (i, entry) in &entries {
        // Add separator blank line before visible (non-tool) messages when a
        // visible message precedes them. Tool entries are transparent — they
        // never add blank lines and never update the visible-role tracker.
        if entry.role != "tool" {
            if last_visible_role.is_some() {
                lines.push(Line::default());
            }
        }
        let rendered = render_message(
            lines,
            entry,
            *i,
            app,
            max_width,
            show_tool_calls_in_history,
            app.config.agent.show_tool_details,
        );
        if rendered {
            last_visible_role = Some(entry.role.clone());
        }
    }
}

/// Render the startup banner — bordered info panel wrapping tightly around content.
pub(super) fn render_banner(f: &mut Frame, app: &mut App, area: Rect) {
    let model = app
        .config
        .llm
        .model
        .as_deref()
        .map(crate::app::model_display_name)
        .unwrap_or_else(|| "unknown".to_string());
    let dir = {
        let cwd = std::env::current_dir().unwrap_or_default();
        if let Some(home) = dirs::home_dir() {
            if cwd == home {
                "~".to_string()
            } else if let Ok(rel) = cwd.strip_prefix(&home) {
                format!("~/{}/", rel.display())
            } else {
                cwd.display().to_string()
            }
        } else {
            cwd.display().to_string()
        }
    };
    let title_style = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    let dim = Style::default()
        .fg(Color::LightYellow)
        .add_modifier(Modifier::DIM);
    let value_style = Style::default().fg(Color::Cyan);

    let lines = vec![
        Line::from(vec![Span::styled(" >_ My Code Agent", title_style)]),
        Line::from(""),
        Line::from(vec![
            Span::styled(" model: ", dim),
            Span::styled(model, value_style),
            Span::styled("   ", dim),
            Span::styled("/model to change", dim),
        ]),
        Line::from(vec![
            Span::styled(" directory: ", dim),
            Span::styled(dir, value_style),
        ]),
    ];

    // Border fits content width + 2 for left/right borders + 2 for inner padding.
    let content_width = lines.iter().map(|l| l.width()).max().unwrap_or(0);
    let box_width = (content_width + 4).min(area.width as usize);

    // Inner text width available after borders (2) + padding (2).
    let text_width = box_width.saturating_sub(4).max(1);

    // Calculate total height accounting for line wrapping when the terminal
    // is narrower than the content (e.g. long directory paths).
    let total_text_rows: u16 = lines
        .iter()
        .map(|l| {
            let w = l.width();
            if w == 0 {
                1u16
            } else {
                ((w as u16) + (text_width as u16) - 1) / (text_width as u16).max(1)
            }
        })
        .sum();

    // Height = wrapped content lines + 2 border rows (top + bottom)
    let box_height = (total_text_rows + 2).min(area.height);

    let box_area = Rect {
        x: area.x,
        y: area.y,
        width: box_width as u16,
        height: box_height,
    };

    app.total_lines = box_area.height;
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .border_style(Style::default().fg(Color::Cyan));

    let paragraph = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .block(block);
    f.render_widget(paragraph, box_area);
}

/// Render review reasoning block — transient thinking content shown during code review.
/// Uses a blockquote style with `│ ` prefix and a fixed height so the content below
/// doesn't jump as reasoning streams in. Each line is pre-word-wrapped to
/// `area_width - 2` (for the `"│ "` prefix) to prevent Paragraph re-wrapping and
/// the resulting visual height fluctuations that cause flickering.
pub fn render_review_reasoning(
    lines: &mut Vec<ratatui::text::Line<'static>>,
    app: &mut App,
    max_width: Option<usize>,
    max_height: u16,
) {
    if !app.is_reviewing && app.review_reasoning.is_empty() {
        return;
    }

    let area_width = max_width.unwrap_or(80) as u16;

    if app.review_reasoning.is_empty() {
        // Pad with empty lines to maintain fixed height even when no content
        if max_height > 0 {
            let header_reserve: u16 = 2; // header + trailing empty
            let content_budget = max_height.saturating_sub(header_reserve).max(1);
            lines.push(Line::from(vec![Span::styled(
                "  💭 Thinking...",
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::ITALIC),
            )]));
            for _ in 0..content_budget {
                lines.push(Line::default());
            }
            lines.push(Line::default());
        }
        return;
    }

    // Use build_reasoning_lines for proper markdown rendering with • prefix
    // (same rendering pipeline as the main agent)
    let Some(styled) = super::reasoning::build_reasoning_lines(&app.review_reasoning, area_width)
    else {
        return;
    };

    let total = styled.len();

    // Reserve lines for: header and trailing empty line
    let header_reserve: u16 = 2;
    let content_budget = max_height.saturating_sub(header_reserve).max(1) as usize;

    let vis_pos: u16 = lines.iter().map(|l| super::reasoning::visual_lines(l, area_width)).sum();

    if total > content_budget {
        let hidden_count = total - content_budget;
        let section_id = "review_reasoning";
        let collapsed = !app.collapsed_sections.contains(section_id);

        // Build clickable header line (same as main agent)
        let header = super::reasoning::build_thinking_header(collapsed, hidden_count);
        app.collapsed_toggles
            .push((vis_pos, section_id.to_string(), total));
        lines.push(header);

        if collapsed {
            let start = total.saturating_sub(content_budget);
            for line in styled.iter().skip(start) {
                lines.push(line.clone());
            }
        } else {
            for line in &styled {
                lines.push(line.clone());
            }
        }
    } else {
        // Small reasoning block: show header like main agent
        lines.push(Line::from(vec![Span::styled(
            "  💭 Thinking...",
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::ITALIC),
        )]));
        lines.extend(styled);
        // Pad with empty placeholder lines to keep a fixed height
        let mut content_lines_added = total as u16;
        while content_lines_added < content_budget as u16 {
            lines.push(Line::default());
            content_lines_added += 1;
        }
    }

    lines.push(Line::default());
}

/// Render status messages at the bottom.
pub fn render_status_messages(
    lines: &mut Vec<ratatui::text::Line<'static>>,
    app: &App,
    area: Rect,
) {
    if app.status_messages.is_empty() {
        return;
    }
    lines.push(Line::from(Span::styled(
        "─".repeat(area.width as usize),
        Style::default().fg(Color::DarkGray),
    )));
    for msg in &app.status_messages {
        for line in msg.lines() {
            lines.push(Line::from(line.to_string()));
        }
    }
}

/// Render an aggregated file change summary block at the end of a turn.
/// Inspired by Codex's change summary display: shows a compact list of
/// all files that were modified in the current turn with +X -Y stats.
/// Only renders when not streaming, and when there are actual changes.
pub fn render_file_change_summary(
    lines: &mut Vec<ratatui::text::Line<'static>>,
    app: &App,
    max_width: Option<usize>,
) {
    if app.is_streaming {
        return;
    }

    // Find the most recent user message boundary — scan backward from the end
    // of chat_history for the last user message.
    let turn_start = app.chat_history.iter().rposition(|e| e.role == "user");
    let start_idx = turn_start.unwrap_or(0);

    // Aggregate per-file stats from tool entries in the current turn
    let mut per_file: Vec<(String, usize, usize)> = Vec::new();
    let mut total_adds = 0usize;
    let mut total_dels = 0usize;

    for entry in app.chat_history[start_idx..].iter() {
        if entry.role != "tool" {
            continue;
        }
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&entry.content) {
            // Only show file-change results (file_write, file_update, file_delete)
            let has_file_op = value.get("bytes_written").is_some()
                || value.get("replacements").is_some()
                || value.get("deleted_type").is_some();
            if !has_file_op {
                continue;
            }
            if let Some(path) = value.get("path").and_then(|p| p.as_str()) {
                let diff_str = value.get("git_diff").and_then(|v| v.as_str()).unwrap_or("");
                let (adds, dels) = count_diff_stats(diff_str);
                if adds > 0 || dels > 0 {
                    total_adds += adds;
                    total_dels += dels;
                    per_file.push((path.to_string(), adds, dels));
                }
            }
        }
    }

    if per_file.is_empty() {
        return;
    }

    let area_width = max_width.unwrap_or(80) as usize;

    // Separator line
    lines.push(Line::from(Span::styled(
        "─".repeat(area_width),
        Style::default().fg(Color::DarkGray),
    )));

    // Header: "📝 N files changed: +X -Y"
    lines.push(Line::from(vec![
        Span::styled("📝 ", Style::default().fg(Color::Cyan)),
        Span::styled(
            format!(
                "{} file{} changed: ",
                per_file.len(),
                if per_file.len() == 1 { "" } else { "s" }
            ),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("+{} -{}", total_adds, total_dels),
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
    ]));

    // Per-file entries
    for (path, adds, dels) in &per_file {
        let (add_color, del_color) = if *adds > 0 && *dels > 0 {
            (Color::Green, Color::Red)
        } else if *adds > 0 {
            (Color::Green, Color::DarkGray)
        } else {
            (Color::DarkGray, Color::Red)
        };

        lines.push(Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(path.clone(), Style::default().fg(Color::Cyan)),
            Span::styled("  ", Style::default()),
            Span::styled(
                format!("+{}", adds),
                Style::default().fg(add_color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!(" -{}", dels), Style::default().fg(del_color)),
        ]));
    }

    lines.push(Line::default());
}
