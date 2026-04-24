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
    // Bare trailing `:` (no digits) is stripped like other punctuation
    assert_eq!(parse_file_refs("@foo.rs:")[0].path, "foo.rs");
    assert_eq!(parse_file_refs("@foo.rs:")[0].offset, 0);
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
    // Verify offset=0 and shown equals total lines for a small file
    match &result.attachments[0].1 {
        AttachStatus::Attached { offset, shown, lines, truncated } => {
            assert_eq!(*offset, 0);
            assert_eq!(*shown, *lines);
            assert!(!truncated);
        }
        _ => panic!("expected Attached status"),
    }
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
    // Truncation notice should now suggest both @path:N and file_read
    assert!(result.expanded.contains(&format!("@{}:", rel_path)));
    assert!(result.expanded.contains("file_read tool with offset="));
    assert_eq!(result.attachments.len(), 1);
    match &result.attachments[0].1 {
        AttachStatus::Attached { truncated, offset, shown, lines } => {
            assert!(truncated);
            assert_eq!(*offset, 0);
            assert!(*shown < *lines);
        }
        _ => panic!("expected Attached status"),
    }
}

// ── Offset syntax tests ──

#[test]
fn test_parse_offset_suffix() {
    let refs = parse_file_refs("read @src/main.rs:50");
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].path, "src/main.rs");
    assert_eq!(refs[0].offset, 50);
}

#[test]
fn test_parse_offset_zero() {
    let refs = parse_file_refs("read @src/main.rs:0");
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].path, "src/main.rs");
    assert_eq!(refs[0].offset, 0);
}

#[test]
fn test_parse_bare_colon_not_offset() {
    // Trailing `:` without digits is just punctuation, stripped from path
    let refs = parse_file_refs("look at @src/main.rs:");
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].path, "src/main.rs");
    assert_eq!(refs[0].offset, 0);
}

#[test]
fn test_parse_colon_with_non_digits_not_offset() {
    // `:abc` is not an offset — the full token (minus trailing punctuation) is the path
    // Note: `trim_end_matches` only strips matching chars from the END, so `:abc` is NOT stripped.
    // This matches the original behavior before offset support was added.
    let refs = parse_file_refs("look at @src/main.rs:abc");
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].path, "src/main.rs:abc");
    assert_eq!(refs[0].offset, 0);
}

#[test]
fn test_parse_offset_with_trailing_punctuation() {
    let refs = parse_file_refs("read @src/main.rs:50,");
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].path, "src/main.rs");
    assert_eq!(refs[0].offset, 50);
}


#[test]
fn test_expand_with_offset() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("lines.txt");
    let content: String = (0..10).map(|i| format!("line {}\n", i)).collect();
    fs::write(&file_path, &content).unwrap();

    let path = file_path.to_str().unwrap();
    let input = format!("read @{}:5", path);
    let config = Config::default();
    let result = expand_file_refs(&input, &config);

    // Should contain lines 5–9 (0-indexed), not line 0–4
    assert!(result.expanded.contains("line 5"));
    assert!(result.expanded.contains("line 9"));
    assert!(!result.expanded.contains("line 0\n"));
    assert!(!result.expanded.contains("line 4"));
    // Should include offset attribute in the XML tag
    assert!(result.expanded.contains("offset=\"5\""));
    // Should include end-of-file notice
    assert!(result.expanded.contains("end of file"));
    assert_eq!(result.attachments.len(), 1);
    // Verify AttachStatus fields for offset read
    match &result.attachments[0].1 {
        AttachStatus::Attached { offset, shown, lines, truncated } => {
            assert_eq!(*offset, 5);
            assert_eq!(*shown, 5); // lines 5–9 = 5 lines
            assert_eq!(*lines, 10);
            assert!(!truncated);
        }
        _ => panic!("expected Attached status"),
    }
}

#[test]
fn test_expand_with_offset_truncated() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("big.txt");
    // 1000 lines so that offset 300 leaves 700 remaining > 500 max_lines → truncated
    let content: String = (0..1000).map(|i| format!("line {}\n", i)).collect();
    fs::write(&file_path, &content).unwrap();

    let path = file_path.to_str().unwrap();
    let input = format!("read @{}:300", path);
    let config = Config::default();
    let result = expand_file_refs(&input, &config);

    // Should contain line 300 onward, truncated
    assert!(result.expanded.contains("line 300"));
    // Truncation notice should suggest @path:N for next chunk
    assert!(result.expanded.contains(&format!("@{}:", path)));
    assert!(result.expanded.contains("to read the next chunk"));
    // Should include offset attribute
    assert!(result.expanded.contains("offset=\"300\""));
    match &result.attachments[0].1 {
        AttachStatus::Attached { truncated, offset, shown, lines } => {
            assert!(truncated);
            assert_eq!(*offset, 300);
            assert!(*shown < *lines);
        }
        _ => panic!("expected Attached status with truncation"),
    }
}

#[test]
fn test_expand_offset_beyond_file_end() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("small.txt");
    fs::write(&file_path, "only one line\n").unwrap();

    let path = file_path.to_str().unwrap();
    let input = format!("read @{}:999", path);
    let config = Config::default();
    let result = expand_file_refs(&input, &config);

    // Offset beyond file end should produce empty content, not an error
    assert!(result.expanded.contains("offset=\"999\""));
    // Should show a clear notice about the offset being beyond the file
    assert!(result.expanded.contains("beyond end of file"));
    assert_eq!(result.attachments.len(), 1);
    // Verify AttachStatus: offset beyond end, shown=0, not truncated
    match &result.attachments[0].1 {
        AttachStatus::Attached { offset, shown, lines, truncated } => {
            assert_eq!(*offset, 999);
            assert_eq!(*shown, 0);
            assert_eq!(*lines, 1);
            assert!(!truncated);
        }
        _ => panic!("expected Attached status"),
    }
}
