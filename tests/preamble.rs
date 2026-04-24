// Regression guards for the agent preamble.
//
// These tests verify that key instructions are present in the preamble template.
// If a test fails after a wording change, DO NOT just update the test — verify
// that the intended instruction is still communicated to the model. These exist
// to prevent accidentally removing critical behavioral guidance.

use my_code_agent::preamble::PREAMBLE_TEMPLATE;

#[test]
fn test_preamble_mentions_file_read_truncation() {
    assert!(
        PREAMBLE_TEMPLATE.contains("file_read"),
        "Preamble should mention file_read tool"
    );
    assert!(
        PREAMBLE_TEMPLATE.contains("truncated") && PREAMBLE_TEMPLATE.contains("offset"),
        "Preamble should instruct the model to use offset when file_read is truncated"
    );
}

#[test]
fn test_preamble_mentions_filepath_truncation_continuation() {
    assert!(
        PREAMBLE_TEMPLATE.contains("@filepath") || PREAMBLE_TEMPLATE.contains("@path"),
        "Preamble should mention @filepath / @path user attachment syntax"
    );
    assert!(
        PREAMBLE_TEMPLATE.contains("file_read") && PREAMBLE_TEMPLATE.contains("suggested offset"),
        "Preamble should instruct the model to use file_read with the suggested offset when @filepath is truncated"
    );
}

#[test]
fn test_preamble_filepath_syntax_is_for_users_only() {
    assert!(
        PREAMBLE_TEMPLATE.contains("@path:N") || PREAMBLE_TEMPLATE.contains("@path:"),
        "Preamble should explain the @path:N offset syntax"
    );
    assert!(
        PREAMBLE_TEMPLATE.contains("for users only"),
        "Preamble should clarify that @path:N is for users only, not for the model"
    );
}

#[test]
fn test_preamble_read_fully_before_modifying() {
    assert!(
        PREAMBLE_TEMPLATE.contains("Read fully before modifying"),
        "Preamble should have the 'Read fully before modifying' rule"
    );
    assert!(
        PREAMBLE_TEMPLATE.contains("@filepath") && PREAMBLE_TEMPLATE.contains("truncation notice"),
        "Rule 7 should cover @filepath truncation notices in addition to file_read truncation"
    );
}

#[test]
fn test_preamble_has_all_tools() {
    let tools = [
        "file_read", "file_write", "file_update", "file_delete",
        "shell_exec", "code_search", "list_dir", "glob",
    ];
    for tool in &tools {
        assert!(
            PREAMBLE_TEMPLATE.contains(tool),
            "Preamble should mention the `{tool}` tool"
        );
    }
}

#[test]
fn test_preamble_has_knowledge_placeholder() {
    assert!(
        PREAMBLE_TEMPLATE.contains("{knowledge}"),
        "Preamble should have {{knowledge}} placeholder for project knowledge injection"
    );
}
