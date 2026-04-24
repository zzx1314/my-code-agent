use my_code_agent::context::{expand_file_refs, parse_file_refs, AttachStatus};
use my_code_agent::config::Config;
use std::fs;

// ── parse_file_refs unit tests (moved from src/core/context.rs) ──

#[test]
fn test_parse_simple_ref() {
    let refs = parse_file_refs("explain @src/main.rs");
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].path, "src/main.rs");
}

#[test]
fn test_parse_multiple_refs() {
    let refs = parse_file_refs("compare @src/main.rs and @src/lib.rs");
    assert_eq!(refs.len(), 2);
    assert_eq!(refs[0].path, "src/main.rs");
    assert_eq!(refs[1].path, "src/lib.rs");
}

#[test]
fn test_parse_trailing_punctuation() {
    let refs = parse_file_refs("look at @src/main.rs,");
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].path, "src/main.rs");
}

#[test]
fn test_parse_trailing_punctuation_variants() {
    assert_eq!(parse_file_refs("@foo.rs:")[0].path, "foo.rs");
    assert_eq!(parse_file_refs("@foo.rs;")[0].path, "foo.rs");
    assert_eq!(parse_file_refs("@foo.rs!")[0].path, "foo.rs");
    assert_eq!(parse_file_refs("@foo.rs?")[0].path, "foo.rs");
    // @ inside opening brackets IS treated as a ref
    assert_eq!(parse_file_refs("(@foo.rs)")[0].path, "foo.rs");
    assert_eq!(parse_file_refs("[@foo.rs]")[0].path, "foo.rs");
    assert_eq!(parse_file_refs("{@foo.rs}")[0].path, "foo.rs");
}

#[test]
fn test_parse_no_refs() {
    let refs = parse_file_refs("just a normal message");
    assert_eq!(refs.len(), 0);
}

#[test]
fn test_parse_email_not_treated_as_ref() {
    let refs = parse_file_refs("send to user@example.com");
    assert_eq!(refs.len(), 0);
}

#[test]
fn test_parse_at_start() {
    let refs = parse_file_refs("@README.md what is this");
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].path, "README.md");
}

#[test]
fn test_parse_dot_in_extension() {
    let refs = parse_file_refs("check @src/main.rs and @Cargo.toml");
    assert_eq!(refs.len(), 2);
    assert_eq!(refs[0].path, "src/main.rs");
    assert_eq!(refs[1].path, "Cargo.toml");
}

// ── expand_file_refs integration tests ──

#[test]
fn test_expand_single_file() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("hello.txt");
    fs::write(&file_path, "hello world").unwrap();

    let path = file_path.to_str().unwrap();
    let input = format!("explain @{}", path);
    let config = Config::default();
    let result = expand_file_refs(&input, &config);

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
    let config = Config::default();
    let result = expand_file_refs(&input, &config);

    assert!(result.expanded.contains("file A"));
    assert!(result.expanded.contains("fn main()"));
    assert_eq!(result.attachments.len(), 2);
}

#[test]
fn test_expand_missing_file_absolute() {
    let config = Config::default();
    let result = expand_file_refs("read @/nonexistent/path/to/file.txt", &config);
    assert!(result.expanded.contains("error="));
    assert!(result.expanded.contains("<file"));
    assert_eq!(result.attachments.len(), 1);
    assert!(matches!(result.attachments[0].1, AttachStatus::Error(_)));
}

#[test]
fn test_expand_missing_file_relative() {
    let config = Config::default();
    let result = expand_file_refs("read @nonexistent/file.txt", &config);
    assert!(result.expanded.contains("error="));
    assert!(result.expanded.contains("<file"));
    assert_eq!(result.attachments.len(), 1);
}

#[test]
fn test_expand_no_refs() {
    let config = Config::default();
    let result = expand_file_refs("just a normal message", &config);
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
    let config = Config::default();
    let result = expand_file_refs(&input, &config);

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
    let config = Config::default();
    let result = expand_file_refs(&input, &config);

    assert!(result.expanded.contains("content"));
    assert_eq!(result.attachments.len(), 1);
}

#[test]
fn test_expand_truncates_large_file() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("big.txt");
    let content: String = (0..600).map(|i| format!("line {}\n", i)).collect();
    fs::write(&file_path, &content).unwrap();

    let config = Config::default();
    let rel_path = file_path.to_str().unwrap();
    let input = format!("read @{}", rel_path);
    let result = expand_file_refs(&input, &config);
    assert!(result.expanded.contains("file truncated"));
    assert_eq!(result.attachments.len(), 1);
    match &result.attachments[0].1 {
        AttachStatus::Attached { truncated, .. } => assert!(truncated),
        _ => panic!("expected Attached status"),
    }
}
