use my_code_agent::ui::render::{MarkdownRenderer, ReasoningTracker};
use my_code_agent::ui::render::{render_full, render_streaming_markdown};

// ── MarkdownRenderer state tests ──

#[test]
fn test_renderer_new_is_empty() {
    let r = MarkdownRenderer::new();
    assert!(r.get_buffer().is_empty());
}

#[test]
fn test_renderer_default_is_empty() {
    let r = MarkdownRenderer::default();
    assert!(r.get_buffer().is_empty());
}

#[test]
fn test_renderer_push_text_accumulates() {
    let mut r = MarkdownRenderer::new();
    r.push_text("hello ");
    r.push_text("world");
    assert_eq!(r.get_buffer(), "hello world");
}

#[test]
fn test_renderer_flush_noop() {
    let mut r = MarkdownRenderer::new();
    r.push_text("some text");
    r.flush(); // currently no-op, should not panic
    assert_eq!(r.get_buffer(), "some text");
}

#[test]
fn test_renderer_take_buffer_resets() {
    let mut r = MarkdownRenderer::new();
    r.push_text("content");
    let taken = r.take_buffer();
    assert_eq!(taken, "content");
    assert!(r.get_buffer().is_empty());
}

#[test]
fn test_renderer_take_buffer_then_push() {
    let mut r = MarkdownRenderer::new();
    r.push_text("first");
    assert_eq!(r.take_buffer(), "first");
    r.push_text("second");
    assert_eq!(r.get_buffer(), "second");
}

// ── ReasoningTracker state tests ──

#[test]
fn test_tracker_new_is_empty() {
    let t = ReasoningTracker::new();
    assert!(!t.is_reasoning());
    assert!(t.reasoning_buf().is_empty());
    assert!(t.total_reasoning().is_empty());
}

#[test]
fn test_tracker_default_is_empty() {
    let t = ReasoningTracker::default();
    assert!(!t.is_reasoning());
}

#[test]
fn test_tracker_append_starts_reasoning() {
    let mut t = ReasoningTracker::new();
    t.append("thinking...");
    assert!(t.is_reasoning());
    assert_eq!(t.reasoning_buf(), "thinking...");
}

#[test]
fn test_tracker_append_accumulates() {
    let mut t = ReasoningTracker::new();
    t.append("part one ");
    t.append("part two");
    assert_eq!(t.reasoning_buf(), "part one part two");
    assert!(t.is_reasoning());
    // total_reasoning is still empty until end_segment
    assert!(t.total_reasoning().is_empty());
}

#[test]
fn test_tracker_end_segment_accumulates_total() {
    let mut t = ReasoningTracker::new();
    t.append("first reasoning");
    t.end_segment();
    assert!(!t.is_reasoning());
    assert!(t.reasoning_buf().is_empty());
    // total_reasoning includes the segment text + newline
    assert_eq!(t.total_reasoning(), "first reasoning\n");
}

#[test]
fn test_tracker_multiple_segments() {
    let mut t = ReasoningTracker::new();
    t.append("segment one");
    t.end_segment();
    t.append("segment two");
    t.end_segment();
    assert_eq!(t.total_reasoning(), "segment one\nsegment two\n");
}

#[test]
fn test_tracker_end_segment_empty_buf_no_junk() {
    let mut t = ReasoningTracker::new();
    // Ending a segment without ever appending should not add anything
    t.end_segment();
    assert!(t.total_reasoning().is_empty());
}

#[test]
fn test_tracker_flush_unfinished() {
    let mut t = ReasoningTracker::new();
    t.append("interrupted thought");
    t.flush_unfinished();
    assert_eq!(t.total_reasoning(), "interrupted thought\n");
    // reasoning_buf is cleared by flush_unfinished
    assert_eq!(t.reasoning_buf(), "");
}

#[test]
fn test_tracker_into_total_reasoning_consumes() {
    let mut t = ReasoningTracker::new();
    t.append("hello");
    t.end_segment();
    let total = t.into_total_reasoning();
    assert_eq!(total, "hello\n");
}

