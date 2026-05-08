use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

// ── Color palette ──────────────────────────────────────────────────────────────
const HEADING_COLORS: [Color; 6] = [
    Color::Cyan,      // H1
    Color::Yellow,    // H2
    Color::Green,     // H3
    Color::Magenta,   // H4
    Color::LightBlue, // H5
    Color::LightCyan, // H6
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
const TABLE_BORDER_FG: Color = Color::Rgb(100, 100, 100);
const TABLE_HEADER_FG: Color = Color::Rgb(180, 220, 255);
const TABLE_HEADER_BG: Color = Color::Rgb(40, 50, 65);
const TABLE_ALT_ROW_BG: Color = Color::Rgb(30, 30, 38);

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
    /// Inside a markdown table
    Table {
        header_cells: Vec<String>,
        alignments: Vec<TableAlignment>,
        rows: Vec<Vec<String>>,
    },
}

/// Column alignment for table rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TableAlignment {
    Left,
    Center,
    Right,
}

// ── Main renderer ──────────────────────────────────────────────────────────────

/// Render a markdown string into styled `Line`s for ratatui.
///
/// Supports: headings (h1–h6), fenced code blocks, inline code, bold, italic,
/// blockquotes, unordered/ordered lists, horizontal rules, links, and tables.
pub fn render_markdown(text: &str) -> Vec<Line<'static>> {
    if text.is_empty() {
        return vec![];
    }

    let lines: Vec<&str> = text.split('\n').collect();
    let mut result = Vec::new();
    let mut state = BlockState::Paragraph;
    let mut prev_was_heading = false;
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];
        let mut reprocess = false;

        match &mut state {
            BlockState::CodeBlock { lang, lines: code_lines } => {
                if line.trim_start().starts_with("```") {
                    // Close code block
                    prev_was_heading = false;
                    let code_lines = code_lines.clone();
                    let code_lang = lang.clone();
                    state = BlockState::Paragraph;
                    result.extend(render_code_block_lines(&code_lines, code_lang.as_deref()));
                } else {
                    code_lines.push(line.to_string());
                }
            }
            BlockState::Table { header_cells, alignments, rows } => {
                let trimmed = line.trim();
                if !trimmed.is_empty() && is_table_row(trimmed) {
                    let cells = parse_table_cells(trimmed);
                    rows.push(cells);
                } else {
                    // End of table — render it and re-process this line
                    let hdr = header_cells.clone();
                    let aligns = alignments.clone();
                    let table_rows = rows.clone();
                    state = BlockState::Paragraph;
                    result.extend(render_table(&hdr, &aligns, &table_rows));
                    if !trimmed.is_empty() {
                        result.push(Line::default());
                    }
                    reprocess = true;
                    prev_was_heading = false;
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
                    i += 1;
                    continue;
                }

                // Heading
                if let Some(level) = heading_level(trimmed) {
                    let content = &trimmed[level..];
                    // Strip trailing # marks
                    let content = content.trim_end_matches('#').trim();
                    result.push(render_heading(level, content));
                    prev_was_heading = true;
                    i += 1;
                    continue;
                }

                // Horizontal rule (---, ***, ___)
                if is_horizontal_rule(trimmed) {
                    prev_was_heading = false;
                    result.push(render_horizontal_rule());
                    result.push(Line::default());
                    i += 1;
                    continue;
                }

                // Table: header row followed by separator enters table mode
                if is_table_header_candidate(trimmed)
                    && i + 1 < lines.len()
                    && is_table_separator(lines[i + 1].trim())
                {
                    let header = parse_table_cells(trimmed);
                    let aligns = parse_column_alignments(lines[i + 1].trim());
                    state = BlockState::Table {
                        header_cells: header,
                        alignments: aligns,
                        rows: Vec::new(),
                    };
                    prev_was_heading = false;
                    i += 2; // Skip header and separator
                    continue;
                }

                // Blockquote
                if trimmed.starts_with('>') {
                    prev_was_heading = false;
                    let quote_content = trimmed.strip_prefix('>').unwrap_or(trimmed);
                    let quote_content = quote_content.strip_prefix(' ').unwrap_or(quote_content);
                    result.push(render_blockquote(quote_content));
                    i += 1;
                    continue;
                }

                // Unordered list (- or * followed by space)
                if let Some(item) = parse_unordered_list(trimmed) {
                    prev_was_heading = false;
                    result.push(render_unordered_item(item));
                    i += 1;
                    continue;
                }

                // Ordered list (1. 2. etc.)
                if let Some((num, item)) = parse_ordered_list(trimmed) {
                    prev_was_heading = false;
                    result.push(render_ordered_item(num, item));
                    i += 1;
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
                    i += 1;
                    continue;
                }

                // Regular paragraph text with inline formatting
                prev_was_heading = false;
                result.push(render_inline(line));
            }
        }

        if !reprocess {
            i += 1;
        }
    }

    // If a code block was never closed (streaming case), render it as-is
    if let BlockState::CodeBlock { lang, lines: code_lines } = &state {
        if !code_lines.is_empty() {
            result.extend(render_code_block_lines(code_lines, lang.as_deref()));
        }
    }

    // If a table was never closed (streaming case), render it as-is
    if let BlockState::Table { header_cells, alignments, rows } = &state {
        result.extend(render_table(header_cells, alignments, rows));
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
    let color = HEADING_COLORS
        .get(level - 1)
        .copied()
        .unwrap_or(Color::Cyan);
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
    let mut spans = vec![Span::styled("  • ", Style::default().fg(BULLET_FG))];
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
    // Use get() to safely slice, avoiding invalid byte boundary panics
    if num_end > 0 && line.get(num_end..num_end + 2).is_some_and(|s| s == ". ") {
        let num: u32 = line[..num_end].parse().ok()?;
        let rest = line.get(num_end + 2..)?;
        Some((num, rest))
    } else {
        None
    }
}

