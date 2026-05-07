use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

// ── Color palette ──────────────────────────────────────────────────────────────
const HEADING_COLORS: [Color; 6] = [
    Color::Cyan,      // H1
    Color::Yellow,     // H2
    Color::Green,      // H3
    Color::Magenta,    // H4
    Color::LightBlue,  // H5
    Color::LightCyan,  // H6
];

const CODE_BG: Color = Color::Rgb(40, 40, 40);
const CODE_FG: Color = Color::Rgb(212, 212, 212);
const CODE_BORDER_FG: Color = Color::Rgb(80, 80, 80);
const INLINE_CODE_BG: Color = Color::Rgb(55, 55, 55);
const INLINE_CODE_FG: Color = Color::Rgb(220, 220, 220);
const BLOCKQUOTE_FG: Color = Color::Rgb(150, 150, 150);
const BLOCKQUOTE_BAR: Color = Color::Rgb(100, 100, 100);
const LINK_FG: Color = Color::Rgb(86, 156, 214);
const HR_FG: Color = Color::Rgb(100, 100, 100);
const BULLET_FG: Color = Color::Rgb(180, 180, 180);

// ── Block-level parsing state ──────────────────────────────────────────────────

/// Represents the kind of block element currently being parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
enum BlockState {
    /// Normal paragraph text
    Paragraph,
    /// Inside a fenced code block (``` ... ```)
    CodeBlock {
        lang: Option<String>,
        lines: Vec<String>,
    },
}

// ── Main renderer ──────────────────────────────────────────────────────────────

/// Render a markdown string into styled `Line`s for ratatui.
///
/// Supports: headings (h1–h6), fenced code blocks, inline code, bold, italic,
/// blockquotes, unordered/ordered lists, horizontal rules, and links.
pub fn render_markdown(text: &str) -> Vec<Line<'static>> {
    if text.is_empty() {
        return vec![];
    }

    let mut result = Vec::new();
    let mut state = BlockState::Paragraph;
    let mut prev_was_heading = false;

    for line in text.split('\n') {
        match &mut state {
            BlockState::CodeBlock { lang: _, lines } => {
                if line.trim_start().starts_with("```") {
                    // Close code block
                    prev_was_heading = false;
                    let code_lines = lines.clone();
                    state = BlockState::Paragraph;
                    result.extend(render_code_block_lines(&code_lines));
                } else {
                    lines.push(line.to_string());
                }
            }
            BlockState::Paragraph => {
                let trimmed = line.trim_start();

                // Fenced code block open
                if trimmed.starts_with("```") {
                    prev_was_heading = false;
                    let lang_part = trimmed[3..].trim();
                    let lang = if lang_part.is_empty() {
                        None
                    } else {
                        Some(lang_part.to_string())
                    };
                    state = BlockState::CodeBlock {
                        lang,
                        lines: Vec::new(),
                    };
                    continue;
                }

                // Heading
                if let Some(level) = heading_level(trimmed) {
                    let content = &trimmed[level..];
                    // Strip trailing # marks
                    let content = content.trim_end_matches('#').trim();
                    result.push(render_heading(level, content));
                    prev_was_heading = true;
                    continue;
                }

                // Horizontal rule (---, ***, ___)
                if is_horizontal_rule(trimmed) {
                    prev_was_heading = false;
                    result.push(render_horizontal_rule());
                    result.push(Line::default());
                    continue;
                }

                // Blockquote
                if trimmed.starts_with('>') {
                    prev_was_heading = false;
                    let quote_content = trimmed.strip_prefix('>').unwrap_or(trimmed);
                    let quote_content = quote_content.strip_prefix(' ').unwrap_or(quote_content);
                    result.push(render_blockquote(quote_content));
                    continue;
                }

                // Unordered list (- or * followed by space)
                if let Some(item) = parse_unordered_list(trimmed) {
                    prev_was_heading = false;
                    result.push(render_unordered_item(item));
                    continue;
                }

                // Ordered list (1. 2. etc.)
                if let Some((num, item)) = parse_ordered_list(trimmed) {
                    prev_was_heading = false;
                    result.push(render_ordered_item(num, item));
                    continue;
                }

                // Empty line = paragraph break
                if trimmed.is_empty() {
                    if prev_was_heading {
                        // Skip blank line right after a heading to avoid double spacing
                        prev_was_heading = false;
                    } else {
                        result.push(Line::default());
                    }
                    continue;
                }

                // Regular paragraph text with inline formatting
                prev_was_heading = false;
                result.push(render_inline(line));
            }
        }
    }

    // If a code block was never closed (streaming case), render it as-is
    if let BlockState::CodeBlock { lines, .. } = &state {
        if !lines.is_empty() {
            result.extend(render_code_block_lines(lines));
        }
    }

    result
}

