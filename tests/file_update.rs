use my_code_agent::tools::FileUpdate;
use my_code_agent::tools::Tool;
use std::fs;

async fn call_update(
    path: &str,
    start_line: usize,
    delete_count: usize,
    new_content: &str,
) -> Result<String, String> {
    let args = serde_json::to_value(my_code_agent::tools::file_update::FileUpdateArgs {
        path: path.to_string(),
        start_line,
        delete_count,
        new_content: new_content.to_string(),
    })
    .unwrap();
    FileUpdate.call(args).await
}

fn parse_output(result: &str) -> my_code_agent::tools::file_update::FileUpdateOutput {
    serde_json::from_str(result).unwrap()
}

#[tokio::test]
async fn test_replace_single_line() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    fs::write(&path, "hello world").unwrap();

    let result = call_update(path.to_str().unwrap(), 1, 1, "hi world")
        .await
        .unwrap();
    let output = parse_output(&result);
    assert_eq!(fs::read_to_string(&path).unwrap(), "hi world");
    assert_eq!(output.replacements, 1);
}

#[tokio::test]
async fn test_replace_multiple_lines() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    fs::write(&path, "line1\nline2\nline3").unwrap();

    let result = call_update(path.to_str().unwrap(), 1, 3, "new1\nnew2\nnew3")
        .await
        .unwrap();
    let output = parse_output(&result);
    assert_eq!(fs::read_to_string(&path).unwrap(), "new1\nnew2\nnew3");
    assert_eq!(output.replacements, 3);
}

#[tokio::test]
async fn test_insert_lines() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    fs::write(&path, "line2\nline3").unwrap();

    let result = call_update(path.to_str().unwrap(), 1, 0, "line1")
        .await
        .unwrap();
    let output = parse_output(&result);
    assert_eq!(fs::read_to_string(&path).unwrap(), "line1\nline2\nline3");
    assert_eq!(output.replacements, 0);
}

#[tokio::test]
async fn test_delete_lines() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    fs::write(&path, "delete_me\nkeep_me").unwrap();

    let result = call_update(path.to_str().unwrap(), 1, 1, "")
        .await
        .unwrap();
    let output = parse_output(&result);
    assert_eq!(fs::read_to_string(&path).unwrap(), "keep_me");
    assert_eq!(output.replacements, 1);
}

#[tokio::test]
async fn test_start_line_out_of_range() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    fs::write(&path, "hello world").unwrap();

    let result = call_update(path.to_str().unwrap(), 999, 0, "new").await;
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("beyond file length"));
}

#[tokio::test]
async fn test_delete_count_too_large() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    fs::write(&path, "hello").unwrap();

    let result = call_update(path.to_str().unwrap(), 1, 99, "new").await;
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("exceeds file length"));
}

#[tokio::test]
async fn test_unicode() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    fs::write(&path, "你好世界").unwrap();

    let result = call_update(path.to_str().unwrap(), 1, 1, "再见世界")
        .await
        .unwrap();
    let output = parse_output(&result);
    assert_eq!(fs::read_to_string(&path).unwrap(), "再见世界");
    assert_eq!(output.replacements, 1);
}

#[tokio::test]
async fn test_append_at_end() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    fs::write(&path, "line1").unwrap();

    let result = call_update(path.to_str().unwrap(), 2, 0, "line2")
        .await
        .unwrap();
    let output = parse_output(&result);
    assert_eq!(fs::read_to_string(&path).unwrap(), "line1\nline2");
    assert_eq!(output.replacements, 0);
}

#[tokio::test]
async fn test_replace_with_trailing_newline() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    fs::write(&path, "hello\nworld\n").unwrap();

    let result = call_update(path.to_str().unwrap(), 2, 1, "earth")
        .await
        .unwrap();
    let output = parse_output(&result);
    assert_eq!(fs::read_to_string(&path).unwrap(), "hello\nearth\n");
    assert_eq!(output.replacements, 1);
}

