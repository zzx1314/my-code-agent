use ratatui::prelude::*;

use crate::app::App;

use super::COLLAPSE_THRESHOLD;

/// Build styled reasoning lines (Codex-inspired: `• ` prefix, dim/italic, markdown).
/// Returns `None` when reasoning is empty, so callers can skip.
pub fn build_reasoning_lines(reasoning: &str, area_width: u16) -> Option<Vec<Line<'static>>> {
    if reasoning.trim().is_empty() {
        return None;
    }

    // Use area_width - 4 so the "  • " / "    " prefix (4 chars) doesn't
    // cause the Paragraph widget to re-wrap the line beyond area_width.
    // This keeps each content line at exactly 1 visual line, preventing
    // total_lines from fluctuating when the collapsed reasoning content
    // changes between frames during streaming.
    let wrap_width = (area_width as usize).saturating_sub(4).max(10);
    // Trim trailing whitespace from the reasoning text before markdown
    // rendering. LLM reasoning output often ends with paragraph-level
    // newlines (\n\n) — without trimming, these produce trailing empty
    // lines in the rendered output that stack with the explicit blank
    // lines added by the streaming layout to create double spacing.
    let rendered = crate::ui::render::render_full(reasoning.trim_end(), Some(wrap_width));
    if rendered.is_empty() {
        return None;
    }

    let summary_style = Style::default()
        .add_modifier(Modifier::DIM)
        .add_modifier(Modifier::ITALIC);
    let styled: Vec<Line<'static>> = rendered
        .into_iter()
        .map(|mut line| {
            line.spans = line
                .spans
                .into_iter()
                .map(|span| {
                    let fg = span.style.fg.unwrap_or(Color::DarkGray);
                    span.patch_style(summary_style.fg(fg))
                })
                .collect();
            line
        })
        .collect();

    if styled.is_empty() {
        return None;
    }

    // Prepend "• " prefix to first line, "  " to subsequent lines
    let prefixed: Vec<Line<'static>> = styled
        .into_iter()
        .enumerate()
        .map(|(i, line)| {
            let prefix = if i == 0 { "  • " } else { "    " };
            let mut prefixed = Line::from(Span::raw(prefix));
            prefixed.spans.extend(line.spans);
            prefixed
        })
        .collect();

    Some(prefixed)
}

/// Compute how many visual lines a `Line` occupies after word-wrap at `width`.
fn visual_lines(line: &Line<'_>, width: u16) -> u16 {
    let line_width = line.width() as u16;
    if line_width == 0 || width == 0 {
        1
    } else {
        (line_width + width - 1) / width
    }
}

/// Build a collapsible thinking header line (▶ collapsed / ▼ expanded).
fn build_thinking_header(collapsed: bool, hidden_count: usize) -> Line<'static> {
    if collapsed {
        Line::from(vec![
            Span::styled(
                "  ▶ ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "💭 Thinking... ",
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::ITALIC),
            ),
            Span::styled(
                format!("({} lines hidden)", hidden_count),
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::DIM),
            ),
        ])
    } else {
        Line::from(vec![
            Span::styled(
                "  ▼ ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "💭 Thinking...",
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::ITALIC),
            ),
        ])
    }
}

/// Render reasoning lines into the output, with collapsible support.
///
/// Always inserts a blank-line separator and "💭 Thinking..." header before the
/// content so distinct reasoning segments remain visually separated even when
/// each segment is shorter than `COLLAPSE_THRESHOLD`.
pub fn render_reasoning_inline(
    lines: &mut Vec<Line<'static>>,
    reasoning: &str,
    app: &mut App,
    section_id: &str,
    area_width: u16,
) {
    let Some(styled) = build_reasoning_lines(reasoning, area_width) else {
        return;
    };
    let total = styled.len();
    let collapsed = !app.collapsed_sections.contains(section_id);

    let vis_pos: u16 = lines.iter().map(|l| visual_lines(l, area_width)).sum();

    if total > COLLAPSE_THRESHOLD {
        // Build clickable header line
        let hidden_count = total - COLLAPSE_THRESHOLD;
        let header = build_thinking_header(collapsed, hidden_count);

        app.collapsed_toggles
            .push((vis_pos, section_id.to_string(), total));

        lines.push(header);

        if collapsed {
            // Show the LAST COLLAPSE_THRESHOLD lines (newest content visible by default).
            let start = total.saturating_sub(COLLAPSE_THRESHOLD);
            for line in styled.iter().skip(start) {
                lines.push(line.clone());
            }
        } else {
            // Show all lines when expanded
            for line in &styled {
                lines.push(line.clone());
            }
        }
    } else {
        // Small reasoning block: show a minimal header so even short segments
        // are visually separated from adjacent reasoning blocks.
        lines.push(Line::from(vec![Span::styled(
            "  💭 Thinking...",
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::ITALIC),
        )]));
        lines.extend(styled);
    }
}

/// Render an active streaming reasoning segment inline (Codex-style).
/// Shows the reasoning content with dim/italic style and `• ` prefix.
/// This handles only the currently streaming segment — completed segments
/// are rendered separately via `render_reasoning_inline` to match the
/// final per-entry output format.
pub fn render_streaming_reasoning_inline(
    lines: &mut Vec<ratatui::text::Line<'static>>,
    reasoning: &str,
    app: &mut App,
    section_id: &str,
    area_width: u16,
) {
    if app.config.agent.thinking_display == "hidden" {
        return;
    }

    if reasoning.trim().is_empty() {
        return;
    }

    if let Some(styled) = build_reasoning_lines(reasoning, area_width) {
        let section_id_owned = section_id.to_string();
        let collapsed = !app.collapsed_sections.contains(&section_id_owned);
        let total = styled.len();

        let vis_pos: u16 = lines.iter().map(|l| visual_lines(l, area_width)).sum();

        if total > COLLAPSE_THRESHOLD {
            let hidden_count = total - COLLAPSE_THRESHOLD;
            let header = build_thinking_header(collapsed, hidden_count);

            app.collapsed_toggles
                .push((vis_pos, section_id.to_string(), total));

            lines.push(header);

            if collapsed {
                let start = total.saturating_sub(COLLAPSE_THRESHOLD);
                for line in styled.iter().skip(start) {
                    lines.push(line.clone());
                }
            } else {
                for line in &styled {
                    lines.push(line.clone());
                }
            }
        } else {
            // Small active reasoning block: show a minimal header so even short
            // streaming segments are visually separated from adjacent blocks.
            lines.push(Line::from(vec![Span::styled(
                "  💭 Thinking...",
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::ITALIC),
            )]));
            lines.extend(styled);
        }
    }
}
