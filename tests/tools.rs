use my_deepseek_agent::tools::all_tools;

#[test]
fn test_all_tools_count() {
    let tools = all_tools();
    assert_eq!(tools.len(), 4);
}

#[test]
fn test_all_tools_names() {
    let tools = all_tools();
    let names: Vec<String> = tools.iter().map(|t| t.name()).collect();
    assert!(names.contains(&"file_read".to_string()));
    assert!(names.contains(&"file_write".to_string()));
    assert!(names.contains(&"shell_exec".to_string()));
    assert!(names.contains(&"code_search".to_string()));
}
