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
            limit,
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
