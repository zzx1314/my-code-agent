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
