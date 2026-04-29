use ratatui::prelude::*;
use ratatui::text::{Line, Span, Text};

pub fn color_to_fg_ansi(color: Color) -> String {
    match color {
        Color::Reset => "\x1b[39m".to_string(),
        Color::Black => "\x1b[30m".to_string(),
        Color::Red => "\x1b[31m".to_string(),
        Color::Green => "\x1b[32m".to_string(),
        Color::Yellow => "\x1b[33m".to_string(),
        Color::Blue => "\x1b[34m".to_string(),
        Color::Magenta => "\x1b[35m".to_string(),
        Color::Cyan => "\x1b[36m".to_string(),
        Color::White => "\x1b[37m".to_string(),
        Color::Rgb(r, g, b) => format!("\x1b[38;2;{};{};{}m", r, g, b),
        Color::Indexed(i) => format!("\x1b[38;5;{}m", i),
        Color::LightRed => "\x1b[91m".to_string(),
        Color::LightGreen => "\x1b[92m".to_string(),
        Color::LightYellow => "\x1b[93m".to_string(),
        Color::LightBlue => "\x1b[94m".to_string(),
        Color::LightMagenta => "\x1b[95m".to_string(),
        Color::LightCyan => "\x1b[96m".to_string(),
        _ => "".to_string(),
    }
}

pub fn modifier_to_ansi(modifier: Modifier) -> String {
    let mut codes = Vec::new();
    if modifier.contains(Modifier::BOLD) {
        codes.push("1");
    }
    if modifier.contains(Modifier::DIM) {
        codes.push("2");
    }
    if modifier.contains(Modifier::ITALIC) {
        codes.push("3");
    }
    if modifier.contains(Modifier::UNDERLINED) {
        codes.push("4");
    }
    if codes.is_empty() {
        "".to_string()
    } else {
        format!("\x1b[{}m", codes.join(";"))
    }
}

pub fn ansi_reset() -> &'static str {
    "\x1b[0m"
}

pub fn style_text(text: &str, fg: Option<Color>, bold: bool, dim: bool) -> String {
    let mut result = String::new();
    if let Some(color) = fg {
        result.push_str(&color_to_fg_ansi(color));
    }
    if bold { result.push_str("\x1b[1m"); }
    if dim { result.push_str("\x1b[2m"); }
    result.push_str(text);
    result.push_str(ansi_reset());
    result
}

/// The ASCII art banner displayed on startup.
pub const BANNER_ART: &str = r#"
  __  __       ____          _      
|  \/  |_   _/ ___|___   __| | ___ 
| |\/| | | | | |   / _ \ / _` |/ _ \
| |  | | |_| | |__| (_) | (_| |  __/
|_|  |_|\__, |\____\___/ \__,_|\___|
        |___/                     
"#;

/// 返回启动 banner 的 ratatui Text（可直接传给 Paragraph）
pub fn make_startup_text() -> Text<'static> {
    let mut lines: Vec<Line> = Vec::new();

    // ASCII art，用青色显示
    for l in BANNER_ART.lines() {
        lines.push(Line::from(Span::styled(
            l.to_string(),
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        )));
    }

    lines.push(Line::from(""));

    // 标题行
    lines.push(Line::from(Span::styled(
        "My Code Agent",
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
    )));

    // 副标题
    lines.push(Line::from(Span::styled(
        "  Interactive AI Coding Assistant",
        Style::default().fg(Color::LightYellow).add_modifier(Modifier::DIM),
    )));

    lines.push(Line::from(""));

    lines.push(Line::from(Span::styled(
        "Type your message below to start chatting.",
        Style::default().fg(Color::LightYellow).add_modifier(Modifier::DIM),
    )));

    lines.push(Line::from(Span::styled(
        "Commands: /help  /save  /load  /new  /think  /model",
        Style::default().fg(Color::LightYellow).add_modifier(Modifier::DIM),
    )));

    Text::from(lines)
}
