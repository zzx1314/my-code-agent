use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};

use crate::app::App;

/// Render the command/file completion menu below or above the input area.
pub fn render_completion_menu(f: &mut Frame, app: &mut App, input_area: Rect) {
    if app.completion_items.is_empty() {
        return;
    }

    let max_visible_items = 10;
    let menu_height = (app.completion_items.len().min(max_visible_items) as u16) + 2;
    let menu_width = 50u16.min(input_area.width);

    let menu_y = if input_area.y >= menu_height {
        input_area.y - menu_height
    } else {
        input_area.y + input_area.height
    };

    let menu_rect = Rect {
        x: input_area.x,
        y: menu_y,
        width: menu_width,
        height: menu_height,
    }
    .intersection(f.area());

    f.render_widget(Clear, menu_rect);

    let items: Vec<ListItem> = app
        .completion_items
        .iter()
        .map(|item| {
            let style = if item == &app.completion_items[app.completion_selected] {
                Style::default()
                    .bg(Color::Blue)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            ListItem::new(item.as_str()).style(style)
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(app.completion_selected));

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(match app.completion_type {
                    Some('/') => " Commands ",
                    Some('@') => " Files ",
                    _ => " Completions ",
                })
                .border_style(Style::default().fg(Color::Cyan))
                .style(Style::default().bg(Color::Black)),
        )
        .highlight_style(
            Style::default()
                .bg(Color::Blue)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    f.render_stateful_widget(list, menu_rect, &mut state);
}

/// Render a confirmation dialog overlay for dangerous operations.
pub fn render_confirmation_dialog(f: &mut Frame, app: &mut App) {
    let Some(pending) = &app.pending_confirmation else {
        return;
    };

    let area = f.area();

    // Dialog dimensions
    let dialog_width = 60.min(area.width.saturating_sub(4));
    let detail_lines = pending.detail.lines().count() as u16;
    let dialog_height = (detail_lines + 6).min(area.height.saturating_sub(4));

    let dialog_x = (area.width.saturating_sub(dialog_width)) / 2;
    let dialog_y = (area.height.saturating_sub(dialog_height)) / 2;

    let dialog_rect = Rect {
        x: dialog_x,
        y: dialog_y,
        width: dialog_width,
        height: dialog_height,
    };

    // Dim the background
    let overlay = Rect {
        x: 0,
        y: 0,
        width: area.width,
        height: area.height,
    };
    f.render_widget(Clear, overlay);

    // Dialog block
    let block = Block::default()
        .title(" ⚠️  Confirmation Required ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow))
        .style(Style::default().bg(Color::Black).fg(Color::White));

    let inner = block.inner(dialog_rect);
    f.render_widget(block, dialog_rect);

    // Render the reason (bold, yellow)
    let reason_text = format!("\n{}\n", &pending.reason);
    let reason_paragraph = Paragraph::new(reason_text).style(
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    );
    f.render_widget(
        reason_paragraph,
        Rect {
            x: inner.x,
            y: inner.y,
            width: inner.width,
            height: 2,
        },
    );

    // Render the detail
    let detail_paragraph = Paragraph::new(pending.detail.as_str())
        .wrap(Wrap { trim: false })
        .style(Style::default().fg(Color::White));
    f.render_widget(
        detail_paragraph,
        Rect {
            x: inner.x,
            y: inner.y + 2,
            width: inner.width,
            height: inner.height.saturating_sub(5),
        },
    );

    // Render prompt at the bottom
    let prompt_text = "  [Y] Yes   [N] No   [Esc] Cancel";
    let prompt = Paragraph::new(prompt_text).style(Style::default().fg(Color::Cyan));

    f.render_widget(
        prompt,
        Rect {
            x: inner.x,
            y: inner.y + inner.height.saturating_sub(2),
            width: inner.width,
            height: 1,
        },
    );
}

/// Render the model picker overlay.
pub fn render_model_picker(f: &mut Frame, app: &mut App, input_area: Rect) {
    if app.model_options.is_empty() {
        return;
    }

    let max_visible_items = 10;
    let menu_height = (app.model_options.len().min(max_visible_items) as u16) + 2;
    let menu_width = 60u16.min(input_area.width);

    let menu_y = if input_area.y >= menu_height {
        input_area.y - menu_height
    } else {
        input_area.y + input_area.height
    };

    let menu_rect = Rect {
        x: input_area.x,
        y: menu_y,
        width: menu_width,
        height: menu_height,
    }
    .intersection(f.area());

    f.render_widget(Clear, menu_rect);

    let items: Vec<ListItem> = app
        .model_options
        .iter()
        .enumerate()
        .map(|(idx, model)| {
            let prefix = if idx == app.model_selected {
                "▶ "
            } else {
                "  "
            };
            let style = if idx == app.model_selected {
                Style::default()
                    .bg(Color::Blue)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Cyan)
            };
            ListItem::new(format!("{}{}", prefix, model)).style(style)
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(app.model_selected));

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Select Model (↑↓ Enter Esc) ")
                .border_style(Style::default().fg(Color::Yellow))
                .style(Style::default().bg(Color::Black)),
        )
        .highlight_style(
            Style::default()
                .bg(Color::Blue)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        );

    f.render_stateful_widget(list, menu_rect, &mut state);
}

/// Render the provider picker overlay.
pub fn render_provider_picker(f: &mut Frame, app: &mut App, input_area: Rect) {
    if app.provider_options.is_empty() {
        return;
    }

    let menu_height = (app.provider_options.len() as u16) + 2;
    let menu_width = 30u16.min(input_area.width);

    let menu_y = if input_area.y >= menu_height {
        input_area.y - menu_height
    } else {
        input_area.y + input_area.height
    };

    let menu_rect = Rect {
        x: input_area.x,
        y: menu_y,
        width: menu_width,
        height: menu_height,
    }
    .intersection(f.area());

    f.render_widget(Clear, menu_rect);

    let items: Vec<ListItem> = app
        .provider_options
        .iter()
        .enumerate()
        .map(|(idx, provider)| {
            let prefix = if idx == app.provider_selected {
                "▶ "
            } else {
                "  "
            };
            let style = if idx == app.provider_selected {
                Style::default()
                    .bg(Color::Blue)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Cyan)
            };
            ListItem::new(format!("{}{}", prefix, provider)).style(style)
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(app.provider_selected));

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Select Provider (↑↓ Enter Esc) ")
                .border_style(Style::default().fg(Color::Green))
                .style(Style::default().bg(Color::Black)),
        )
        .highlight_style(
            Style::default()
                .bg(Color::Blue)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        );

    f.render_stateful_widget(list, menu_rect, &mut state);
}

/// Render the session picker overlay.
pub fn render_session_picker(f: &mut Frame, app: &mut App, input_area: Rect) {
    if app.session_options.is_empty() {
        return;
    }

    let menu_height = (app.session_options.len() as u16) + 2;
    let menu_width = 50u16.min(input_area.width);

    let menu_y = if input_area.y >= menu_height {
        input_area.y - menu_height
    } else {
        input_area.y + input_area.height
    };

    let menu_rect = Rect {
        x: input_area.x,
        y: menu_y,
        width: menu_width,
        height: menu_height,
    }
    .intersection(f.area());

    f.render_widget(Clear, menu_rect);

    let items: Vec<ListItem> = app
        .session_options
        .iter()
        .enumerate()
        .map(|(idx, session)| {
            let prefix = if idx == app.session_selected {
                "▶ "
            } else {
                "  "
            };
            let style = if idx == app.session_selected {
                Style::default()
                    .bg(Color::Blue)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Cyan)
            };
            let display_text = format!(
                "{}{} ({} turns, {} tokens)",
                prefix, session.name, session.turns, session.tokens
            );
            ListItem::new(display_text).style(style)
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(app.session_selected));

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Select Session (↑↓ Enter Esc) ")
                .border_style(Style::default().fg(Color::Yellow))
                .style(Style::default().bg(Color::Black)),
        )
        .highlight_style(
            Style::default()
                .bg(Color::Blue)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        );

    f.render_stateful_widget(list, menu_rect, &mut state);
}
