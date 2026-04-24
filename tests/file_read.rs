use my_code_agent::tools::file_read::{FileRead, FileReadArgs, FileReadError, FileReadOutput};
use rig::tool::Tool;
use tempfile::TempDir;

async fn read_file(
    path: &str,
    offset: Option<usize>,
    limit: Option<usize>,
) -> Result<FileReadOutput, FileReadError> {
    FileRead
        .call(FileReadArgs {
            path: path.to_string(),
            offset,
            // Use a very large limit when not specified to preserve old test behavior
            limit: limit.or(Some(100_000)),
        })
        .await
}

#[tokio::test]
async fn test_read_entire_file() {
    let tmp = TempDir::new().unwrap();
    let file_path = tmp.path().join("hello.txt");
    std::fs::write(&file_path, "line1\nline2\nline3\n").unwrap();

    let output = read_file(file_path.to_str().unwrap(), None, None)
        .await
        .unwrap();
    assert_eq!(output.lines, 3);
    assert!(output.content.contains("line1"));
    assert!(output.content.contains("line2"));
    assert!(output.content.contains("line3"));
    assert_eq!(output.path, file_path.to_str().unwrap());
}

#[tokio::test]
async fn test_read_with_offset() {
    let tmp = TempDir::new().unwrap();
    let file_path = tmp.path().join("offset.txt");
    std::fs::write(&file_path, "line1\nline2\nline3\nline4\nline5\n").unwrap();

    let output = read_file(file_path.to_str().unwrap(), Some(2), None)
        .await
        .unwrap();
    assert_eq!(output.lines, 5); // total lines in file
    assert!(!output.content.contains("line1"));
    assert!(!output.content.contains("line2"));
    assert!(output.content.contains("line3"));
    assert!(output.content.contains("line4"));
    assert!(output.content.contains("line5"));
}

#[tokio::test]
async fn test_read_with_limit() {
    let tmp = TempDir::new().unwrap();
    let file_path = tmp.path().join("limit.txt");
    std::fs::write(&file_path, "line1\nline2\nline3\nline4\nline5\n").unwrap();

    let output = read_file(file_path.to_str().unwrap(), None, Some(2))
        .await
        .unwrap();
    assert_eq!(output.lines, 5); // total lines in file
    assert!(output.content.contains("line1"));
    assert!(output.content.contains("line2"));
    assert!(!output.content.contains("line3"));
}

#[tokio::test]
async fn test_read_with_offset_and_limit() {
    let tmp = TempDir::new().unwrap();
    let file_path = tmp.path().join("both.txt");
    std::fs::write(&file_path, "line1\nline2\nline3\nline4\nline5\n").unwrap();

    let output = read_file(file_path.to_str().unwrap(), Some(1), Some(2))
        .await
        .unwrap();
    assert_eq!(output.lines, 5);
    assert!(!output.content.contains("line1"));
    assert!(output.content.contains("line2"));
    assert!(output.content.contains("line3"));
    assert!(!output.content.contains("line4"));
}

#[tokio::test]
async fn test_read_nonexistent_file() {
    let result = read_file("/nonexistent/path/file.txt", None, None).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_read_empty_file() {
    let tmp = TempDir::new().unwrap();
    let file_path = tmp.path().join("empty.txt");
    std::fs::write(&file_path, "").unwrap();

    let output = read_file(file_path.to_str().unwrap(), None, None)
        .await
        .unwrap();
    assert_eq!(output.lines, 0);
    assert_eq!(output.content, "");
}

#[tokio::test]
async fn test_read_line_numbers() {
    let tmp = TempDir::new().unwrap();
    let file_path = tmp.path().join("numbers.txt");
    std::fs::write(&file_path, "alpha\nbeta\ngamma\n").unwrap();

    let output = read_file(file_path.to_str().unwrap(), None, None)
        .await
        .unwrap();
    // Lines should be numbered 1-indexed with formatting
    assert!(output.content.contains("1 | alpha"));
    assert!(output.content.contains("2 | beta"));
    assert!(output.content.contains("3 | gamma"));
}

#[tokio::test]
async fn test_offset_beyond_file_length() {
    let tmp = TempDir::new().unwrap();
    let file_path = tmp.path().join("short.txt");
    std::fs::write(&file_path, "only_line\n").unwrap();

    let output = read_file(file_path.to_str().unwrap(), Some(100), None)
        .await
        .unwrap();
    assert_eq!(output.lines, 1);
    assert_eq!(output.content, ""); // no lines selected
}

// ── Default limit (200 lines) and truncation tests ──

#[tokio::test]
async fn test_default_limit_200_lines() {
    let tmp = TempDir::new().unwrap();
    let file_path = tmp.path().join("large.txt");
    // Create a file with 300 lines — exceeds the 200-line default
    let content: String = (0..300).map(|i| format!("line {}\n", i)).collect();
    std::fs::write(&file_path, &content).unwrap();

    // Call with limit=None, which defaults to 200
    let output = FileRead
        .call(FileReadArgs {
            path: file_path.to_str().unwrap().to_string(),
            offset: None,
            limit: None, // uses DEFAULT_READ_LIMIT = 200
        })
        .await
        .unwrap();

    assert_eq!(output.lines, 300); // total lines in file
    assert!(output.truncated);
    assert!(output.content.contains("line 0"));
    assert!(output.content.contains("line 199"));
    assert!(!output.content.contains("line 200")); // beyond limit
    assert!(output.content.contains("showing 200 of 300 total lines"));
    assert!(output.content.contains("offset=200 to read more"));
}

#[tokio::test]
async fn test_explicit_limit_overrides_default() {
    let tmp = TempDir::new().unwrap();
    let file_path = tmp.path().join("mid.txt");
    let content: String = (0..50).map(|i| format!("line {}\n", i)).collect();
    std::fs::write(&file_path, &content).unwrap();

    // Explicitly request limit=10
    let output = FileRead
        .call(FileReadArgs {
            path: file_path.to_str().unwrap().to_string(),
            offset: None,
            limit: Some(10),
        })
        .await
        .unwrap();

    assert_eq!(output.lines, 50);
    assert!(output.truncated);
    assert!(output.content.contains("showing 10 of 50 total lines"));
}

#[tokio::test]
async fn test_small_file_not_truncated() {
    let tmp = TempDir::new().unwrap();
    let file_path = tmp.path().join("small.txt");
    std::fs::write(&file_path, "line1\nline2\nline3\n").unwrap();

    let output = FileRead
        .call(FileReadArgs {
            path: file_path.to_str().unwrap().to_string(),
            offset: None,
            limit: None, // defaults to 200, but file is only 3 lines
        })
        .await
        .unwrap();

    assert_eq!(output.lines, 3);
    assert!(!output.truncated);
    assert!(!output.content.contains("showing"));
}
