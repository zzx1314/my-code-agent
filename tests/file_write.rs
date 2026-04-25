use my_code_agent::tools::file_write::{FileWrite, FileWriteArgs, FileWriteError, FileWriteOutput};
use rig::tool::Tool;
use tempfile::TempDir;

async fn write_file(
    path: &str,
    content: &str,
    create_dirs: bool,
) -> Result<FileWriteOutput, FileWriteError> {
    FileWrite
        .call(FileWriteArgs {
            path: path.to_string(),
            content: content.to_string(),
            create_dirs,
        })
        .await
}

#[tokio::test]
async fn test_write_new_file() {
    let tmp = TempDir::new().unwrap();
    let file_path = tmp.path().join("new.txt");

    let output = write_file(file_path.to_str().unwrap(), "hello world", false)
        .await
        .unwrap();
    assert_eq!(output.bytes_written, 11);
    assert_eq!(output.path, file_path.to_str().unwrap());

    let content = std::fs::read_to_string(&file_path).unwrap();
    assert_eq!(content, "hello world");
}

#[tokio::test]
async fn test_overwrite_existing_file() {
    let tmp = TempDir::new().unwrap();
    let file_path = tmp.path().join("overwrite.txt");
    std::fs::write(&file_path, "old content").unwrap();

    let output = write_file(file_path.to_str().unwrap(), "new content", false)
        .await
        .unwrap();
    assert_eq!(output.bytes_written, 11);

    let content = std::fs::read_to_string(&file_path).unwrap();
    assert_eq!(content, "new content");
}

#[tokio::test]
async fn test_write_with_create_dirs() {
    let tmp = TempDir::new().unwrap();
    let file_path = tmp.path().join("nested/dir/file.txt");

    let output = write_file(file_path.to_str().unwrap(), "nested content", true)
        .await
        .unwrap();
    assert_eq!(output.bytes_written, 14);

    let content = std::fs::read_to_string(&file_path).unwrap();
    assert_eq!(content, "nested content");
}

#[tokio::test]
async fn test_write_without_create_dirs_fails() {
    let tmp = TempDir::new().unwrap();
    let file_path = tmp.path().join("nonexistent_dir/file.txt");

    let result = write_file(file_path.to_str().unwrap(), "content", false).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_write_empty_content() {
    let tmp = TempDir::new().unwrap();
    let file_path = tmp.path().join("empty.txt");

    let output = write_file(file_path.to_str().unwrap(), "", false)
        .await
        .unwrap();
    assert_eq!(output.bytes_written, 0);

    let content = std::fs::read_to_string(&file_path).unwrap();
    assert_eq!(content, "");
}

#[tokio::test]
async fn test_write_unicode_content() {
    let tmp = TempDir::new().unwrap();
    let file_path = tmp.path().join("unicode.txt");

    let unicode_content = "你好世界 🌍 日本語";
    let output = write_file(file_path.to_str().unwrap(), unicode_content, false)
        .await
        .unwrap();
    assert_eq!(output.bytes_written, unicode_content.len());

    let content = std::fs::read_to_string(&file_path).unwrap();
    assert_eq!(content, unicode_content);
}
