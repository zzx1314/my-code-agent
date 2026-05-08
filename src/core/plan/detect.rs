/// Detect whether the given text contains a task plan header.
///
/// Returns `true` if the text includes a markdown heading like
/// `## 📋 Task Plan`, `## Task Plan`, `## Plan`, or `### Plan`.
///
/// A code fence (` ``` `) appearing before the first heading suppresses
/// the detection so that plan-like text inside code blocks is ignored.
pub fn detect_task_plan(text: &str) -> bool {
    if text.contains("```") {
        let first_code = text.find("```").unwrap_or(usize::MAX);
        let first_header = text.find("##").unwrap_or(usize::MAX);
        if first_code < first_header {
            return false;
        }
    }
    text.contains("## 📋 Task Plan")
        || text.contains("## Task Plan")
        || text.contains("## Plan")
        || text.contains("### Plan")
}
