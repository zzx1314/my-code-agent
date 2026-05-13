use my_code_agent::tools::list_dir::{ListDir, ListDirArgs, ListDirOutput};
use my_code_agent::tools::Tool;
use std::fs;
use tempfile::TempDir;

async fn list_dir(path: &str, max_depth: usize) -> Result<String, String> {
    let args = serde_json::to_value(ListDirArgs {
        path: path.to_string(),
        max_depth,
    })
    .unwrap();
    ListDir.call(args).await
}

fn parse_output(result: &str) -> ListDirOutput {
    serde_json::from_str(result).unwrap()
}

#[tokio::test]
async fn test_list_dir_flat() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("file1.txt"), "hello").unwrap();
    fs::write(tmp.path().join("file2.rs"), "fn main() {}").unwrap();
    fs::create_dir(tmp.path().join("subdir")).unwrap();

    let result = list_dir(tmp.path().to_str().unwrap(), 1).await.unwrap();
    let output = parse_output(&result);

    assert_eq!(output.total_files, 2);
    assert_eq!(output.total_dirs, 1);
    assert_eq!(output.entries.len(), 3);
    assert_eq!(output.entries[0].entry_type, "directory");
    assert_eq!(output.entries[0].name, "subdir");
    assert!(output.entries[0].children.is_none());
}

#[tokio::test]
async fn test_list_dir_with_depth() {
    let tmp = TempDir::new().unwrap();
    let subdir = tmp.path().join("src");
    fs::create_dir(&subdir).unwrap();
    fs::write(subdir.join("main.rs"), "fn main() {}").unwrap();
    fs::write(subdir.join("lib.rs"), "pub mod foo;").unwrap();

    let result = list_dir(tmp.path().to_str().unwrap(), 2).await.unwrap();
    let output = parse_output(&result);

    assert_eq!(output.total_files, 2);
    assert_eq!(output.total_dirs, 1);
    let src_entry = &output.entries[0];
    assert_eq!(src_entry.name, "src");
    assert!(src_entry.children.is_some());
    let children = src_entry.children.as_ref().unwrap();
    assert_eq!(children.len(), 2);
    assert!(children.iter().all(|c| c.entry_type == "file"));
}

#[tokio::test]
async fn test_list_dir_not_found() {
    let err = list_dir("/nonexistent/path", 1).await.unwrap_err();
    assert!(err.contains("not found"));
}

#[tokio::test]
async fn test_list_dir_not_a_directory() {
    let tmp = TempDir::new().unwrap();
    let file_path = tmp.path().join("file.txt");
    fs::write(&file_path, "hello").unwrap();

    let err = list_dir(file_path.to_str().unwrap(), 1).await.unwrap_err();
    assert!(err.contains("not a directory"));
}

#[tokio::test]
async fn test_list_dir_empty_directory() {
    let tmp = TempDir::new().unwrap();
    let empty = tmp.path().join("empty");
    fs::create_dir(&empty).unwrap();

    let result = list_dir(empty.to_str().unwrap(), 1).await.unwrap();
    let output = parse_output(&result);

    assert_eq!(output.total_files, 0);
    assert_eq!(output.total_dirs, 0);
    assert!(output.entries.is_empty());
}

#[tokio::test]
async fn test_list_dir_sorted_directories_first() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("a_file.txt"), "").unwrap();
    fs::create_dir(tmp.path().join("z_dir")).unwrap();
    fs::write(tmp.path().join("m_file.rs"), "").unwrap();
    fs::create_dir(tmp.path().join("a_dir")).unwrap();

    let result = list_dir(tmp.path().to_str().unwrap(), 1).await.unwrap();
    let output = parse_output(&result);

    assert_eq!(output.entries[0].name, "a_dir");
    assert_eq!(output.entries[1].name, "z_dir");
    assert_eq!(output.entries[2].name, "a_file.txt");
    assert_eq!(output.entries[3].name, "m_file.rs");
}

#[tokio::test]
async fn test_list_dir_deep_nesting() {
    let tmp = TempDir::new().unwrap();
    let l1 = tmp.path().join("l1");
    let l2 = l1.join("l2");
    let l3 = l2.join("l3");
    fs::create_dir_all(&l3).unwrap();
    fs::write(l3.join("deep.txt"), "found me").unwrap();

    let output1 = list_dir(tmp.path().to_str().unwrap(), 1).await.unwrap();
    let o1 = parse_output(&output1);
    assert_eq!(o1.total_dirs, 1);
    let l1_entry = &o1.entries[0];
    assert!(l1_entry.children.is_none());

    let output3 = list_dir(tmp.path().to_str().unwrap(), 3).await.unwrap();
    let o3 = parse_output(&output3);
    assert_eq!(o3.total_dirs, 3);
    assert_eq!(o3.total_files, 0);

    let output4 = list_dir(tmp.path().to_str().unwrap(), 4).await.unwrap();
    let o4 = parse_output(&output4);
    assert_eq!(o4.total_dirs, 3);
    assert_eq!(o4.total_files, 1);
}
