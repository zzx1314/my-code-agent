use my_code_agent::tools::FileDelete;
use rig::tool::Tool;
use std::fs;

async fn call_delete(path: &str, recursive: bool) -> Result<<FileDelete as Tool>::Output, <FileDelete as Tool>::Error> {
    FileDelete
        .call(my_code_agent::tools::file_delete::FileDeleteArgs {
            path: path.to_string(),
            recursive,
            snippet: None,
            allow_multiple: false,
            auto_approve: true, // skip confirmation prompts in tests
        })
        .await
}

async fn call_delete_snippet(
    path: &str,
    snippet: &str,
    allow_multiple: bool,
) -> Result<<FileDelete as Tool>::Output, <FileDelete as Tool>::Error> {
    FileDelete
        .call(my_code_agent::tools::file_delete::FileDeleteArgs {
            path: path.to_string(),
            recursive: false,
            snippet: Some(snippet.to_string()),
            allow_multiple,
            auto_approve: true, // skip confirmation prompts in tests
        })
        .await
}

// ── Whole file/directory deletion tests ──

#[tokio::test]
async fn test_delete_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    fs::write(&path, "hello").unwrap();

    let result = call_delete(path.to_str().unwrap(), false)
        .await
        .unwrap();

    assert_eq!(result.deleted_type, "file");
    assert!(!path.exists());
}

#[tokio::test]
async fn test_delete_empty_directory() {
    let dir = tempfile::tempdir().unwrap();
    let subdir = dir.path().join("empty_dir");
    fs::create_dir(&subdir).unwrap();

    let result = call_delete(subdir.to_str().unwrap(), false)
        .await
        .unwrap();

    assert_eq!(result.deleted_type, "directory");
    assert!(!subdir.exists());
}

#[tokio::test]
async fn test_delete_directory_recursive() {
    let dir = tempfile::tempdir().unwrap();
    let subdir = dir.path().join("subdir");
    fs::create_dir(&subdir).unwrap();
    fs::write(subdir.join("file1.txt"), "content1").unwrap();
    fs::create_dir(subdir.join("nested")).unwrap();
    fs::write(subdir.join("nested/file2.txt"), "content2").unwrap();

    let result = call_delete(subdir.to_str().unwrap(), true)
        .await
        .unwrap();

    assert_eq!(result.deleted_type, "directory");
    assert!(!subdir.exists());
}

#[tokio::test]
async fn test_delete_non_empty_directory_without_recursive_fails() {
    let dir = tempfile::tempdir().unwrap();
    let subdir = dir.path().join("notempty");
    fs::create_dir(&subdir).unwrap();
    fs::write(subdir.join("file.txt"), "content").unwrap();

    let err = call_delete(subdir.to_str().unwrap(), false)
        .await
        .unwrap_err();

    // Should get an IO error (directory not empty)
    assert!(err.to_string().contains("IO error"));
    // Directory should still exist
    assert!(subdir.exists());
}

#[tokio::test]
async fn test_delete_nonexistent_path() {
    let err = call_delete("/nonexistent/path/file.txt", false)
        .await
        .unwrap_err();

    let msg = err.to_string();
    assert!(msg.contains("not found"));
}

#[tokio::test]
async fn test_delete_returns_path() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("to_delete.txt");
    fs::write(&path, "bye").unwrap();

    let path_str = path.to_str().unwrap().to_string();
    let result = call_delete(&path_str, false).await.unwrap();

    assert_eq!(result.path, path_str);
}

// ── Snippet deletion tests ──

#[tokio::test]
async fn test_delete_snippet_single_line() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("code.rs");
    fs::write(&path, "fn foo() {\n    println!(\"hello\");\n}\n").unwrap();

    let result = call_delete_snippet(path.to_str().unwrap(), "    println!(\"hello\");\n", false)
        .await
        .unwrap();

    assert_eq!(result.deleted_type, "snippet");
    assert_eq!(result.deletions, Some(1));
    assert!(result.diff.is_some());

    let new_content = fs::read_to_string(&path).unwrap();
    assert_eq!(new_content, "fn foo() {\n}\n");
}