// ── Block-level renderers ──────────────────────────────────────────────────────

fn heading_level(line: &str) -> Option<usize> {
    let mut count = 0;
    for ch in line.chars() {
        if ch == '#' {
            count += 1;
        } else {
            break;
        }
    }
    if count >= 1 && count <= 6 && line.chars().nth(count) == Some(' ') {
        Some(count)
    } else {
        None
    }
}

fn render_heading(level: usize, content: &str) -> Line<'static> {
    let color = HEADING_COLORS.get(level - 1).copied().unwrap_or(Color::Cyan);
    let style = Style::default().fg(color).add_modifier(Modifier::BOLD);

    let prefix = match level {
        1 => "█ ",
        2 => "▓ ",
        3 => "▒ ",
        _ => "░ ",
    };

    // Parse inline formatting within heading text
    let mut spans = vec![Span::styled(prefix, style)];
    let inline_spans = parse_inline_spans(content, style);
    spans.extend(inline_spans);

    Line::from(spans)
}

fn is_horizontal_rule(line: &str) -> bool {
    let stripped = line.trim();
    (stripped.starts_with("---") && stripped.chars().all(|c| c == '-' || c == ' '))
        || (stripped.starts_with("***") && stripped.chars().all(|c| c == '*' || c == ' '))
        || (stripped.starts_with("___") && stripped.chars().all(|c| c == '_' || c == ' '))
}

fn render_horizontal_rule() -> Line<'static> {
    Line::from(Span::styled("─".repeat(60), Style::default().fg(HR_FG)))
}

fn render_blockquote(text: &str) -> Line<'static> {
    let spans = vec![
        Span::styled("│ ", Style::default().fg(BLOCKQUOTE_BAR)),
        Span::styled(text.to_string(), Style::default().fg(BLOCKQUOTE_FG)),
    ];
    Line::from(spans)
}

fn parse_unordered_list(line: &str) -> Option<&str> {
    if (line.starts_with("- ") || line.starts_with("* ") || line.starts_with("+ "))
        && line.len() > 2
    {
        Some(&line[2..])
    } else {
        None
    }
}

fn render_unordered_item(content: &str) -> Line<'static> {
    let mut spans = vec![
        Span::styled("  • ", Style::default().fg(BULLET_FG)),
    ];
    spans.extend(parse_inline_spans(content, Style::default()));
    Line::from(spans)
}

fn parse_ordered_list(line: &str) -> Option<(u32, &str)> {
    // Match "1. ", "2. ", etc.
    let bytes = line.as_bytes();
    let mut num_end = 0;
    while num_end < bytes.len() && bytes[num_end].is_ascii_digit() {
        num_end += 1;
    }
    if num_end > 0
        && num_end + 2 <= line.len()
        && &line[num_end..num_end + 2] == ". "
    {
        let num: u32 = line[..num_end].parse().ok()?;
        Some((num, &line[num_end + 2..]))
    } else {
        None
    }
}

fn render_ordered_item(num: u32, content: &str) -> Line<'static> {
    let mut spans = vec![
        Span::styled(format!("  {}. ", num), Style::default().fg(BULLET_FG)),
    ];
    spans.extend(parse_inline_spans(content, Style::default()));
    Line::from(spans)
}

fn render_code_block_lines(lines: &[String]) -> Vec<Line<'static>> {
    let mut result = Vec::new();

    // Top border
    result.push(Line::from(Span::styled(
        "┌───",
        Style::default().fg(CODE_BORDER_FG),
    )));

    // Code content
    for line in lines {
        let content = format!(" │ {}", line);
        result.push(Line::from(Span::styled(
            content,
            Style::default().fg(CODE_FG).bg(CODE_BG),
        )));
    }

    // Bottom border
    result.push(Line::from(Span::styled(
        "└───",
        Style::default().fg(CODE_BORDER_FG),
    )));

    result
}

// ── Inline formatting ──────────────────────────────────────────────────────────

/// Render a line of text with inline markdown formatting.
fn render_inline(text: &str) -> Line<'static> {
    Line::from(parse_inline_spans(text, Style::default()))
}

