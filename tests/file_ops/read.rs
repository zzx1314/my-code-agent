use my_code_agent::core::config::Config;
use my_code_agent::tools::file_read::{FileRead, FileReadArgs, FileReadOutput};
use my_code_agent::tools::Tool;
use tempfile::TempDir;

fn make_reader() -> FileRead {
    let config = Config::default();
    FileRead::from_config(&config)
}

async fn read_file(
    path: &str,
    offset: Option<usize>,
    limit: Option<usize>,
) -> Result<String, String> {
    let args = serde_json::to_value(FileReadArgs {
        path: path.to_string(),
        offset,
        limit: limit.or(Some(100_000)),
    })
    .unwrap();
    make_reader().call(args).await
}

fn parse_output(result: &str) -> FileReadOutput {
    serde_json::from_str(result).unwrap()
}

#[tokio::test]
async fn test_read_entire_file() {
    let tmp = TempDir::new().unwrap();
    let file_path = tmp.path().join("hello.txt");
    std::fs::write(&file_path, "line1\nline2\nline3\n").unwrap();

    let result = read_file(file_path.to_str().unwrap(), None, None)
        .await
        .unwrap();
    let output = parse_output(&result);
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

    let result = read_file(file_path.to_str().unwrap(), Some(2), None)
        .await
        .unwrap();
    let output = parse_output(&result);
    assert_eq!(output.lines, 5);
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

    let result = read_file(file_path.to_str().unwrap(), None, Some(2))
        .await
        .unwrap();
    let output = parse_output(&result);
    assert_eq!(output.lines, 5);
    assert!(output.content.contains("line1"));
    assert!(output.content.contains("line2"));
    assert!(!output.content.contains("line3"));
}

#[tokio::test]
async fn test_read_with_offset_and_limit() {
    let tmp = TempDir::new().unwrap();
    let file_path = tmp.path().join("both.txt");
    std::fs::write(&file_path, "line1\nline2\nline3\nline4\nline5\n").unwrap();

    let result = read_file(file_path.to_str().unwrap(), Some(1), Some(2))
        .await
        .unwrap();
    let output = parse_output(&result);
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

    let result = read_file(file_path.to_str().unwrap(), None, None)
        .await
        .unwrap();
    let output = parse_output(&result);
    assert_eq!(output.lines, 0);
    assert_eq!(output.content, "");
}

#[tokio::test]
async fn test_read_line_numbers() {
    let tmp = TempDir::new().unwrap();
    let file_path = tmp.path().join("numbers.txt");
    std::fs::write(&file_path, "alpha\nbeta\ngamma\n").unwrap();

    let result = read_file(file_path.to_str().unwrap(), None, None)
        .await
        .unwrap();
    let output = parse_output(&result);
    assert!(output.content.contains("1 | alpha"));
    assert!(output.content.contains("2 | beta"));
    assert!(output.content.contains("3 | gamma"));
}

#[tokio::test]
async fn test_offset_beyond_file_length() {
    let tmp = TempDir::new().unwrap();
    let file_path = tmp.path().join("short.txt");
    std::fs::write(&file_path, "only_line\n").unwrap();

    let result = read_file(file_path.to_str().unwrap(), Some(100), None)
        .await
        .unwrap();
    let output = parse_output(&result);
    assert_eq!(output.lines, 1);
    assert_eq!(output.content, "");
}

#[tokio::test]
async fn test_default_limit_200_lines() {
    let tmp = TempDir::new().unwrap();
    let file_path = tmp.path().join("large.txt");
    let content: String = (0..300).map(|i| format!("line {}\n", i)).collect();
    std::fs::write(&file_path, &content).unwrap();

    let args = serde_json::to_value(FileReadArgs {
        path: file_path.to_str().unwrap().to_string(),
        offset: None,
        limit: None,
    })
    .unwrap();
    let result = make_reader().call(args).await.unwrap();
    let output = parse_output(&result);

    assert_eq!(output.lines, 300);
    assert!(output.truncated);
    assert!(output.content.contains("line 0"));
    assert!(output.content.contains("line 199"));
    assert!(!output.content.contains("line 200"));
    assert!(output.content.contains("showing 200 of 300 total lines"));
    assert!(output.content.contains("offset=200 to read more"));
}

#[tokio::test]
async fn test_explicit_limit_overrides_default() {
    let tmp = TempDir::new().unwrap();
    let file_path = tmp.path().join("mid.txt");
    let content: String = (0..50).map(|i| format!("line {}\n", i)).collect();
    std::fs::write(&file_path, &content).unwrap();

    let args = serde_json::to_value(FileReadArgs {
        path: file_path.to_str().unwrap().to_string(),
        offset: None,
        limit: Some(10),
    })
    .unwrap();
    let result = make_reader().call(args).await.unwrap();
    let output = parse_output(&result);

    assert_eq!(output.lines, 50);
    assert!(output.truncated);
    assert!(output.content.contains("showing 10 of 50 total lines"));
}

#[tokio::test]
async fn test_small_file_not_truncated() {
    let tmp = TempDir::new().unwrap();
    let file_path = tmp.path().join("small.txt");
    std::fs::write(&file_path, "line1\nline2\nline3\n").unwrap();

    let args = serde_json::to_value(FileReadArgs {
        path: file_path.to_str().unwrap().to_string(),
        offset: None,
        limit: None,
    })
    .unwrap();
    let result = make_reader().call(args).await.unwrap();
    let output = parse_output(&result);

    assert_eq!(output.lines, 3);
    assert!(!output.truncated);
    assert!(!output.content.contains("showing"));
}
