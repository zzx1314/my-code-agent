use ratatui::{prelude::*, widgets::Paragraph};

use crate::app::App;

/// Render the bottom status bar showing model, token usage, and state.
pub fn render_status_bar(f: &mut Frame, app: &mut App, area: Rect) {
    let mut spans = Vec::new();

    // Use display alias so long OpenRouter model names are shortened.
    let model_label = app
        .config
        .llm
        .model
        .as_deref()
        .map(crate::app::model_display_name)
        .unwrap_or_else(|| "unknown".to_string());
    spans.push(Span::styled(
        format!("Model: {}", model_label),
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

    if app.translating {
        let dot_cycle = (app.marquee_frame / 4) % 4;
        let dots = ".".repeat(dot_cycle as usize);
        spans.push(Span::styled(
            format!(" | 🌐 Translating{}", dots),
            Style::default().fg(Color::Yellow),
        ));
    } else if app.is_streaming {
        // Tool call indicator: when the model is actively calling a tool,
        // show ⚙️ tool_name with animated dots in the status bar.
        if let Some(ref tool_call) = app.current_tool_call {
            let dot_cycle = (app.marquee_frame / 4) % 4;
            let dots = ".".repeat(dot_cycle as usize);
            spans.push(Span::styled(
                format!(" | ⚙️ {} {}", tool_call.name, dots),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ));
        } else if let Some((ref name, _)) = app.streaming_tool_result {
            // Briefly show the completed tool result while tool result is set
            // Show marquee dots to indicate the process is still ongoing.
            let dot_cycle = (app.marquee_frame / 4) % 4;
            let dots = ".".repeat(dot_cycle as usize);
            spans.push(Span::styled(
                format!(" | ✅ {} complete{}", name, dots),
                Style::default().fg(Color::Green),
            ));
        } else if !app.streaming_status.is_empty() {
            // Inter-turn waiting indicator (e.g. "⏳ Waiting for model response...")
            // Show with animated dots to indicate an active wait state.
            let dot_cycle = (app.marquee_frame / 4) % 4;
            let dots = ".".repeat(dot_cycle as usize);
            spans.push(Span::styled(
                format!(" | {} {}", app.streaming_status, dots),
                Style::default().fg(Color::Yellow),
            ));
        } else if app.is_reasoning_active && !app.streaming_text.is_empty() {
            // Show reasoning header as status when text is also streaming
            if let Some(ref header) = app.streaming_reasoning_header {
                spans.push(Span::styled(
                    format!(" | 💭 {}", header),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ));
            } else {
                let dot_cycle = (app.marquee_frame / 4) % 4;
                let dots = ".".repeat(dot_cycle as usize);
                spans.push(Span::styled(
                    format!(" | Streaming{}", dots),
                    Style::default().fg(Color::Yellow),
                ));
            }
        } else if app.is_reasoning_active
            || (!app.streaming_reasoning.is_empty() && app.streaming_text.is_empty())
        {
            // Model is thinking/reasoning — show the extracted header or a fallback
            if let Some(ref header) = app.streaming_reasoning_header {
                spans.push(Span::styled(
                    format!(" | 💭 {}", header),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ));
            } else {
                let dot_cycle = (app.marquee_frame / 4) % 4;
                let dots = ".".repeat(dot_cycle as usize);
                spans.push(Span::styled(
                    format!(" | Thinking{}", dots),
                    Style::default().fg(Color::Yellow),
                ));
            }
        } else {
            let dot_cycle = (app.marquee_frame / 4) % 4;
            let dots = ".".repeat(dot_cycle as usize);
            spans.push(Span::styled(
                format!(" | Streaming{}", dots),
                Style::default().fg(Color::Yellow),
            ));
        }
    } else if app.is_reviewing {
        let dot_cycle = (app.marquee_frame / 4) % 4;
        let dots = ".".repeat(dot_cycle as usize);
        spans.push(Span::styled(
            format!(" | 🔍 Reviewing{}", dots),
            Style::default().fg(Color::Cyan),
        ));
    } else if let Some(ref msg) = app.review_complete_message {
        if app.review_complete_timer > 0 {
            let verdict_color = app
                .review_complete_verdict
                .as_ref()
                .map(|v| match v {
                    crate::core::types::review::ReviewVerdict::Approved => Color::Green,
                    crate::core::types::review::ReviewVerdict::NeedsRevision => Color::Yellow,
                })
                .unwrap_or(Color::Green);
            spans.push(Span::styled(
                format!(" | {}", msg),
                Style::default().fg(verdict_color),
            ));
        } else {
            spans.push(Span::styled(" | Ready", Style::default().fg(Color::Green)));
        }
    } else if app.selection_mode {
        spans.push(Span::styled(
            " | [SELECT]",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ));
    } else if app.shell_mode {
        spans.push(Span::styled(
            " | 🐚 Shell",
            Style::default().fg(Color::Cyan),
        ));
    } else {
        // Idle state — no marquee dots.
        spans.push(Span::styled(
            " | Ready",
            Style::default().fg(Color::Green),
        ));
    }

    let status_bar = Paragraph::new(Line::from(spans));
    f.render_widget(status_bar, area);
}
