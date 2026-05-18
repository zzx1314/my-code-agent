use my_code_agent::core::config::Config;
use my_code_agent::tools::ToolRegistry;

#[test]
fn test_all_tools_count() {
    let config = Config::default();
    let tools = ToolRegistry::from_config(&config);
    assert_eq!(tools.len(), 19);
}

#[test]
fn test_all_tools_names() {
    let config = Config::default();
    let tools = ToolRegistry::from_config(&config);
    let names: Vec<String> = tools.iter().map(|t| t.name().to_string()).collect();
    assert!(names.contains(&"file_read".to_string()));
    assert!(names.contains(&"file_outline".to_string()));
    assert!(names.contains(&"file_write".to_string()));
    assert!(names.contains(&"file_update".to_string()));
    assert!(names.contains(&"apply_patch".to_string()));
    assert!(names.contains(&"file_delete".to_string()));
    assert!(names.contains(&"propose_str_replace".to_string()));
    assert!(names.contains(&"end_turn".to_string()));
    assert!(names.contains(&"shell_exec".to_string()));
    assert!(names.contains(&"code_search".to_string()));
    assert!(names.contains(&"code_review".to_string()));
    assert!(names.contains(&"list_dir".to_string()));
    assert!(names.contains(&"glob".to_string()));
    assert!(names.contains(&"git_status".to_string()));
    assert!(names.contains(&"git_diff".to_string()));
    assert!(names.contains(&"git_log".to_string()));
    assert!(names.contains(&"git_commit".to_string()));
    assert!(names.contains(&"file_undo".to_string()));
    assert!(names.contains(&"write_todos".to_string()));
}
