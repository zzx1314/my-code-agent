use ratatui::prelude::*;

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
 _                               _   
  _ __ ___  _   _    ___ ___   __| | ___    __ _  __ _  ___ _ __ | |_ 
 | '_ ` _ \ | | | |  / __/ _ \ / _` |/ _ \  / _` |/ _` |/ _ \ '_ \| __|
 | | | | | | |_| | | (_| (_) | (_| |  __/ | (_| | (_| |  __/ | | | |_ 
 |_| |_| |_|\__, |  \___\___/ \__,_|\___|  \__,_|\__, |\___|_| |_|\__|
            |___/                                |___/ 
"#;

/// Returns the startup banner lines with styling applied (for TUI rendering).
pub fn make_banner_text() -> String {
    format!(
        "{}\n{}\n",
        style_text("My Code Agent", Some(Color::Cyan), true, false),
        style_text("  Interactive AI Coding Assistant", Some(Color::DarkGray), false, true),
    )
}

/// Returns the full startup banner (ASCII + info) for the chat area.
pub fn make_startup_display() -> String {
    let mut text = String::new();
    text.push_str(BANNER_ART);
    text.push_str("\n");
    text.push_str(&make_banner_text());
    text.push_str("\n");
    text.push_str(&style_text("Type your message below to start chatting.", Some(Color::DarkGray), false, true));
    text.push_str("\n");
    text.push_str(&style_text("Commands: /help  /save  /load  /new  /think", Some(Color::DarkGray), false, true));
    text
}
