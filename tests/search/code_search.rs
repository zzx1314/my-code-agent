use my_code_agent::tools::code_search::{CodeSearch, CodeSearchArgs, CodeSearchOutput};
use my_code_agent::tools::Tool;
use tempfile::TempDir;

async fn search(
    pattern: &str,
    path: Option<&str>,
    file_type: Option<&str>,
    max_results: usize,
    case_insensitive: bool,
) -> Result<String, String> {
    let args = serde_json::to_value(CodeSearchArgs {
        pattern: pattern.to_string(),
        path: path.map(|s| s.to_string()),
        file_type: file_type.map(|s| s.to_string()),
        max_results,
        case_insensitive,
    })
    .unwrap();
    CodeSearch.call(args).await
}

fn parse_output(result: &str) -> CodeSearchOutput {
    serde_json::from_str(result).unwrap()
}

#[tokio::test]
async fn test_search_finds_pattern() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(
        tmp.path().join("code.rs"),
        "fn hello() {\n    println!(\"hello\");\n}\n",
    )
    .unwrap();

    let result = search("hello", Some(tmp.path().to_str().unwrap()), None, 50, false)
        .await
        .unwrap();
    let output = parse_output(&result);
    assert_eq!(output.pattern, "hello");
    assert!(output.total_matches > 0);
    assert!(output.matches[0].line.contains("hello"));
}

#[tokio::test]
async fn test_search_no_matches() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("code.rs"), "fn foo() {}\n").unwrap();

    let result = search(
        "xyz_nonexistent",
        Some(tmp.path().to_str().unwrap()),
        None,
        50,
        false,
    )
    .await
    .unwrap();
    let output = parse_output(&result);
    assert_eq!(output.total_matches, 0);
    assert!(output.matches.is_empty());
}

#[tokio::test]
async fn test_search_case_insensitive() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("code.rs"), "fn HelloWorld() {}\n").unwrap();

    let result_sensitive = search(
        "helloworld",
        Some(tmp.path().to_str().unwrap()),
        None,
        50,
        false,
    )
    .await
    .unwrap();
    let o1 = parse_output(&result_sensitive);
    assert_eq!(o1.total_matches, 0);

    let result_insensitive = search(
        "helloworld",
        Some(tmp.path().to_str().unwrap()),
        None,
        50,
        true,
    )
    .await
    .unwrap();
    let o2 = parse_output(&result_insensitive);
    assert!(o2.total_matches > 0);
}

#[tokio::test]
async fn test_search_file_type_filter() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("code.rs"), "fn my_func() {}\n").unwrap();
    std::fs::write(tmp.path().join("code.py"), "def my_func():\n    pass\n").unwrap();

    let result_rs = search(
        "my_func",
        Some(tmp.path().to_str().unwrap()),
        Some("rs"),
        50,
        false,
    )
    .await
    .unwrap();
    let o_rs = parse_output(&result_rs);
    assert!(o_rs.total_matches >= 1);
    assert!(o_rs.matches.iter().all(|m| m.file.ends_with(".rs")));

    let result_py = search(
        "my_func",
        Some(tmp.path().to_str().unwrap()),
        Some("py"),
        50,
        false,
    )
    .await
    .unwrap();
    let o_py = parse_output(&result_py);
    assert!(o_py.total_matches >= 1);
    assert!(o_py.matches.iter().all(|m| m.file.ends_with(".py")));
}

#[tokio::test]
async fn test_search_max_results() {
    let tmp = TempDir::new().unwrap();
    let content: String = (0..20).map(|i| format!("match_line_{}\n", i)).collect();
    std::fs::write(tmp.path().join("many.rs"), content).unwrap();

    let result = search(
        "match_line",
        Some(tmp.path().to_str().unwrap()),
        None,
        5,
        false,
    )
    .await
    .unwrap();
    let output = parse_output(&result);
    assert_eq!(output.matches.len(), 5);
}

#[tokio::test]
async fn test_search_match_structure() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(
        tmp.path().join("struct.rs"),
        "line one\nfn target() {}\nline three\n",
    )
    .unwrap();

    let result = search(
        "target",
        Some(tmp.path().to_str().unwrap()),
        None,
        50,
        false,
    )
    .await
    .unwrap();
    let output = parse_output(&result);
    assert_eq!(output.total_matches, 1);
    let m = &output.matches[0];
    assert!(m.file.contains("struct.rs"));
    assert_eq!(m.line_number, 2);
    assert!(m.line.contains("target"));
}