fn render_ordered_item(num: u32, content: &str) -> Line<'static> {
    let mut spans = vec![Span::styled(
        format!("  {}. ", num),
        Style::default().fg(BULLET_FG),
    )];
    spans.extend(parse_inline_spans(content, Style::default()));
    Line::from(spans)
}

fn render_code_block_lines(lines: &[String], lang: Option<&str>) -> Vec<Line<'static>> {
    let mut result = Vec::new();

    // Top border with optional language label
    let mut spans = vec![Span::styled("┌─── ", Style::default().fg(CODE_BORDER_FG))];
    if let Some(lang) = lang {
        if !lang.is_empty() {
            spans.push(Span::styled(
                lang.to_string(),
                Style::default().fg(Color::Cyan),
            ));
        }
    }
    result.push(Line::from(spans));

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

// ── Table parsing and rendering ────────────────────────────────────────────────

/// Check if a line looks like a table header candidate (contains | and has non-whitespace content).
fn is_table_header_candidate(line: &str) -> bool {
    // Must start with | (or trimmed start) and contain at least one |
    let trimmed = line.trim();
    trimmed.starts_with('|') && trimmed.contains('|') && trimmed.len() > 1
}

/// Check if a line is a table separator (e.g., |---|---| or |:---:|:---:|).
fn is_table_separator(line: &str) -> bool {
    let trimmed = line.trim();
    if !trimmed.starts_with('|') {
        return false;
    }
    // Remove leading/trailing |
    let inner = trimmed.trim_start_matches('|').trim_end_matches('|');
    if inner.is_empty() {
        return false;
    }
    // Each cell in separator must be only dashes, colons, and spaces
    inner.split('|').all(|cell| {
        let cell = cell.trim();
        !cell.is_empty() && cell.chars().all(|c| c == '-' || c == ':' || c == ' ')
    })
}

/// Check if a line is a table data row (contains |).
fn is_table_row(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with('|') && trimmed.contains('|') && trimmed.len() > 1
}

/// Parse cells from a table row like `| a | b | c |` → vec!["a", "b", "c"].
fn parse_table_cells(line: &str) -> Vec<String> {
    let trimmed = line.trim();
    // Remove leading and trailing |
    let inner = trimmed.trim_start_matches('|').trim_end_matches('|');
    inner
        .split('|')
        .map(|cell| cell.trim().to_string())
        .collect()
}

/// Parse column alignments from a separator line like `|:---|:---:|---:|`.
fn parse_column_alignments(line: &str) -> Vec<TableAlignment> {
    let trimmed = line.trim();
    let inner = trimmed.trim_start_matches('|').trim_end_matches('|');
    inner
        .split('|')
        .map(|cell| {
            let cell = cell.trim();
            let left = cell.starts_with(':');
            let right = cell.ends_with(':');
            match (left, right) {
                (true, true) => TableAlignment::Center,
                (false, true) => TableAlignment::Right,
                _ => TableAlignment::Left,
            }
        })
        .collect()
}

/// Compute display width of a string (approximation: 1 per char, 2 for wide chars).
fn display_width(s: &str) -> usize {
    s.chars()
        .map(|c| unicode_width::UnicodeWidthChar::width(c).unwrap_or(1))
        .sum()
}

/// Pad a string to the target display width according to alignment.
fn align_cell(content: &str, width: usize, align: TableAlignment) -> String {
    let content_width = display_width(content);
    if content_width >= width {
        return content.to_string();
    }
    let padding = width - content_width;
    match align {
        TableAlignment::Left => format!("{}{}", content, " ".repeat(padding)),
        TableAlignment::Right => format!("{}{}", " ".repeat(padding), content),
        TableAlignment::Center => {
            let left_pad = padding / 2;
            let right_pad = padding - left_pad;
            format!("{}{}{}", " ".repeat(left_pad), content, " ".repeat(right_pad))
        }
    }
}

