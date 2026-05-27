use my_code_agent::core::context::tool_dedup::{DedupAction, ToolCallDedup};
use std::fs;
use tempfile::TempDir;

/// Helper: create a temp file with content, return (TempDir, path_string).
fn make_temp_file(name: &str, content: &str) -> (TempDir, String) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(name);
    fs::write(&path, content).unwrap();
    (dir, path.to_string_lossy().to_string())
}

#[test]
fn test_outline_first_call_is_allow() {
    let (_dir, path) = make_temp_file("a.rs", "fn foo() {}\nfn bar() {}\n");
    let mut dedup = ToolCallDedup::new();

    let action = dedup.check_file_outline(&path);
    assert!(
        matches!(action, DedupAction::Allow),
        "First outline call should be Allow"
    );
}

#[test]
fn test_outline_second_call_returns_cached() {
    let (_dir, path) = make_temp_file("b.rs", "fn foo() {}\nfn bar() {}\n");
    let mut dedup = ToolCallDedup::new();

    dedup.record_file_outline(&path, 2, "outline content");

    let action = dedup.check_file_outline(&path);
    assert!(
        matches!(action, DedupAction::ShortCircuit(_)),
        "Duplicate outline should be ShortCircuit"
    );
    if let DedupAction::ShortCircuit(info) = action {
        assert_eq!(info.cached_outline, "outline content".to_string());
        assert_eq!(info.total_lines, 2);
    }
}

#[test]
fn test_outline_invalidate_path_causes_re_outline() {
    let (_dir, path) = make_temp_file("c.rs", "fn foo() {}\n");
    let mut dedup = ToolCallDedup::new();

    dedup.record_file_outline(&path, 1, "outline content");

    dedup.invalidate_path(&path);

    let action = dedup.check_file_outline(&path);
    assert!(
        matches!(action, DedupAction::Allow),
        "After invalidate_path should be Allow"
    );
}

#[test]
fn test_outline_invalidate_all_causes_re_outline() {
    let (_dir, path) = make_temp_file("d.rs", "fn foo() {}\n");
    let mut dedup = ToolCallDedup::new();

    dedup.record_file_outline(&path, 1, "outline content");

    dedup.invalidate_all();

    let action = dedup.check_file_outline(&path);
    assert!(
        matches!(action, DedupAction::Allow),
        "After invalidate_all should be Allow"
    );
}

#[test]
fn test_outline_reset_clears_all() {
    let (_dir, path) = make_temp_file("e.rs", "fn foo() {}\n");
    let mut dedup = ToolCallDedup::new();

    dedup.record_file_outline(&path, 1, "outline content");

    dedup.reset();

    let action = dedup.check_file_outline(&path);
    assert!(
        matches!(action, DedupAction::Allow),
        "After reset should be Allow"
    );
}

#[test]
fn test_outline_modified_file_not_short_circuited() {
    let (_dir, path) = make_temp_file("f.rs", "fn foo() {}\n");
    let mut dedup = ToolCallDedup::new();

    dedup.record_file_outline(&path, 1, "outline content");

    // Wait a bit and modify the file
    std::thread::sleep(std::time::Duration::from_millis(50));
    fs::write(&path, "fn bar() {}\n").unwrap();

    // Should be Allow because file was modified
    let action = dedup.check_file_outline(&path);
    assert!(
        matches!(action, DedupAction::Allow),
        "Modified file should be Allow"
    );
}

#[test]
fn test_outline_len_tracks_entries() {
    let mut dedup = ToolCallDedup::new();
    assert_eq!(dedup.len(), 0);

    let (_dir1, path1) = make_temp_file("g.rs", "a\n");
    let (_dir2, path2) = make_temp_file("h.rs", "b\n");

    dedup.record_file_outline(&path1, 1, "outline1");
    assert_eq!(dedup.len(), 1);

    dedup.record_file_outline(&path2, 1, "outline2");
    assert_eq!(dedup.len(), 2);

    dedup.invalidate_path(&path1);
    assert_eq!(dedup.len(), 1);
}

#[test]
fn test_outline_different_path_not_duplicate() {
    let (_dir1, path1) = make_temp_file("i.rs", "a\n");
    let (_dir2, path2) = make_temp_file("j.rs", "b\n");
    let mut dedup = ToolCallDedup::new();

    dedup.record_file_outline(&path1, 1, "outline1");

    // Different path → not a duplicate
    let action = dedup.check_file_outline(&path2);
    assert!(
        matches!(action, DedupAction::Allow),
        "Different path should be Allow"
    );
}
