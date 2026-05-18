use my_code_agent::ui::markdown::render_markdown;

#[test]
fn test_heading() {
    let result = render_markdown("# Hello World", None);
    assert!(!result.is_empty());
    // First line should have heading content
    let line_str = format!("{:?}", result[0]);
    assert!(line_str.contains("Hello World"));
}

#[test]
fn test_code_block() {
    let text = "```rust\nfn main() {\n    println!(\"hi\");\n}\n```";
    let result = render_markdown(text, None);
    // Should have: top border, 4 code lines, bottom border = 6
    assert!(result.len() >= 4);
    // Top border should contain the language label
    let top_border = format!("{:?}", result[0]);
    assert!(
        top_border.contains("rust"),
        "top border should contain language label, got: {}",
        top_border
    );
}

#[test]
fn test_unclosed_code_block() {
    let text = "```rust\nfn main() {\n    println!(\"hi\");";
    let result = render_markdown(text, None);
    assert!(!result.is_empty());
}

#[test]
fn test_bold() {
    let result = render_markdown("This is **bold** text", None);
    assert!(!result.is_empty());
}

#[test]
fn test_inline_code() {
    let result = render_markdown("Use `println!` for output", None);
    assert!(!result.is_empty());
}

#[test]
fn test_horizontal_rule() {
    let result = render_markdown("---", None);
    assert!(!result.is_empty());
}

#[test]
fn test_blockquote() {
    let result = render_markdown("> This is a quote", None);
    assert!(!result.is_empty());
}

#[test]
fn test_unordered_list() {
    let result = render_markdown("- Item 1\n- Item 2", None);
    assert!(result.len() >= 2);
}

#[test]
fn test_ordered_list() {
    let result = render_markdown("1. First\n2. Second", None);
    assert!(result.len() >= 2);
}

#[test]
fn test_empty() {
    let result = render_markdown("", None);
    assert!(result.is_empty());
}

#[test]
fn test_link() {
    let result = render_markdown("[Rust](https://rust-lang.org)", None);
    assert!(!result.is_empty());
}

#[test]
fn test_table_basic() {
    let text = "| Name  | Age | City    |\n|-------|-----|---------|\n| Alice | 30  | Beijing |\n| Bob   | 25  | Shanghai|";
    let result = render_markdown(text, None);
    // Should have: top border + header + separator + 2 data rows + bottom border = 6 lines
    assert_eq!(result.len(), 6, "basic table should have 6 lines, got {}", result.len());
    // Top border should contain box drawing chars
    let top_str = format!("{:?}", result[0]);
    assert!(top_str.contains('┌'), "top border should have ┌, got: {}", top_str);
    assert!(top_str.contains('┐'), "top border should have ┐, got: {}", top_str);
    // Bottom border
    let bottom_str = format!("{:?}", result[5]);
    assert!(bottom_str.contains('└'), "bottom border should have └, got: {}", bottom_str);
    assert!(bottom_str.contains('┘'), "bottom border should have ┘, got: {}", bottom_str);
}

#[test]
fn test_table_with_alignment() {
    let text = "| Left | Center | Right |\n|:-----|:------:|------:|\n| a    | b      | c     |";
    let result = render_markdown(text, None);
    // top + header + separator + 1 data row + bottom = 5 lines
    assert_eq!(result.len(), 5);
    // Header line should contain "Left", "Center", "Right"
    let header_str = format!("{:?}", result[1]);
    assert!(header_str.contains("Left"), "header should contain 'Left'");
    assert!(header_str.contains("Center"), "header should contain 'Center'");
    assert!(header_str.contains("Right"), "header should contain 'Right'");
}

#[test]
fn test_table_empty_cells() {
    let text = "| A | B |\n|---|---|\n|   | x |";
    let result = render_markdown(text, None);
    assert_eq!(result.len(), 5, "table with empty cells should have 5 lines");
}

#[test]
fn test_table_single_column() {
    let text = "| Only |\n|------|\n| one  |\n| two  |";
    let result = render_markdown(text, None);
    assert_eq!(result.len(), 6, "single column table should have 6 lines");
}

#[test]
fn test_table_inline_formatting_in_cells() {
    let text = "| **Bold** | *Italic* |\n|----------|----------|\n| text     | text     |";
    let result = render_markdown(text, None);
    assert_eq!(result.len(), 5);
    // Header row (index 1) should have bold styling
    let header_spans = &result[1];
    let has_bold = header_spans.spans.iter().any(|s| {
        s.style.add_modifier.contains(ratatui::style::Modifier::BOLD)
    });
    assert!(has_bold, "header row should have bold styling");
}

#[test]
fn test_table_followed_by_paragraph() {
    let text = "| A |\n|---|\n| x |\n\nSome text after table";
    let result = render_markdown(text, None);
    // table (5) + blank + paragraph = 7
    assert_eq!(result.len(), 7, "table then blank then paragraph");
    let last_line = format!("{:?}", result[6]);
    assert!(last_line.contains("Some text"), "last line should be paragraph");
}

#[test]
fn test_table_followed_by_non_table_line() {
    let text = "| A | B |\n|---|---|\n| 1 | 2 |\nNot a table row";
    let result = render_markdown(text, None);
    // table (5) + blank + paragraph = 7
    assert_eq!(result.len(), 7, "non-table row should end table and render as paragraph");
    let last_line = format!("{:?}", result[6]);
    assert!(last_line.contains("Not a table row"), "non-table row should render as paragraph");
}

#[test]
fn test_table_not_confused_with_hr() {
    // Ensure a table separator row is not confused with a horizontal rule
    let text = "| x |\n|---|\n| y |";
    let result = render_markdown(text, None);
    assert_eq!(result.len(), 5, "should be a table, not HR");
    let top_str = format!("{:?}", result[0]);
    assert!(top_str.contains('┌'), "should have table border, not HR");
}

#[test]
fn test_table_in_mixed_content() {
    let text = "# Title\n\n| Name | Value |\n|------|-------|\n| foo  | bar   |\n\n- List item";
    let result = render_markdown(text, None);
    assert!(result.len() >= 8, "mixed content with table should have >= 8 lines, got {}", result.len());
    let heading_str = format!("{:?}", result[0]);
    assert!(heading_str.contains("Title"));
    let all_str = format!("{:?}", result);
    assert!(all_str.contains('┌'), "should contain table top border");
    assert!(all_str.contains('└'), "should contain table bottom border");
}

#[test]
fn test_mixed() {
    let text = "# Title\n\nSome **bold** and `code` text\n\n```rust\nfn main() {}\n```\n\n- List item\n> Quote\n\n---\n";
    let result = render_markdown(text, None);
    assert!(result.len() > 10);
}
