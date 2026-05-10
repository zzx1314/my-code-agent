use my_code_agent::core::tool_dedup::{DedupAction, ToolCallDedup};
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
fn test_dedup_first_read_is_allow() {
    let (_dir, path) = make_temp_file("a.rs", "line1\nline2\nline3\n");
    let mut dedup = ToolCallDedup::new();

    let action = dedup_check(&mut dedup, &path, 0, 200);
    assert!(matches!(action, DedupAction::Allow), "First read should be Allow");
}

#[test]
fn test_dedup_second_identical_read_is_short_circuit() {
    let (_dir, path) = make_temp_file("b.rs", "line1\nline2\nline3\n");
    let mut dedup = ToolCallDedup::new();

    // Record a read
    dedup.record_file_read(&path, 0, 200, 3, 0, 3);

    // Second read with same params should short-circuit
    let action = dedup_check(&mut dedup, &path, 0, 200);
    assert!(
        matches!(action, DedupAction::ShortCircuit(_)),
        "Duplicate read should be ShortCircuit"
    );
}

#[test]
fn test_dedup_third_read_allows_full_re_read() {
    let (_dir, path) = make_temp_file("c.rs", "line1\nline2\nline3\n");
    let mut dedup = ToolCallDedup::new();

    dedup.record_file_read(&path, 0, 200, 3, 0, 3);

    // First hit → short circuit
    let _ = dedup_check(&mut dedup, &path, 0, 200);
    // Second hit → allow (model may have lost context)
    let action = dedup_check(&mut dedup, &path, 0, 200);
    assert!(matches!(action, DedupAction::Allow), "Third read should Allow");
}

#[test]
fn test_dedup_short_circuit_message_format() {
    let (_dir, path) = make_temp_file("d.rs", "line1\nline2\nline3\n");
    let mut dedup = ToolCallDedup::new();

    dedup.record_file_read(&path, 0, 200, 3, 0, 3);

    let action = dedup_check(&mut dedup, &path, 0, 200);
    if let DedupAction::ShortCircuit(info) = action {
        let msg = info.format_message();
        assert!(msg.contains("[DEDUP]"), "Message should contain [DEDUP] prefix");
        assert!(msg.contains(&path), "Message should contain file path");
        assert!(msg.contains("conversation history"), "Message should reference conversation history");
    } else {
        panic!("Expected ShortCircuit");
    }
}

#[test]
fn test_dedup_different_offset_not_duplicate() {
    let (_dir, path) = make_temp_file("e.rs", "line1\nline2\nline3\n");
    let mut dedup = ToolCallDedup::new();

    dedup.record_file_read(&path, 0, 200, 3, 0, 3);

    // Different offset → not a duplicate
    let action = dedup_check(&mut dedup, &path, 1, 200);
    assert!(matches!(action, DedupAction::Allow), "Different offset should be Allow");
}

#[test]
fn test_dedup_different_limit_not_duplicate() {
    let (_dir, path) = make_temp_file("f.rs", "line1\nline2\nline3\n");
    let mut dedup = ToolCallDedup::new();

    dedup.record_file_read(&path, 0, 200, 3, 0, 3);

    // Different limit → not a duplicate
    let action = dedup_check(&mut dedup, &path, 0, 50);
    assert!(matches!(action, DedupAction::Allow), "Different limit should be Allow");
}

#[test]
fn test_dedup_different_path_not_duplicate() {
    let (_dir, path1) = make_temp_file("g.rs", "line1\nline2\nline3\n");
    let (_dir2, path2) = make_temp_file("h.rs", "line1\nline2\nline3\n");
    let mut dedup = ToolCallDedup::new();

    dedup.record_file_read(&path1, 0, 200, 3, 0, 3);

    // Different path → not a duplicate
    let action = dedup_check(&mut dedup, &path2, 0, 200);
    assert!(matches!(action, DedupAction::Allow), "Different path should be Allow");
}

#[test]
fn test_dedup_invalidate_path_causes_re_read() {
    let (_dir, path) = make_temp_file("i.rs", "line1\nline2\nline3\n");
    let mut dedup = ToolCallDedup::new();

    dedup.record_file_read(&path, 0, 200, 3, 0, 3);

    // Invalidate
    dedup.invalidate_path(&path);

    // Should be Allow again
    let action = dedup_check(&mut dedup, &path, 0, 200);
    assert!(matches!(action, DedupAction::Allow), "After invalidation should be Allow");
}

#[test]
fn test_dedup_reset_clears_all() {
    let (_dir, path) = make_temp_file("j.rs", "line1\nline2\nline3\n");
    let mut dedup = ToolCallDedup::new();

    dedup.record_file_read(&path, 0, 200, 3, 0, 3);

    // Reset
    dedup.reset();

    // Should be Allow again
    let action = dedup_check(&mut dedup, &path, 0, 200);
    assert!(matches!(action, DedupAction::Allow), "After reset should be Allow");
}

#[test]
fn test_dedup_modified_file_not_short_circuited() {
    let (_dir, path) = make_temp_file("k.rs", "line1\nline2\nline3\n");
    let mut dedup = ToolCallDedup::new();

    dedup.record_file_read(&path, 0, 200, 3, 0, 3);

    // Wait a bit and modify the file
    std::thread::sleep(std::time::Duration::from_millis(50));
    fs::write(&path, "modified content\n").unwrap();

    // Should be Allow because file was modified
    let action = dedup_check(&mut dedup, &path, 0, 200);
    assert!(matches!(action, DedupAction::Allow), "Modified file should be Allow");
}

#[test]
fn test_dedup_len_tracks_entries() {
    let mut dedup = ToolCallDedup::new();
    assert_eq!(dedup.len(), 0);

    let (_dir1, path1) = make_temp_file("l.rs", "a\n");
    let (_dir2, path2) = make_temp_file("m.rs", "b\n");

    dedup.record_file_read(&path1, 0, 200, 1, 0, 1);
    assert_eq!(dedup.len(), 1);

    dedup.record_file_read(&path2, 0, 200, 1, 0, 1);
    assert_eq!(dedup.len(), 2);

    dedup.invalidate_path(&path1);
    assert_eq!(dedup.len(), 1);
}

/// Helper that mimics what FileRead::call does.
fn dedup_check(dedup: &mut ToolCallDedup, path: &str, offset: usize, limit: usize) -> DedupAction {
    dedup.check_file_read(path, offset, limit)
}
