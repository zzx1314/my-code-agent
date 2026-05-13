use my_code_agent::tools::FileUpdate;
use my_code_agent::tools::Tool;
use std::fs;

async fn call_update(
    path: &str,
    old: &str,
    new: &str,
    allow_multiple: bool,
) -> Result<String, String> {
    let args = serde_json::to_value(my_code_agent::tools::file_update::FileUpdateArgs {
        path: path.to_string(),
        old: old.to_string(),
        new: new.to_string(),
        allow_multiple,
    })
    .unwrap();
    FileUpdate.call(args).await
}

fn parse_output(result: &str) -> my_code_agent::tools::file_update::FileUpdateOutput {
    serde_json::from_str(result).unwrap()
}

#[tokio::test]
async fn test_simple_replacement() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    fs::write(&path, "hello world").unwrap();

    let result = call_update(path.to_str().unwrap(), "hello", "hi", false)
        .await
        .unwrap();
    let output = parse_output(&result);
    assert_eq!(fs::read_to_string(&path).unwrap(), "hi world");
    assert_eq!(output.replacements, 1);
}

#[tokio::test]
async fn test_multiple_replacement() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    fs::write(&path, "hello hello hello").unwrap();

    let result = call_update(path.to_str().unwrap(), "hello", "hi", true)
        .await
        .unwrap();
    let output = parse_output(&result);
    assert_eq!(fs::read_to_string(&path).unwrap(), "hi hi hi");
    assert_eq!(output.replacements, 3);
}

#[tokio::test]
async fn test_string_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    fs::write(&path, "hello world").unwrap();

    let result = call_update(path.to_str().unwrap(), "xyz", "abc", false).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_multiple_matches_error_without_flag() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    fs::write(&path, "hello hello world").unwrap();

    let result = call_update(path.to_str().unwrap(), "hello", "hi", false).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_unicode_replacement() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    fs::write(&path, "你好世界").unwrap();

    let result = call_update(path.to_str().unwrap(), "你好", "再见", false)
        .await
        .unwrap();
    let output = parse_output(&result);
    assert_eq!(fs::read_to_string(&path).unwrap(), "再见世界");
    assert_eq!(output.replacements, 1);
}

#[tokio::test]
async fn test_build_diff() {
    use my_code_agent::tools::build_diff;

    let diff = build_diff("hello", "hi", "hello world");
    assert!(diff.contains("-hello"));
    assert!(diff.contains("+hi"));
}
