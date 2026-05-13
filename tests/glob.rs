use my_code_agent::tools::glob::{GlobArgs, GlobOutput, GlobSearch};
use my_code_agent::tools::Tool;
use std::fs;
use tempfile::TempDir;

async fn glob_search(
    pattern: &str,
    cwd: Option<&str>,
    max_results: usize,
) -> Result<String, String> {
    let args = serde_json::to_value(GlobArgs {
        pattern: pattern.to_string(),
        cwd: cwd.map(|s| s.to_string()),
        max_results,
    })
    .unwrap();
    GlobSearch.call(args).await
}

fn parse_output(result: &str) -> GlobOutput {
    serde_json::from_str(result).unwrap()
}

#[tokio::test]
async fn test_glob_find_rs_files() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("main.rs"), "fn main() {}").unwrap();
    fs::write(tmp.path().join("lib.rs"), "pub mod foo;").unwrap();
    fs::write(tmp.path().join("readme.md"), "# Hello").unwrap();

    let result = glob_search("**/*.rs", Some(tmp.path().to_str().unwrap()), 100)
        .await
        .unwrap();
    let output = parse_output(&result);

    assert_eq!(output.total_matches, 2);
    assert!(output.matches.iter().any(|m| m.ends_with("main.rs")));
    assert!(output.matches.iter().any(|m| m.ends_with("lib.rs")));
    assert!(!output.matches.iter().any(|m| m.ends_with(".md")));
    assert!(!output.truncated);
}

#[tokio::test]
async fn test_glob_nested_files() {
    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("src");
    fs::create_dir(&src).unwrap();
    fs::write(src.join("main.rs"), "").unwrap();
    let core = src.join("core");
    fs::create_dir(&core).unwrap();
    fs::write(core.join("mod.rs"), "").unwrap();

    let result = glob_search("**/*.rs", Some(tmp.path().to_str().unwrap()), 100)
        .await
        .unwrap();
    let output = parse_output(&result);
    assert_eq!(output.total_matches, 2);
}

#[tokio::test]
async fn test_glob_no_matches() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("main.rs"), "").unwrap();

    let result = glob_search("**/*.py", Some(tmp.path().to_str().unwrap()), 100)
        .await
        .unwrap();
    let output = parse_output(&result);
    assert_eq!(output.total_matches, 0);
    assert!(output.matches.is_empty());
}

#[tokio::test]
async fn test_glob_max_results_truncation() {
    let tmp = TempDir::new().unwrap();
    for i in 0..20 {
        fs::write(tmp.path().join(format!("file_{:02}.txt", i)), "").unwrap();
    }

    let result = glob_search("**/*.txt", Some(tmp.path().to_str().unwrap()), 5)
        .await
        .unwrap();
    let output = parse_output(&result);
    assert_eq!(output.matches.len(), 5);
    assert_eq!(output.total_matches, 20);
    assert!(output.truncated);
}

#[tokio::test]
async fn test_glob_single_star() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("Cargo.toml"), "").unwrap();
    fs::write(tmp.path().join("Cargo.lock"), "").unwrap();
    let src = tmp.path().join("src");
    fs::create_dir(&src).unwrap();
    fs::write(src.join("main.rs"), "").unwrap();

    let result = glob_search("*.toml", Some(tmp.path().to_str().unwrap()), 100)
        .await
        .unwrap();
    let output = parse_output(&result);
    assert_eq!(output.total_matches, 1);
    assert!(output.matches[0].ends_with("Cargo.toml"));
}

#[tokio::test]
async fn test_glob_double_star_recursive() {
    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("src");
    fs::create_dir(&src).unwrap();
    fs::write(src.join("main.rs"), "").unwrap();

    let result = glob_search("**/*.rs", Some(tmp.path().to_str().unwrap()), 100)
        .await
        .unwrap();
    let output = parse_output(&result);
    assert_eq!(output.total_matches, 1);
    assert!(output.matches[0].contains("src/main.rs"));
}

#[tokio::test]
async fn test_glob_relative_paths() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("main.rs"), "").unwrap();

    let result = glob_search("**/*.rs", Some(tmp.path().to_str().unwrap()), 100)
        .await
        .unwrap();
    let output = parse_output(&result);
    assert!(!output.matches[0].starts_with('/'));
}

#[tokio::test]
async fn test_glob_question_mark() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("f1.rs"), "").unwrap();
    fs::write(tmp.path().join("f2.rs"), "").unwrap();
    fs::write(tmp.path().join("f10.rs"), "").unwrap();

    let result = glob_search("f?.rs", Some(tmp.path().to_str().unwrap()), 100)
        .await
        .unwrap();
    let output = parse_output(&result);
    assert_eq!(output.total_matches, 2);
}

#[tokio::test]
async fn test_glob_invalid_pattern() {
    let tmp = TempDir::new().unwrap();
    let result = glob_search("[invalid", Some(tmp.path().to_str().unwrap()), 100).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    println!("Actual error: {}", err);
    assert!(err.contains("Invalid glob pattern") || err.contains("Pattern") || err.contains("invalid"));
}