#[tokio::test]
async fn test_insert_with_trailing_newline() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    fs::write(&path, "line2\nline3\n").unwrap();

    let result = call_update(path.to_str().unwrap(), 1, 0, "line1")
        .await
        .unwrap();
    let output = parse_output(&result);
    assert_eq!(fs::read_to_string(&path).unwrap(), "line1\nline2\nline3\n");
    assert_eq!(output.replacements, 0);
}

#[tokio::test]
async fn test_delete_with_trailing_newline() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    fs::write(&path, "delete_me\nkeep_me\n").unwrap();

    let result = call_update(path.to_str().unwrap(), 1, 1, "")
        .await
        .unwrap();
    let output = parse_output(&result);
    assert_eq!(fs::read_to_string(&path).unwrap(), "keep_me\n");
    assert_eq!(output.replacements, 1);
}

#[tokio::test]
async fn test_append_at_end_with_trailing_newline() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    fs::write(&path, "line1\n").unwrap();

    let result = call_update(path.to_str().unwrap(), 2, 0, "line2")
        .await
        .unwrap();
    let output = parse_output(&result);
    assert_eq!(fs::read_to_string(&path).unwrap(), "line1\nline2\n");
    assert_eq!(output.replacements, 0);
}

#[tokio::test]
async fn test_start_line_beyond_with_trailing_newline_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    fs::write(&path, "hello\nworld\n").unwrap();

    // file_read shows 2 lines, so start_line=4 (which is total_lines+2) should be rejected
    let result = call_update(path.to_str().unwrap(), 4, 0, "new").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_delete_all_lines_with_trailing_newline_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    fs::write(&path, "hello\nworld\n").unwrap();

    // file_read shows 2 lines, so start_line=1 + delete_count=3 = 3 > 2 should be rejected
    let result = call_update(path.to_str().unwrap(), 1, 3, "").await;
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("exceeds file length"));
}

#[tokio::test]
async fn test_build_line_diff() {
    use my_code_agent::tools::build_line_diff;

    let original = vec!["line1", "line2", "line3"];
    let diff = build_line_diff(2, 1, "modified", &original);
    assert!(diff.contains("-line2"));
    assert!(diff.contains("+modified"));
    assert!(diff.contains("@@ line 2 @@"));
}

#[tokio::test]
async fn test_build_diff() {
    use my_code_agent::tools::build_diff;

    let diff = build_diff("hello", "hi", "hello world");
    assert!(diff.contains("-hello"));
    assert!(diff.contains("+hi"));
}

// ── Tests for the trailing-newline-in-new_content bug ──

#[tokio::test]
async fn test_new_content_with_trailing_newline() {
    // When new_content ends with \n (common when LLM generates code blocks),
    // split('\n') should NOT produce an extra empty line.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    fs::write(&path, "line1\nline2\nline3").unwrap();

    // new_content with trailing \n — simulates what LLMs often send
    let result = call_update(path.to_str().unwrap(), 2, 1, "replacement\n")
        .await
        .unwrap();
    let output = parse_output(&result);
    assert_eq!(fs::read_to_string(&path).unwrap(), "line1\nreplacement\nline3");
    assert_eq!(output.replacements, 1);
}

#[tokio::test]
async fn test_new_content_with_brackets_and_trailing_newline() {
    // This simulates replacing a Rust function body — brackets {} and trailing newline
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    fs::write(
        &path,
        "fn old() {\n    old_stuff();\n}",
    )
    .unwrap();

    // new_content with brackets and trailing \n
    let new_content = "fn new() {\n    new_stuff();\n}\n";
    let result = call_update(path.to_str().unwrap(), 1, 3, new_content)
        .await
        .unwrap();
    let output = parse_output(&result);
    // Should be exactly the replacement without extra blank lines
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "fn new() {\n    new_stuff();\n}"
    );
    assert_eq!(output.replacements, 3);
}

#[tokio::test]
async fn test_new_content_with_multiple_trailing_newlines() {
    // Multiple trailing newlines should all be stripped by trim_end_matches('\n')
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    fs::write(&path, "line1\nline2").unwrap();

    let result = call_update(path.to_str().unwrap(), 2, 1, "replacement\n\n\n")
        .await
        .unwrap();
    let output = parse_output(&result);
    // All trailing \n in new_content are trimmed, so replacement is just "replacement"
    assert_eq!(fs::read_to_string(&path).unwrap(), "line1\nreplacement");
    assert_eq!(output.replacements, 1);
}

