use ratatui::{prelude::*, widgets::Paragraph};

use crate::app::App;

/// Render the bottom status bar showing model, token usage, and state.
pub fn render_status_bar(f: &mut Frame, app: &mut App, area: Rect) {
    let mut spans = Vec::new();

    spans.push(Span::styled(
        format!(
            "Model: {}",
            app.config.llm.model.as_deref().unwrap_or("unknown")
        ),
        Style::default().fg(Color::DarkGray),
    ));

    if let Some(ref turn_line) = app.turn_usage_line {
        spans.push(Span::styled(
            format!(" | {}", turn_line),
            Style::default().fg(Color::DarkGray),
        ));
        // Show per-turn cache hit rate if available
        if let Some(cache_line) =
            crate::core::context::context_cache::global_cache().format_turn_cache_line()
        {
            spans.push(Span::styled(
                format!(" | {}", cache_line),
                Style::default().fg(Color::DarkGray),
            ));
        }
    }

    if app.is_streaming {
        let dot_cycle = (app.marquee_frame / 4) % 4;
        let dots = ".".repeat(dot_cycle as usize);
        spans.push(Span::styled(
            format!(" | Streaming{}", dots),
            Style::default().fg(Color::Yellow),
        ));
    } else if app.is_reviewing {
        let dot_cycle = (app.marquee_frame / 4) % 4;
        let dots = ".".repeat(dot_cycle as usize);
        spans.push(Span::styled(
            format!(" | 🔍 Reviewing{}", dots),
            Style::default().fg(Color::Cyan),
        ));
    } else if let Some(ref msg) = app.review_complete_message {
        if app.review_complete_timer > 0 {
            let verdict_color = app.review_complete_verdict
                .as_ref()
                .map(|v| match v {
                    crate::core::types::review::ReviewVerdict::Approved => Color::Green,
                    crate::core::types::review::ReviewVerdict::NeedsRevision => Color::Yellow,
                    crate::core::types::review::ReviewVerdict::Rejected => Color::Red,
                })
                .unwrap_or(Color::Green);
            spans.push(Span::styled(
                format!(" | {}", msg),
                Style::default().fg(verdict_color),
            ));
        } else {
            spans.push(Span::styled(" | Ready", Style::default().fg(Color::Green)));
        }
    } else if app.shell_mode {
        spans.push(Span::styled(
            " | 🐚 Shell",
            Style::default().fg(Color::Cyan),
        ));
    } else {
        spans.push(Span::styled(" | Ready", Style::default().fg(Color::Green)));
    }

    let status_bar = Paragraph::new(Line::from(spans));
    f.render_widget(status_bar, area);
}