/// Parse inline markdown elements into styled `Span`s.
///
/// Supports: **bold**, *italic*, `inline code`, ~~strikethrough~~, [link](url)
fn parse_inline_spans(text: &str, base_style: Style) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut i = 0;
    let mut current = String::new();

    while i < len {
        // Inline code: `...`
        if chars[i] == '`' && !is_escaped(&chars, i) {
            if !current.is_empty() {
                spans.push(Span::styled(
                    std::mem::take(&mut current),
                    base_style,
                ));
            }
            if let Some((end, code)) = find_closing(&chars, i + 1, '`', true) {
                spans.push(Span::styled(
                    format!(" {} ", code),
                    base_style.fg(INLINE_CODE_FG).bg(INLINE_CODE_BG),
                ));
                i = end + 1;
                continue;
            }
        }

        // Bold: **...** (must check before italic since * overlaps)
        if i + 1 < len && chars[i] == '*' && chars[i + 1] == '*' {
            if !current.is_empty() {
                spans.push(Span::styled(
                    std::mem::take(&mut current),
                    base_style,
                ));
            }
            if let Some((end, content)) = find_closing(&chars, i + 2, '*', false) {
                // Find closing **
                if end + 1 < len && chars[end] == '*' && chars[end + 1] == '*' {
                    let bold_style = base_style.add_modifier(Modifier::BOLD);
                    spans.extend(parse_inline_spans_inner(&content, bold_style));
                    i = end + 2;
                    continue;
                }
            }
            // Not a valid bold, treat as literal
            current.push(chars[i]);
            i += 1;
            continue;
        }

        // Strikethrough: ~~...~~
        if i + 1 < len && chars[i] == '~' && chars[i + 1] == '~' {
            if !current.is_empty() {
                spans.push(Span::styled(
                    std::mem::take(&mut current),
                    base_style,
                ));
            }
            if let Some((end, content)) = find_closing_double(&chars, i + 2, '~') {
                let strike_style = base_style.add_modifier(Modifier::CROSSED_OUT);
                spans.push(Span::styled(content, strike_style));
                i = end + 2;
                continue;
            }
            current.push(chars[i]);
            i += 1;
            continue;
        }

        // Italic: *...* or _..._
        if (chars[i] == '*' || chars[i] == '_') && !is_escaped(&chars, i) {
            // Make sure it's not ** (already handled) and not part of ___
            if i > 0 && chars[i] == '_' && chars[i - 1] == '_' {
                current.push(chars[i]);
                i += 1;
                continue;
            }
            if !current.is_empty() {
                spans.push(Span::styled(
                    std::mem::take(&mut current),
                    base_style,
                ));
            }
            if let Some((end, content)) = find_closing(&chars, i + 1, chars[i], true) {
                let italic_style = base_style.add_modifier(Modifier::ITALIC);
                spans.extend(parse_inline_spans_inner(&content, italic_style));
                i = end + 1;
                continue;
            }
        }

        // Link: [text](url)
        if chars[i] == '[' {
            if !current.is_empty() {
                spans.push(Span::styled(
                    std::mem::take(&mut current),
                    base_style,
                ));
            }
            // Find closing ] then (url)
            if let Some(bracket_end) = find_char(&chars, i + 1, ']') {
                if bracket_end + 1 < len && chars[bracket_end + 1] == '(' {
                    if let Some(paren_end) = find_char(&chars, bracket_end + 2, ')') {
                        let link_text: String = chars[i + 1..bracket_end].iter().collect();
                        spans.push(Span::styled(
                            link_text,
                            base_style.fg(LINK_FG).add_modifier(Modifier::UNDERLINED),
                        ));
                        i = paren_end + 1;
                        continue;
                    }
                }
            }
        }

        current.push(chars[i]);
        i += 1;
    }

    if !current.is_empty() {
        spans.push(Span::styled(current, base_style));
    }

    spans
}

/// Inner variant that skips certain checks to avoid infinite recursion.
fn parse_inline_spans_inner(text: &str, base_style: Style) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut i = 0;
    let mut current = String::new();

    while i < len {
        // Inline code: `...`
        if chars[i] == '`' {
            if !current.is_empty() {
                spans.push(Span::styled(
                    std::mem::take(&mut current),
                    base_style,
                ));
            }
            if let Some((end, code)) = find_closing(&chars, i + 1, '`', true) {
                spans.push(Span::styled(
                    format!(" {} ", code),
                    base_style.fg(INLINE_CODE_FG).bg(INLINE_CODE_BG),
                ));
                i = end + 1;
                continue;
            }
        }

        // Link inside bold/italic
        if chars[i] == '[' {
            if !current.is_empty() {
                spans.push(Span::styled(
                    std::mem::take(&mut current),
                    base_style,
                ));
            }
            if let Some(bracket_end) = find_char(&chars, i + 1, ']') {
                if bracket_end + 1 < len && chars[bracket_end + 1] == '(' {
                    if let Some(paren_end) = find_char(&chars, bracket_end + 2, ')') {
                        let link_text: String = chars[i + 1..bracket_end].iter().collect();
                        spans.push(Span::styled(
                            link_text,
                            base_style.fg(LINK_FG).add_modifier(Modifier::UNDERLINED),
                        ));
                        i = paren_end + 1;
                        continue;
                    }
                }
            }
        }

        current.push(chars[i]);
        i += 1;
    }

    if !current.is_empty() {
        spans.push(Span::styled(current, base_style));
    }

    spans
}

