use my_code_agent::ui::render::{MarkdownRenderer, ReasoningTracker};

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
