use ratatui::prelude::*;

use crate::app::App;

use super::COLLAPSE_THRESHOLD;
use crate::ui::render::render_full;

/// Render a collapsible block of `Line`s. If the number of content lines exceeds
/// `COLLAPSE_THRESHOLD` and the section is in the collapsed set, only the first
/// `COLLAPSE_THRESHOLD` lines are shown with a toggle to expand. If expanded, all
/// lines are shown with a toggle to collapse.
///
/// `area_width` is the terminal width used to compute visual line positions (after
/// word-wrap). Toggle positions are stored as visual line indices so that mouse clicks
/// (which also operate in visual line space) hit the correct toggle even when lines wrap.
pub fn render_collapsible_block<'a>(
    lines: &mut Vec<ratatui::text::Line<'a>>,
    app: &mut App,
    section_id: &str,
    content: Vec<ratatui::text::Line<'a>>,
    area_width: u16,
) {
    let total = content.len();
    let collapsed = !app.collapsed_sections.contains(section_id);

    /// Compute how many visual lines a `Line` occupies after word-wrap at `width`.
    fn visual_lines(line: &ratatui::text::Line<'_>, width: u16) -> u16 {
        let line_width = line.width() as u16;
        if line_width == 0 || width == 0 {
            1 // empty lines still occupy one row
        } else {
            (line_width + width - 1) / width
        }
    }

    // The current visual line position (after word-wrap) in the `lines` buffer.
    let mut vis_pos: u16 = lines.iter().map(|l| visual_lines(l, area_width)).sum();

    if total > COLLAPSE_THRESHOLD {
        if collapsed {
            // Show first COLLAPSE_THRESHOLD lines
            for line in content.into_iter().take(COLLAPSE_THRESHOLD) {
                vis_pos += visual_lines(&line, area_width);
                lines.push(line);
            }
            app.collapsed_toggles
                .push((vis_pos, section_id.to_string(), total));
            lines.push(ratatui::text::Line::from(vec![
                ratatui::text::Span::styled(
                    format!(
                        "  [+ {} more lines - click to expand]",
                        total - COLLAPSE_THRESHOLD
                    ),
                    ratatui::style::Style::default()
                        .fg(ratatui::style::Color::Yellow)
                        .add_modifier(ratatui::style::Modifier::BOLD),
                ),
            ]));
        } else {
            // Show all lines
            for line in content {
                vis_pos += visual_lines(&line, area_width);
                lines.push(line);
            }
            app.collapsed_toggles
                .push((vis_pos, section_id.to_string(), total));
            lines.push(ratatui::text::Line::from(vec![
                ratatui::text::Span::styled(
                    "  [-] click to collapse",
                    ratatui::style::Style::default()
                        .fg(ratatui::style::Color::Yellow)
                        .add_modifier(ratatui::style::Modifier::BOLD),
                ),
            ]));
        }
    } else {
        // Small enough, show all without toggle
        for line in content {
            lines.push(line);
        }
    }
}

/// Parse a unified diff string and return (additions, deletions) counts.
pub fn count_diff_stats(diff: &str) -> (usize, usize) {
    let mut adds = 0usize;
    let mut dels = 0usize;
    for line in diff.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('+') && !trimmed.starts_with("+++") {
            adds += 1;
        } else if trimmed.starts_with('-') && !trimmed.starts_with("---") {
            dels += 1;
        }
    }
    (adds, dels)
}