#[tokio::test]
async fn test_new_content_with_only_newlines() {
    // new_content consisting ONLY of newlines should produce empty replacement
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    fs::write(&path, "line1\nline2\nline3").unwrap();

    let result = call_update(path.to_str().unwrap(), 2, 1, "\n\n\n")
        .await
        .unwrap();
    let output = parse_output(&result);
    assert_eq!(fs::read_to_string(&path).unwrap(), "line1\nline3");
    assert_eq!(output.replacements, 1);
}

#[tokio::test]
async fn test_build_line_diff_with_trailing_newline() {
    use my_code_agent::tools::build_line_diff;

    let original = vec!["old_line"];
    // new_content with trailing \n — diff should not show an extra +
    let diff = build_line_diff(1, 1, "new_line\n", &original);
    assert!(diff.contains("+new_line"));
    // Count the number of '+' lines — should be exactly 1
    let plus_count = diff.lines().filter(|l| l.starts_with('+')).count();
    assert_eq!(plus_count, 1, "Diff should have exactly 1 + line, not 2");
}

#[tokio::test]
async fn test_closing_bracket_indent_mismatch() {
    // LLM sends "}" (no indent) but preserved line is "    }" (indented).
    // Without bracket-aware dedup, this produces "}}\n    }" → double bracket.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.rs");
    fs::write(
        &path,
        "fn foo() {\n    body();\n}",
    )
    .unwrap();

    // LLM replaces body but includes "}" without indent; preserved line is "}"
    let result = call_update(path.to_str().unwrap(), 2, 1, "    new_body();\n}")
        .await
        .unwrap();
    let output = parse_output(&result);
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "fn foo() {\n    new_body();\n}"
    );
    assert_eq!(output.replacements, 1);
}

#[tokio::test]
async fn test_closing_bracket_with_punctuation() {
    // LLM sends "};" but preserved line is "    };" — should still dedup.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.rs");
    fs::write(
        &path,
        "struct Foo {\n    x: i32,\n};",
    )
    .unwrap();

    let result = call_update(path.to_str().unwrap(), 2, 1, "    y: i32,\n};")
        .await
        .unwrap();
    let output = parse_output(&result);
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "struct Foo {\n    y: i32,\n};"
    );
    assert_eq!(output.replacements, 1);
}

#[tokio::test]
async fn test_cross_bracket_type_no_dedup() {
    // Model sends "}" but preserved line is ");" — different bracket types (} vs )),
    // should NOT deduplicate. This test actually reaches the bracket_kind check
    // because preserved_start (3) < total_lines (5).
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.rs");
    fs::write(
        &path,
        "fn foo() -> Result<()> {\n    bar(\n        x\n    );\n}",
    )
    .unwrap();

    // Replace lines 2-3 only. Preserved line 4 is "    );" (bracket kind: ')').
    // Model includes "}" at end (bracket kind: '{').
    let result = call_update(
        path.to_str().unwrap(),
        2,
        2,
        "    bar(\n        y\n    )\n}",
    )
    .await
    .unwrap();
    let output = parse_output(&result);

    // bracket_kind("}") = Some('}'), bracket_kind(");") = Some(')')
    // '}' != ')' → NOT deduped → model's "}" preserved
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "fn foo() -> Result<()> {\n    bar(\n        y\n    )\n}\n    );\n}"
    );
    assert_eq!(output.replacements, 2);
}

