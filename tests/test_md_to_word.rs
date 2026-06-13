use std::fs;
use tempfile::TempDir;

/// Helper to check if pandoc is available
async fn is_pandoc_available() -> bool {
    tokio::process::Command::new("pandoc")
        .arg("--version")
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[tokio::test]
async fn test_md_to_word_basic_conversion() {
    if !is_pandoc_available().await {
        eprintln!("Skipping test: pandoc not installed");
        return;
    }

    let tmp_dir = TempDir::new().unwrap();
    let md_path = tmp_dir.path().join("test.md");
    let docx_path = tmp_dir.path().join("test.docx");

    // Create a simple markdown file
    fs::write(&md_path, "# Hello World\n\nThis is a test.").unwrap();

    // Run pandoc conversion
    let output = tokio::process::Command::new("pandoc")
        .arg(md_path.to_str().unwrap())
        .arg("-o")
        .arg(docx_path.to_str().unwrap())
        .arg("--to")
        .arg("docx")
        .output()
        .await
        .unwrap();

    assert!(output.status.success(), "Pandoc conversion failed");
    assert!(docx_path.exists(), "Output file was not created");
}

#[tokio::test]
async fn test_md_to_word_with_markdown_features() {
    if !is_pandoc_available().await {
        eprintln!("Skipping test: pandoc not installed");
        return;
    }

    let tmp_dir = TempDir::new().unwrap();
    let md_path = tmp_dir.path().join("features.md");
    let docx_path = tmp_dir.path().join("features.docx");

    // Create markdown with various features
    let content = r#"# Document Title

## Section 1

This is a paragraph with **bold** and *italic* text.

- Item 1
- Item 2
- Item 3

## Section 2

| Column 1 | Column 2 |
|----------|----------|
| Cell 1   | Cell 2   |

```rust
fn main() {
    println!("Hello, world!");
}
```
"#;

    fs::write(&md_path, content).unwrap();

    let output = tokio::process::Command::new("pandoc")
        .arg(md_path.to_str().unwrap())
        .arg("-o")
        .arg(docx_path.to_str().unwrap())
        .arg("--to")
        .arg("docx")
        .output()
        .await
        .unwrap();

    assert!(output.status.success(), "Pandoc conversion failed");
    assert!(docx_path.exists(), "Output file was not created");
}

#[tokio::test]
async fn test_md_to_word_input_not_found() {
    let result = tokio::process::Command::new("pandoc")
        .arg("nonexistent.md")
        .arg("-o")
        .arg("output.docx")
        .arg("--to")
        .arg("docx")
        .output()
        .await;

    // Pandoc should fail when input doesn't exist
    match result {
        Ok(output) => assert!(!output.status.success()),
        Err(_) => {} // Command itself failed, which is also acceptable
    }
}
