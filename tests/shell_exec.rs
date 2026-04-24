use my_code_agent::tools::shell_exec::{ShellExec, ShellExecArgs, ShellExecOutput};
use rig::tool::Tool;

async fn exec_cmd(command: &str, timeout_secs: u64, cwd: Option<&str>) -> ShellExecOutput {
    ShellExec
        .call(ShellExecArgs {
            command: command.to_string(),
            timeout_secs,
            cwd: cwd.map(|s| s.to_string()),
            auto_approve: true, // skip confirmation prompts in tests
        })
        .await
        .unwrap()
}

#[tokio::test]
async fn test_echo_command() {
    let output = exec_cmd("echo hello", 10, None).await;
    assert_eq!(output.exit_code, Some(0));
    assert!(output.stdout.trim() == "hello");
    assert!(!output.timed_out);
}

#[tokio::test]
async fn test_command_with_stderr() {
    let output = exec_cmd("echo error >&2", 10, None).await;
    assert_eq!(output.exit_code, Some(0));
    assert!(output.stderr.trim().contains("error"));
}

#[tokio::test]
async fn test_failing_command() {
    let output = exec_cmd("exit 42", 10, None).await;
    assert_eq!(output.exit_code, Some(42));
}

#[tokio::test]
async fn test_timeout() {
    let output = exec_cmd("sleep 3", 1, None).await;
    assert!(output.timed_out);
    assert_eq!(output.exit_code, None);
    assert!(output.stderr.contains("timed out"));
}

#[tokio::test]
async fn test_cwd() {
    let output = exec_cmd("pwd", 10, Some("/tmp")).await;
    assert_eq!(output.exit_code, Some(0));
    assert!(output.stdout.trim().contains("/tmp"));
}

#[tokio::test]
async fn test_multiline_output() {
    let output = exec_cmd("echo -e 'line1\nline2\nline3'", 10, None).await;
    assert_eq!(output.exit_code, Some(0));
    let lines: Vec<&str> = output.stdout.trim().lines().collect();
    assert_eq!(lines.len(), 3);
}

#[tokio::test]
async fn test_long_output_is_truncated() {
    // Generate output longer than the 10_000 char stdout limit
    let output = exec_cmd("yes a | head -n 11000", 10, None).await;
    assert_eq!(output.exit_code, Some(0));
    assert!(output.stdout.len() < 15000, "output should be truncated");
    assert!(output.stdout.contains("output truncated"));
}