#[test]
fn test_tracker_reasoning_state_transitions() {
    let mut t = ReasoningTracker::new();
    assert!(!t.is_reasoning());
    t.append("start");
    assert!(t.is_reasoning());
    t.end_segment();
    assert!(!t.is_reasoning());
    t.append("again");
    assert!(t.is_reasoning());
    t.end_segment();
    assert!(!t.is_reasoning());
}

// ── render_streaming_markdown (custom renderer) ──

#[test]
fn test_render_empty() {
    let lines = render_streaming_markdown("");
    assert!(lines.is_empty());
}

#[test]
fn test_render_plain_text() {
    let lines = render_streaming_markdown("hello world");
    assert!(!lines.is_empty());
}

#[test]
fn test_render_closed_code_block() {
    let text = "```rust\nfn main() {}\n```";
    let lines = render_streaming_markdown(text);
    assert!(!lines.is_empty());
}

#[test]
fn test_render_unclosed_code_block_produces_lines() {
    let text = "Here is code:\n```rust\nfn main() {\n    println!(\"hi\");";
    let lines = render_streaming_markdown(text);
    assert!(!lines.is_empty());
    let plain_lines = render_streaming_markdown("Here is code:");
    assert!(lines.len() > plain_lines.len());
}

#[test]
fn test_render_unclosed_fence_at_start() {
    let text = "```rust\nfn main() {";
    let lines = render_streaming_markdown(text);
    assert!(!lines.is_empty());
}

#[test]
fn test_render_text_then_unclosed_fence() {
    let text = "intro text\n```python\nprint(";
    let lines = render_streaming_markdown(text);
    assert!(!lines.is_empty());
    let intro_only = render_streaming_markdown("intro text");
    assert!(lines.len() > intro_only.len());
}

#[test]
fn test_render_two_closed_fences() {
    let text = "```rust\na\n```\n```python\nb\n```";
    let lines = render_streaming_markdown(text);
    assert!(!lines.is_empty());
}

#[test]
fn test_render_second_fence_unclosed() {
    let text = "```rust\na\n```\n```python\nb";
    let lines = render_streaming_markdown(text);
    assert!(!lines.is_empty());
}

// ── render_full (same renderer, non-streaming alias) ──

#[test]
fn test_render_full_same_as_streaming() {
    let text = "# Title\n\nSome **bold** text\n\n```rust\nfn main() {}\n```";
    let streaming = render_streaming_markdown(text);
    let full = render_full(text);
    assert_eq!(streaming.len(), full.len());
}

#[test]
fn test_render_full_heading() {
    let lines = render_full("# Hello World");
    assert!(!lines.is_empty());
    let line_str = format!("{:?}", lines[0]);
    assert!(line_str.contains("Hello World"));
}

#[test]
fn test_render_full_bold() {
    let lines = render_full("This is **bold** text");
    assert!(!lines.is_empty());
}

#[test]
fn test_render_full_inline_code() {
    let lines = render_full("Use `println!` for output");
    assert!(!lines.is_empty());
}

#[test]
fn test_render_full_horizontal_rule() {
    let lines = render_full("---");
    assert!(!lines.is_empty());
}

#[test]
fn test_render_full_blockquote() {
    let lines = render_full("> This is a quote");
    assert!(!lines.is_empty());
}

#[test]
fn test_render_full_unordered_list() {
    let lines = render_full("- Item 1\n- Item 2");
    assert!(lines.len() >= 2);
}

#[test]
fn test_render_full_ordered_list() {
    let lines = render_full("1. First\n2. Second");
    assert!(lines.len() >= 2);
}

#[test]
fn test_render_full_link() {
    let lines = render_full("[Rust](https://rust-lang.org)");
    assert!(!lines.is_empty());
}

#[test]
fn test_render_full_mixed() {
    let text = "# Title\n\nSome **bold** and `code` text\n\n```rust\nfn main() {}\n```\n\n- List item\n> Quote\n\n---\n";
    let lines = render_full(text);
    assert!(lines.len() > 10);
}