pub fn try_render_file_tool_result(
    lines: &mut Vec<ratatui::text::Line>,
    content: &str,
    entry_idx: usize,
    app: &mut App,
    show_git_diff: bool,
    area_width: u16,
) -> Option<()> {
    let value: serde_json::Value = serde_json::from_str(content).ok()?;

    // Check if this is a file tool result by looking for a path field
    let path = value.get("path")?.as_str()?;

    // Get the current diff from git (always up-to-date, not stale from tool output)
    // Results are cached per frame in git_diff_cache to avoid spawning git
    // subprocesses for the same file multiple times in one render pass.
    // When a review baseline exists, diff against it instead of HEAD
    // to show only changes since the last review.
    let git_diff_from_tool = value.get("git_diff").and_then(|v| v.as_str()).unwrap_or("");
    let git_diff = if let Some(cached) = app.git_diff_cache.get(path) {
        cached.clone()
    } else {
        let mut git_args = vec!["diff", "--no-color"];
        git_args.push("--");
        git_args.push(path);
        let diff = match std::process::Command::new("git").args(&git_args).output() {
            Ok(o) if o.status.success() => {
                let s = String::from_utf8_lossy(&o.stdout);
                if s.trim().is_empty() {
                    git_diff_from_tool.to_string()
                } else {
                    s.to_string()
                }
            }
            _ => git_diff_from_tool.to_string(),
        };
        app.git_diff_cache.insert(path.to_string(), diff.clone());
        diff
    };
    let git_diff = git_diff.as_str();

    // Determine the action type
    let action = if value.get("bytes_written").is_some() {
        "File Write"
    } else if value.get("replacements").is_some() {
        "File Update"
    } else if let Some(deleted_type) = value.get("deleted_type").and_then(|t| t.as_str()) {
        match deleted_type {
            "file" => "File Delete",
            "directory" => "Directory Delete",
            "snippet" => "Snippet Delete",
            _ => return None,
        }
    } else {
        // Not a file modification (e.g. file_read) — don't render a header
        return None;
    };

    // Header
    lines.push(Line::from(vec![
        Span::styled("📝 ", Style::default().fg(Color::Cyan)),
        Span::styled(
            action,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  File: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            path.to_string(),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
    ]));

    // Show the git diff (collapsible)
    if show_git_diff && !git_diff.is_empty() {
        // Compute diff stats for display
        let (adds, dels) = count_diff_stats(git_diff);
        lines.push(Line::from(vec![
            Span::styled("  ─── git diff ", Style::default().fg(Color::Green)),
            Span::styled(
                format!("+{} -{}", adds, dels),
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" ───", Style::default().fg(Color::Green)),
        ]));

        // Build styled diff lines with preserved @@ hunk headers
        let diff_lines: Vec<Line> = git_diff
            .lines()
            .map(|line| {
                let line = line.trim_end();
                let display_line = if line.is_empty() { " " } else { line };

                let (prefix, style) = if line.starts_with("@@") {
                    (
                        format!("  {}", display_line),
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    )
                } else if line.starts_with('+') && !line.starts_with("+++") {
                    (
                        format!("  {}", display_line),
                        Style::default().fg(Color::Green),
                    )
                } else if line.starts_with('-') && !line.starts_with("---") {
                    (
                        format!("  {}", display_line),
                        Style::default().fg(Color::Red),
                    )
                } else if line.starts_with("+++") || line.starts_with("---") {
                    (
                        format!("  {}", display_line),
                        Style::default()
                            .fg(Color::DarkGray)
                            .add_modifier(Modifier::BOLD),
                    )
                } else if line.starts_with("diff ")
                    || line.starts_with("index ")
                    || line.starts_with("rename ")
                    || line.starts_with("similarity ")
                    || line.starts_with("new file ")
                    || line.starts_with("deleted file ")
                {
                    (
                        format!("  {}", display_line),
                        Style::default().fg(Color::DarkGray),
                    )
                } else if line.starts_with('\\') {
                    (
                        format!("  {}", display_line),
                        Style::default().fg(Color::DarkGray),
                    )
                } else {
                    (format!("  {}", display_line), Style::default())
                };

                Line::from(Span::styled(prefix, style))
            })
            .collect();

        let section_id = format!("gd_{}", entry_idx);
        render_collapsible_block(lines, app, &section_id, diff_lines, area_width);
    }

    Some(())
}

/// Try to parse tool content as a file_outline result and render it with colored spans.
/// Returns Some(()) if the content was successfully rendered as a file outline.
pub fn try_render_file_outline(
    lines: &mut Vec<ratatui::text::Line>,
    content: &str,
    entry_idx: usize,
    app: &mut App,
    area_width: u16,
) -> Option<()> {
    let value: serde_json::Value = serde_json::from_str(content).ok()?;
    let outline = value.get("outline")?.as_str()?;
    let path = value.get("path")?.as_str()?;

    // Header: 📋 File Outline + path
    lines.push(Line::from(vec![
        Span::styled("📋 ", Style::default().fg(Color::Cyan)),
        Span::styled(
            "File Outline",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  File: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            path.to_string(),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
    ]));

    // Build styled outline lines
    let mut outline_content: Vec<Line> = Vec::new();
    for line in outline.lines() {
        let line = line.trim_end();
        if line.is_empty() {
            continue;
        }

        // Total line: "Total: N lines"
        if let Some(total) = line.strip_prefix("Total: ") {
            outline_content.push(Line::from(vec![
                Span::styled("  ", Style::default()),
                Span::styled("── ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("Total: {}", total),
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
            continue;
        }

        // Structure line format: "├── [1-10: 10 lines] fn main"
        // or "└── [1-10: 10 lines] fn main"
        if let Some((_prefix, rest)) = line.split_once("── ") {
            let tree_char = line.chars().next().unwrap_or(' ');

            let mut spans = Vec::new();
            // Tree prefix character
            spans.push(Span::styled(
                format!("  {}", tree_char),
                Style::default().fg(Color::DarkGray),
            ));

            if let Some((range_str, rest_after_range)) = rest.split_once("] ") {
                // Range: "── [1-10: 10 lines"
                let range_part = format!("── {}", range_str);
                spans.push(Span::styled(range_part, Style::default().fg(Color::Blue)));
                spans.push(Span::styled("] ", Style::default().fg(Color::Blue)));

                // Split rest into kind and name
                let after_range = rest_after_range.trim_start();
                if let Some((kind, name)) = after_range.split_once(char::is_whitespace) {
                    let name = name.trim_start();
                    spans.push(Span::styled(
                        kind.to_string(),
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ));
                    if !name.is_empty() {
                        spans.push(Span::styled(
                            format!(" {}", name),
                            Style::default().fg(Color::Green),
                        ));
                    }
                } else {
                    spans.push(Span::styled(
                        after_range.to_string(),
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ));
                }
            } else {
                // Fallback: show the rest as-is
                let rest_display = format!("── {}", rest);
                spans.push(Span::styled(rest_display, Style::default()));
            }

            outline_content.push(Line::from(spans));
        } else {
            // Fallback for lines that don't match the tree format
            outline_content.push(Line::from(format!("  {}", line)));
        }
    }

    // Render collapsible outline content
    let section_id = format!("ol_{}", entry_idx);
    render_collapsible_block(lines, app, &section_id, outline_content, area_width);

    Some(())
}

pub fn try_render_todos(
    lines: &mut Vec<ratatui::text::Line>,
    content: &str,
    max_width: Option<usize>,
) -> Option<()> {
    // Detect Markdown-formatted todos output (starts with the todos header)
    if !content.starts_with("## 📋 Todos") {
        return None;
    }

    let md = render_full(content, max_width);
    lines.extend(md);
    Some(())
}