#[tokio::test]
async fn test_delete_snippet_multiline() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("code.rs");
    fs::write(&path, "fn foo() {\n    let x = 1;\n    let y = 2;\n    x + y\n}\n").unwrap();

    let snippet = "    let x = 1;\n    let y = 2;\n    x + y\n";
    let result = call_delete_snippet(path.to_str().unwrap(), snippet, false)
        .await
        .unwrap();

    assert_eq!(result.deleted_type, "snippet");
    assert_eq!(result.deletions, Some(1));

    let new_content = fs::read_to_string(&path).unwrap();
    assert_eq!(new_content, "fn foo() {\n}\n");
}

#[tokio::test]
async fn test_delete_snippet_multiple_with_allow_multiple() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("code.rs");
    fs::write(&path, "let x = 1;\nlet y = 2;\nlet x = 1;\n").unwrap();

    let result = call_delete_snippet(path.to_str().unwrap(), "let x = 1;\n", true)
        .await
        .unwrap();

    assert_eq!(result.deleted_type, "snippet");
    assert_eq!(result.deletions, Some(2));

    let new_content = fs::read_to_string(&path).unwrap();
    assert_eq!(new_content, "let y = 2;\n");
}

#[tokio::test]
async fn test_delete_snippet_multiple_without_allow_multiple_fails() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("code.rs");
    fs::write(&path, "let x = 1;\nlet y = 2;\nlet x = 1;\n").unwrap();

    let err = call_delete_snippet(path.to_str().unwrap(), "let x = 1;\n", false)
        .await
        .unwrap_err();

    let msg = err.to_string();
    assert!(msg.contains("multiple times"));
    assert!(msg.contains("allow_multiple"));

    // File should be unchanged
    let content = fs::read_to_string(&path).unwrap();
    assert_eq!(content, "let x = 1;\nlet y = 2;\nlet x = 1;\n");
}

#[tokio::test]
async fn test_delete_snippet_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("code.rs");
    fs::write(&path, "fn foo() {}\n").unwrap();

    let err = call_delete_snippet(path.to_str().unwrap(), "nonexistent code", false)
        .await
        .unwrap_err();

    let msg = err.to_string();
    assert!(msg.contains("Snippet not found"));

    // File should be unchanged
    let content = fs::read_to_string(&path).unwrap();
    assert_eq!(content, "fn foo() {}\n");
}

#[tokio::test]
async fn test_delete_snippet_empty_fails() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("code.rs");
    fs::write(&path, "fn foo() {}\n").unwrap();

    let err = call_delete_snippet(path.to_str().unwrap(), "", false)
        .await
        .unwrap_err();

    let msg = err.to_string();
    assert!(msg.contains("Snippet not found"));
}

#[tokio::test]
async fn test_delete_snippet_on_directory_fails() {
    let dir = tempfile::tempdir().unwrap();
    let subdir = dir.path().join("mydir");
    fs::create_dir(&subdir).unwrap();

    let err = call_delete_snippet(subdir.to_str().unwrap(), "some text", false)
        .await
        .unwrap_err();

    let msg = err.to_string();
    assert!(msg.contains("Cannot use snippet mode on a directory"));
}

#[tokio::test]
async fn test_delete_snippet_nonexistent_file() {
    let err = call_delete_snippet("/nonexistent/path/file.txt", "some text", false)
        .await
        .unwrap_err();

    let msg = err.to_string();
    assert!(msg.contains("not found"));
}

#[tokio::test]
async fn test_delete_snippet_returns_diff() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("code.rs");
    fs::write(&path, "fn hello() {\n    println!(\"hi\");\n}\n").unwrap();

    let result = call_delete_snippet(path.to_str().unwrap(), "println!(\"hi\");", false)
        .await
        .unwrap();

    let diff = result.diff.unwrap();
    assert!(diff.contains("@@ line"));
    assert!(diff.contains("-println!(\"hi\");"));
}