/// Render a complete table as styled `Line`s.
fn render_table(
    header: &[String],
    alignments: &[TableAlignment],
    rows: &[Vec<String>],
) -> Vec<Line<'static>> {
    let mut result = Vec::new();

    // Determine column count and compute max widths
    let col_count = header.len();
    if col_count == 0 {
        return result;
    }

    let mut col_widths = vec![0usize; col_count];
    for (i, cell) in header.iter().enumerate() {
        col_widths[i % col_count] = col_widths[i % col_count].max(display_width(cell));
    }
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            col_widths[i % col_count] = col_widths[i % col_count].max(display_width(cell));
        }
    }
    // Ensure minimum width of 3 for separator dashes
    for w in &mut col_widths {
        if *w < 3 {
            *w = 3;
        }
    }

    let border_style = Style::default().fg(TABLE_BORDER_FG);
    let border_char = '│';

    // ┌───┬───┬───┐  (top border)
    let mut top = String::new();
    top.push('┌');
    for (i, w) in col_widths.iter().enumerate() {
        top.push_str(&"─".repeat(w + 2));
        if i + 1 < col_count {
            top.push('┬');
        }
    }
    top.push('┐');
    result.push(Line::from(Span::styled(top, border_style)));

    // Header row: │ content │ content │
    {
        let mut spans = Vec::new();
        spans.push(Span::styled(format!("{border_char} "), border_style));
        for i in 0..col_count {
            let cell = header.get(i).map(|s| s.as_str()).unwrap_or("");
            let align = alignments.get(i).copied().unwrap_or(TableAlignment::Left);
            let padded = align_cell(cell, col_widths[i], align);
            spans.push(Span::styled(
                padded,
                Style::default()
                    .fg(TABLE_HEADER_FG)
                    .bg(TABLE_HEADER_BG)
                    .add_modifier(Modifier::BOLD),
            ));
            if i + 1 < col_count {
                spans.push(Span::styled(
                    format!(" {border_char} "),
                    border_style.bg(TABLE_HEADER_BG),
                ));
            } else {
                spans.push(Span::styled(
                    format!(" {border_char}"),
                    border_style.bg(TABLE_HEADER_BG),
                ));
            }
        }
        result.push(Line::from(spans));
    }

    // ├───┼───┼───┤  (header separator)
    let mut sep = String::new();
    sep.push('├');
    for (i, w) in col_widths.iter().enumerate() {
        sep.push_str(&"─".repeat(w + 2));
        if i + 1 < col_count {
            sep.push('┼');
        }
    }
    sep.push('┤');
    result.push(Line::from(Span::styled(sep, border_style)));

    // Data rows
    for (row_idx, row) in rows.iter().enumerate() {
        let mut spans = Vec::new();
        let row_bg = if row_idx % 2 == 1 {
            Some(TABLE_ALT_ROW_BG)
        } else {
            None
        };

        spans.push(Span::styled(format!("{border_char} "), border_style));
        for i in 0..col_count {
            let cell = row.get(i).map(|s| s.as_str()).unwrap_or("");
            let align = alignments.get(i).copied().unwrap_or(TableAlignment::Left);
            let padded = align_cell(cell, col_widths[i], align);
            let mut cell_style = Style::default();
            if let Some(bg) = row_bg {
                cell_style = cell_style.bg(bg);
            }
            spans.push(Span::styled(padded, cell_style));
            let mut border_style_cell = border_style;
            if let Some(bg) = row_bg {
                border_style_cell = border_style_cell.bg(bg);
            }
            if i + 1 < col_count {
                spans.push(Span::styled(
                    format!(" {border_char} "),
                    border_style_cell,
                ));
            } else {
                spans.push(Span::styled(
                    format!(" {border_char}"),
                    border_style_cell,
                ));
            }
        }
        result.push(Line::from(spans));
    }

    // └───┴───┴───┘  (bottom border)
    let mut bottom = String::new();
    bottom.push('└');
    for (i, w) in col_widths.iter().enumerate() {
        bottom.push_str(&"─".repeat(w + 2));
        if i + 1 < col_count {
            bottom.push('┴');
        }
    }
    bottom.push('┘');
    result.push(Line::from(Span::styled(bottom, border_style)));

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
                spans.push(Span::styled(std::mem::take(&mut current), base_style));
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
                spans.push(Span::styled(std::mem::take(&mut current), base_style));
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
                spans.push(Span::styled(std::mem::take(&mut current), base_style));
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
                spans.push(Span::styled(std::mem::take(&mut current), base_style));
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
                spans.push(Span::styled(std::mem::take(&mut current), base_style));
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
                spans.push(Span::styled(std::mem::take(&mut current), base_style));
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
                spans.push(Span::styled(std::mem::take(&mut current), base_style));
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
fn find_closing(
    chars: &[char],
    start: usize,
    delim: char,
    allow_newline: bool,
) -> Option<(usize, String)> {
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
