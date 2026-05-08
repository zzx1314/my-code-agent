use my_code_agent::tools::file_undo::apply_undo;
use my_code_agent::tools::undo_history::{
    pop_current_session_entries, record_change, set_session_id,
};
use tempfile::tempdir;

#[test]
fn test_undo_session_scoped() {
    // Clean up shared state
    let _ = std::fs::remove_file(".undo_history.json");

    // Set a session ID for testing
    set_session_id("test_session".to_string());

    // --- Test 1: Undo write (new file) ---
    {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.txt");

        record_change(
            &file_path.to_string_lossy(),
            None,
            Some("hello".to_string()),
            "file_write",
        )
        .unwrap();

        std::fs::write(&file_path, "hello").unwrap();
        assert!(file_path.exists());

        let entries = pop_current_session_entries().unwrap();
        assert_eq!(entries.len(), 1);
        let mut details = Vec::new();
        apply_undo(&entries[0], &mut details).unwrap();

        assert!(!file_path.exists());
        assert_eq!(details[0].action, "deleted file (was newly created)");
    }

    // --- Test 2: Undo update (existing file) ---
    {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test2.txt");

        std::fs::write(&file_path, "old content").unwrap();

        record_change(
            &file_path.to_string_lossy(),
            Some("old content".to_string()),
            Some("new content".to_string()),
            "file_update",
        )
        .unwrap();

        std::fs::write(&file_path, "new content").unwrap();

        let entries = pop_current_session_entries().unwrap();
        assert_eq!(entries.len(), 1);
        let mut details = Vec::new();
        apply_undo(&entries[0], &mut details).unwrap();

        assert_eq!(std::fs::read_to_string(&file_path).unwrap(), "old content");
        assert_eq!(details[0].action, "restored previous content");
    }
}