#[tokio::test]
async fn test_cross_bracket_with_semicolon_no_dedup() {
    // Model sends "}" but preserved line is "];" — different bracket types (} vs ]),
    // should NOT deduplicate.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.rs");
    fs::write(
        &path,
        "fn main() {\n    let arr = vec![\n        1,\n    ];\n}",
    )
    .unwrap();

    // Replace lines 2-3 only. Preserved line 4 is "    ];" (bracket kind: ']').
    // Model includes "}" at end (bracket kind: '{').
    let result = call_update(
        path.to_str().unwrap(),
        2,
        2,
        "    let arr = vec![\n        42,\n    ]\n}",
    )
    .await
    .unwrap();
    let output = parse_output(&result);

    // bracket_kind("}") = Some('}'), bracket_kind("];") = Some(']')
    // '}' != ']' → NOT deduped → model's "}" preserved
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "fn main() {\n    let arr = vec![\n        42,\n    ]\n}\n    ];\n}"
    );
    assert_eq!(output.replacements, 2);
}

// ── Tests for insert-mode duplicate line prevention ──

#[tokio::test]
async fn test_insert_duplicate_first_line_dedup() {
    // When inserting (delete_count=0) and new_content starts with the same
    // line as the line before insertion point, the duplicate should be removed.
    // This simulates LLM mistakenly including the preceding line in new_content.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    fs::write(&path, "prefix_line\n").unwrap();

    // Insert after line 1, but new_content INCORRECTLY includes "prefix_line"
    let result = call_update(path.to_str().unwrap(), 2, 0, "prefix_line\nnew_line1\nnew_line2")
        .await
        .unwrap();
    let output = parse_output(&result);
    // Should dedup the duplicate "prefix_line"
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "prefix_line\nnew_line1\nnew_line2\n"
    );
    assert_eq!(output.replacements, 0);
}

#[tokio::test]
async fn test_insert_no_duplicate_when_different() {
    // When inserting (delete_count=0) and new_content starts with a DIFFERENT
    // line than the line before insertion, no dedup should occur.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    fs::write(&path, "prefix_line\n").unwrap();

    let result = call_update(path.to_str().unwrap(), 2, 0, "different_line\nanother_line")
        .await
        .unwrap();
    let output = parse_output(&result);
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "prefix_line\ndifferent_line\nanother_line\n"
    );
    assert_eq!(output.replacements, 0);
}

#[tokio::test]
async fn test_replace_mode_does_not_dedup() {
    // When deleting+replacing (delete_count>0), the first line of new_content
    // should NOT be deduplicated even if it matches the line before insertion.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    fs::write(&path, "line1\nline2\nline3").unwrap();

    // Replace line 2 with content that starts with "line1" (same as line before)
    let result = call_update(path.to_str().unwrap(), 2, 1, "line1\nnew_line2")
        .await
        .unwrap();
    let output = parse_output(&result);
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "line1\nline1\nnew_line2\nline3"
    );
    assert_eq!(output.replacements, 1);
}

#[tokio::test]
async fn test_insert_at_start_no_dedup() {
    // When inserting at the very beginning (start_line=1), there's no previous
    // line to compare against, so no dedup should occur.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    fs::write(&path, "original_line\n").unwrap();

    let result = call_update(path.to_str().unwrap(), 1, 0, "new_first_line")
        .await
        .unwrap();
    let output = parse_output(&result);
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "new_first_line\noriginal_line\n"
    );
    assert_eq!(output.replacements, 0);
}

#[tokio::test]
async fn test_insert_empty_new_content_no_dedup() {
    // When new_content is empty (inserting nothing), the dedup logic should
    // not crash or cause issues.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    fs::write(&path, "some_line\n").unwrap();

    let result = call_update(path.to_str().unwrap(), 2, 0, "")
        .await
        .unwrap();
    let output = parse_output(&result);
    assert_eq!(fs::read_to_string(&path).unwrap(), "some_line\n");
    assert_eq!(output.replacements, 0);
}

#[tokio::test]
async fn test_insert_duplicate_first_line_with_trailing_newline() {
    // Same as test_insert_duplicate_first_line_dedup but new_content has
    // trailing newline (common LLM behavior).
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    fs::write(&path, "prefix_line\n").unwrap();

    let result = call_update(path.to_str().unwrap(), 2, 0, "prefix_line\nnew_line1\n")
        .await
        .unwrap();
    let output = parse_output(&result);
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "prefix_line\nnew_line1\n"
    );
    assert_eq!(output.replacements, 0);
}
