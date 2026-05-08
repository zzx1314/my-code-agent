use my_code_agent::ui::markdown::render_markdown;

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
