use my_code_agent::tools::FileUpdate;
use my_code_agent::tools::Tool;
use std::fs;

async fn call_update(
    path: &str,
    start_line: usize,
    delete_count: usize,
    new_content: &str,
) -> Result<String, String> {
    let args = serde_json::to_value(my_code_agent::tools::file_update::FileUpdateArgs {
        path: path.to_string(),
        start_line,
        delete_count,
        new_content: new_content.to_string(),
    })
    .unwrap();
    FileUpdate.call(args).await
}

fn parse_output(result: &str) -> my_code_agent::tools::file_update::FileUpdateOutput {
    serde_json::from_str(result).unwrap()
}

#[tokio::test]
async fn test_replace_single_line() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    fs::write(&path, "hello world").unwrap();

    let result = call_update(path.to_str().unwrap(), 1, 1, "hi world")
        .await
        .unwrap();
    let output = parse_output(&result);
    assert_eq!(fs::read_to_string(&path).unwrap(), "hi world");
    assert_eq!(output.replacements, 1);
}

#[tokio::test]
async fn test_replace_multiple_lines() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    fs::write(&path, "line1\nline2\nline3").unwrap();

    let result = call_update(path.to_str().unwrap(), 1, 3, "new1\nnew2\nnew3")
        .await
        .unwrap();
    let output = parse_output(&result);
    assert_eq!(fs::read_to_string(&path).unwrap(), "new1\nnew2\nnew3");
    assert_eq!(output.replacements, 3);
}

#[tokio::test]
async fn test_insert_lines() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    fs::write(&path, "line2\nline3").unwrap();

    let result = call_update(path.to_str().unwrap(), 1, 0, "line1")
        .await
        .unwrap();
    let output = parse_output(&result);
    assert_eq!(fs::read_to_string(&path).unwrap(), "line1\nline2\nline3");
    assert_eq!(output.replacements, 0);
}

#[tokio::test]
async fn test_delete_lines() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    fs::write(&path, "delete_me\nkeep_me").unwrap();

    let result = call_update(path.to_str().unwrap(), 1, 1, "")
        .await
        .unwrap();
    let output = parse_output(&result);
    assert_eq!(fs::read_to_string(&path).unwrap(), "keep_me");
    assert_eq!(output.replacements, 1);
}

#[tokio::test]
async fn test_start_line_out_of_range() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    fs::write(&path, "hello world").unwrap();

    let result = call_update(path.to_str().unwrap(), 999, 0, "new").await;
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("beyond file length"));
}

#[tokio::test]
async fn test_delete_count_too_large() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    fs::write(&path, "hello").unwrap();

    let result = call_update(path.to_str().unwrap(), 1, 99, "new").await;
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("exceeds file length"));
}

#[tokio::test]
async fn test_unicode() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    fs::write(&path, "你好世界").unwrap();

    let result = call_update(path.to_str().unwrap(), 1, 1, "再见世界")
        .await
        .unwrap();
    let output = parse_output(&result);
    assert_eq!(fs::read_to_string(&path).unwrap(), "再见世界");
    assert_eq!(output.replacements, 1);
}

#[tokio::test]
async fn test_append_at_end() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    fs::write(&path, "line1").unwrap();

    let result = call_update(path.to_str().unwrap(), 2, 0, "line2")
        .await
        .unwrap();
    let output = parse_output(&result);
    assert_eq!(fs::read_to_string(&path).unwrap(), "line1\nline2");
    assert_eq!(output.replacements, 0);
}

#[tokio::test]
async fn test_replace_with_trailing_newline() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    fs::write(&path, "hello\nworld\n").unwrap();

    let result = call_update(path.to_str().unwrap(), 2, 1, "earth")
        .await
        .unwrap();
    let output = parse_output(&result);
    assert_eq!(fs::read_to_string(&path).unwrap(), "hello\nearth\n");
    assert_eq!(output.replacements, 1);
}

#[tokio::test]
async fn test_insert_with_trailing_newline() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    fs::write(&path, "line2\nline3\n").unwrap();

    let result = call_update(path.to_str().unwrap(), 1, 0, "line1")
        .await
        .unwrap();
    let output = parse_output(&result);
    assert_eq!(fs::read_to_string(&path).unwrap(), "line1\nline2\nline3\n");
    assert_eq!(output.replacements, 0);
}

#[tokio::test]
async fn test_delete_with_trailing_newline() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    fs::write(&path, "delete_me\nkeep_me\n").unwrap();

    let result = call_update(path.to_str().unwrap(), 1, 1, "")
        .await
        .unwrap();
    let output = parse_output(&result);
    assert_eq!(fs::read_to_string(&path).unwrap(), "keep_me\n");
    assert_eq!(output.replacements, 1);
}

#[tokio::test]
async fn test_append_at_end_with_trailing_newline() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    fs::write(&path, "line1\n").unwrap();

    let result = call_update(path.to_str().unwrap(), 2, 0, "line2")
        .await
        .unwrap();
    let output = parse_output(&result);
    assert_eq!(fs::read_to_string(&path).unwrap(), "line1\nline2\n");
    assert_eq!(output.replacements, 0);
}

#[tokio::test]
async fn test_start_line_beyond_with_trailing_newline_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    fs::write(&path, "hello\nworld\n").unwrap();

    // file_read shows 2 lines, so start_line=4 (which is total_lines+2) should be rejected
    let result = call_update(path.to_str().unwrap(), 4, 0, "new").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_delete_all_lines_with_trailing_newline_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    fs::write(&path, "hello\nworld\n").unwrap();

    // file_read shows 2 lines, so start_line=1 + delete_count=3 = 3 > 2 should be rejected
    let result = call_update(path.to_str().unwrap(), 1, 3, "").await;
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("exceeds file length"));
}

#[tokio::test]
async fn test_build_line_diff() {
    use my_code_agent::tools::build_line_diff;

    let original = vec!["line1", "line2", "line3"];
    let diff = build_line_diff(2, 1, "modified", &original);
    assert!(diff.contains("-line2"));
    assert!(diff.contains("+modified"));
    assert!(diff.contains("@@ line 2 @@"));
}

#[tokio::test]
async fn test_build_diff() {
    use my_code_agent::tools::build_diff;

    let diff = build_diff("hello", "hi", "hello world");
    assert!(diff.contains("-hello"));
    assert!(diff.contains("+hi"));
}