// ── Inline helpers ─────────────────────────────────────────────────────────────

/// Find closing delimiter and return (position_after_match, content_between).
/// For backtick inline code (`code`), no nested parsing needed.
fn find_closing(chars: &[char], start: usize, delim: char, allow_newline: bool) -> Option<(usize, String)> {
    let mut content = String::new();
    let mut i = start;
    while i < chars.len() {
        if chars[i] == delim {
            return Some((i, content));
        }
        if !allow_newline && chars[i] == '\n' {
            return None; // Bold/italic don't span lines
        }
        content.push(chars[i]);
        i += 1;
    }
    None
}

/// Find closing ~~ delimiter.
fn find_closing_double(chars: &[char], start: usize, delim: char) -> Option<(usize, String)> {
    let mut content = String::new();
    let mut i = start;
    while i + 1 < chars.len() {
        if chars[i] == delim && chars[i + 1] == delim {
            return Some((i, content));
        }
        if chars[i] == '\n' {
            return None;
        }
        content.push(chars[i]);
        i += 1;
    }
    None
}

/// Find the position of a specific character.
fn find_char(chars: &[char], start: usize, target: char) -> Option<usize> {
    for i in start..chars.len() {
        if chars[i] == target {
            return Some(i);
        }
    }
    None
}

/// Check if a character is escaped with a backslash.
fn is_escaped(chars: &[char], pos: usize) -> bool {
    pos > 0 && chars[pos - 1] == '\\'
}

// ── Public API for streaming ───────────────────────────────────────────────────

/// Render streaming markdown text. Handles unclosed code blocks natively
/// (no temporary fence hack needed).
pub fn render_streaming_markdown(text: &str) -> Vec<Line<'static>> {
    render_markdown(text)
}

/// Render complete (non-streaming) markdown text.
pub fn render_full_markdown(text: &str) -> Vec<Line<'static>> {
    render_markdown(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_heading() {
        let result = render_markdown("# Hello World");
        assert!(!result.is_empty());
        // First line should have heading content
        let line_str = format!("{:?}", result[0]);
        assert!(line_str.contains("Hello World"));
    }

    #[test]
    fn test_code_block() {
        let text = "```rust\nfn main() {\n    println!(\"hi\");\n}\n```";
        let result = render_markdown(text);
        // Should have: top border, 4 code lines, bottom border = 6
        assert!(result.len() >= 4);
    }

    #[test]
    fn test_unclosed_code_block() {
        let text = "```rust\nfn main() {\n    println!(\"hi\");";
        let result = render_markdown(text);
        assert!(!result.is_empty());
    }

    #[test]
    fn test_bold() {
        let result = render_markdown("This is **bold** text");
        assert!(!result.is_empty());
    }

    #[test]
    fn test_inline_code() {
        let result = render_markdown("Use `println!` for output");
        assert!(!result.is_empty());
    }

    #[test]
    fn test_horizontal_rule() {
        let result = render_markdown("---");
        assert!(!result.is_empty());
    }

    #[test]
    fn test_blockquote() {
        let result = render_markdown("> This is a quote");
        assert!(!result.is_empty());
    }

    #[test]
    fn test_unordered_list() {
        let result = render_markdown("- Item 1\n- Item 2");
        assert!(result.len() >= 2);
    }

    #[test]
    fn test_ordered_list() {
        let result = render_markdown("1. First\n2. Second");
        assert!(result.len() >= 2);
    }

    #[test]
    fn test_empty() {
        let result = render_markdown("");
        assert!(result.is_empty());
    }

    #[test]
    fn test_link() {
        let result = render_markdown("[Rust](https://rust-lang.org)");
        assert!(!result.is_empty());
    }

    #[test]
    fn test_mixed() {
        let text = "# Title\n\nSome **bold** and `code` text\n\n```rust\nfn main() {}\n```\n\n- List item\n> Quote\n\n---\n";
        let result = render_markdown(text);
        assert!(result.len() > 10);
    }
}
