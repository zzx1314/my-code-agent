use my_code_agent::context::{expand_file_refs, AttachStatus};
use std::fs;

#[test]
fn test_expand_single_file() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("hello.txt");
    fs::write(&file_path, "hello world").unwrap();

    let path = file_path.to_str().unwrap();
    let input = format!("explain @{}", path);
    let result = expand_file_refs(&input);

    assert!(result.expanded.contains("hello world"));
    assert!(result.expanded.contains("<file"));
    assert_eq!(result.attachments.len(), 1);
    assert!(matches!(result.attachments[0].1, AttachStatus::Attached { .. }));
}

#[test]
fn test_expand_multiple_files() {
    let dir = tempfile::tempdir().unwrap();
    let f1 = dir.path().join("a.txt");
    let f2 = dir.path().join("b.rs");
    fs::write(&f1, "file A").unwrap();
    fs::write(&f2, "fn main() {}").unwrap();

    let p1 = f1.to_str().unwrap();
    let p2 = f2.to_str().unwrap();
    let input = format!("compare @{} and @{}", p1, p2);
    let result = expand_file_refs(&input);

    assert!(result.expanded.contains("file A"));
    assert!(result.expanded.contains("fn main()"));
    assert_eq!(result.attachments.len(), 2);
}

#[test]
fn test_expand_missing_file() {
    let result = expand_file_refs("read @/nonexistent/path/to/file.txt");
    assert!(result.expanded.contains("error="));
    assert!(result.expanded.contains("<file"));
    assert_eq!(result.attachments.len(), 1);
    assert!(matches!(result.attachments[0].1, AttachStatus::Error(_)));
}

#[test]
fn test_expand_no_refs() {
    let result = expand_file_refs("just a normal message");
    assert_eq!(result.expanded, "just a normal message");
    assert!(result.attachments.is_empty());
}

#[test]
fn test_expand_trailing_punctuation_preserved() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.txt");
    fs::write(&file_path, "content").unwrap();

    let path = file_path.to_str().unwrap();
    let input = format!("look at @{}, please", path);
    let result = expand_file_refs(&input);

    // The trailing comma should remain in the output
    assert!(result.expanded.contains(", please"));
    assert!(result.expanded.contains("content"));
}

#[test]
fn test_expand_in_brackets() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.txt");
    fs::write(&file_path, "content").unwrap();

    let path = file_path.to_str().unwrap();
    let input = format!("see (@{}) for details", path);
    let result = expand_file_refs(&input);

    assert!(result.expanded.contains("content"));
    assert_eq!(result.attachments.len(), 1);
}
