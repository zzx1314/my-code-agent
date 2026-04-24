use my_code_agent::ui::render::{MarkdownRenderer, ReasoningTracker};

// ── MarkdownRenderer state tests ──

#[test]
fn test_renderer_new_is_empty() {
    let r = MarkdownRenderer::new();
    assert!(r.current_line().is_empty());
    assert!(r.complete_lines().is_empty());
}

#[test]
fn test_renderer_default_is_empty() {
    let r = MarkdownRenderer::default();
    assert!(r.current_line().is_empty());
    assert!(r.complete_lines().is_empty());
}

#[test]
fn test_renderer_partial_line_accumulates() {
    let mut r = MarkdownRenderer::new();
    // No newline — text goes into current_line
    r.push_text("hello ");
    r.push_text("world");
    assert_eq!(r.current_line(), "hello world");
    assert!(r.complete_lines().is_empty());
}

#[test]
fn test_renderer_newline_moves_to_complete() {
    let mut r = MarkdownRenderer::new();
    // Text with a newline: before-last-newline (including \n) is a complete line,
    // after-last-newline becomes the new current_line
    r.push_text("line one\nline two");
    // After processing, "line one\n" was rendered (complete_lines was cleared after render)
    // and "line two" is the new current_line
    assert_eq!(r.current_line(), "line two");
    assert!(r.complete_lines().is_empty()); // cleared after rendering
}

#[test]
fn test_renderer_trailing_newline_clears_current() {
    let mut r = MarkdownRenderer::new();
    r.push_text("line one\n");
    assert!(r.current_line().is_empty());
    assert!(r.complete_lines().is_empty()); // rendered and cleared
}

#[test]
fn test_renderer_multiple_newlines() {
    let mut r = MarkdownRenderer::new();
    r.push_text("aaa\nbbb\nccc");
    assert_eq!(r.current_line(), "ccc");
    assert!(r.complete_lines().is_empty());
}

#[test]
fn test_renderer_accumulate_then_newline() {
    let mut r = MarkdownRenderer::new();
    r.push_text("partial");
    assert_eq!(r.current_line(), "partial");
    r.push_text(" continued\nnext");
    assert_eq!(r.current_line(), "next");
}

#[test]
fn test_renderer_flush_moves_current_to_complete() {
    let mut r = MarkdownRenderer::new();
    r.push_text("unfinished line");
    assert_eq!(r.current_line(), "unfinished line");
    // Flush moves current_line into complete_lines for rendering, then clears both
    r.flush();
    assert!(r.current_line().is_empty());
    assert!(r.complete_lines().is_empty()); // cleared after rendering
}

#[test]
fn test_renderer_flush_empty_is_noop() {
    let mut r = MarkdownRenderer::new();
    r.flush(); // should not panic
    assert!(r.current_line().is_empty());
    assert!(r.complete_lines().is_empty());
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
    // reasoning_buf is NOT cleared by flush_unfinished
    assert_eq!(t.reasoning_buf(), "interrupted thought");
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
