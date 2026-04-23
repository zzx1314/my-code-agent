use my_code_agent::tools::FileUpdate;
use rig::tool::Tool;
use std::fs;

async fn call_update(path: &str, old: &str, new: &str, allow_multiple: bool) -> Result<<FileUpdate as Tool>::Output, <FileUpdate as Tool>::Error> {
    FileUpdate
        .call(my_code_agent::tools::file_update::FileUpdateArgs {
            path: path.to_string(),
            old: old.to_string(),
            new: new.to_string(),
            allow_multiple,
        })
        .await
}

#[tokio::test]
async fn test_simple_replacement() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    fs::write(&path, "hello world").unwrap();

    let result = call_update(path.to_str().unwrap(), "hello", "hi", false)
        .await
        .unwrap();

    assert_eq!(result.replacements, 1);
    assert_eq!(fs::read_to_string(&path).unwrap(), "hi world");
}

#[tokio::test]
async fn test_multiline_replacement() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.rs");
    fs::write(&path, "fn foo() {\n    let x = 1;\n}\n").unwrap();

    let old = "    let x = 1;";
    let new = "    let x = 2;";
    let result = call_update(path.to_str().unwrap(), old, new, false)
        .await
        .unwrap();

    assert_eq!(result.replacements, 1);
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "fn foo() {\n    let x = 2;\n}\n"
    );
}

#[tokio::test]
async fn test_delete_text() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    fs::write(&path, "hello world").unwrap();

    let result = call_update(path.to_str().unwrap(), " world", "", false)
        .await
        .unwrap();

    assert_eq!(result.replacements, 1);
    assert_eq!(fs::read_to_string(&path).unwrap(), "hello");
}

#[tokio::test]
async fn test_not_found_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    fs::write(&path, "hello world").unwrap();

    let err = call_update(path.to_str().unwrap(), "goodbye", "hi", false)
        .await
        .unwrap_err();

    let msg = err.to_string();
    assert!(msg.contains("not found"));
}

#[tokio::test]
async fn test_multiple_matches_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    fs::write(&path, "ha ha ha").unwrap();

    let err = call_update(path.to_str().unwrap(), "ha", "he", false)
        .await
        .unwrap_err();

    let msg = err.to_string();
    assert!(msg.contains("multiple"));
    assert!(msg.contains("3"));
}

#[tokio::test]
async fn test_allow_multiple() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    fs::write(&path, "ha ha ha").unwrap();

    let result = call_update(path.to_str().unwrap(), "ha", "he", true)
        .await
        .unwrap();

    assert_eq!(result.replacements, 3);
    assert_eq!(fs::read_to_string(&path).unwrap(), "he he he");
}

#[tokio::test]
async fn test_diff_output() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    fs::write(&path, "line1\nline2\nline3\n").unwrap();

    let result = call_update(path.to_str().unwrap(), "line2", "LINE2", false)
        .await
        .unwrap();

    assert!(result.diff.contains("@@ line 2 @@"));
    assert!(result.diff.contains("-line2"));
    assert!(result.diff.contains("+LINE2"));
}

#[tokio::test]
async fn test_nonexistent_file() {
    let err = call_update("/nonexistent/file.txt", "old", "new", false)
        .await
        .unwrap_err();

    let msg = err.to_string();
    assert!(msg.contains("IO error") || msg.contains("No such file"));
}
