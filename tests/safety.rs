use my_code_agent::tools::safety::{
    is_dangerous_deletion, is_dangerous_shell_command, is_dangerous_snippet_deletion,
};

// ── Shell command safety ──

#[test]
fn test_dangerous_shell_commands() {
    assert!(is_dangerous_shell_command("rm -rf /").is_some());
    assert!(is_dangerous_shell_command("rm -rf ~").is_some());
    assert!(is_dangerous_shell_command("sudo apt install something").is_some());
    assert!(is_dangerous_shell_command("git push --force origin main").is_some());
    assert!(is_dangerous_shell_command("git push -f").is_some());
    assert!(is_dangerous_shell_command("npm publish").is_some());
    assert!(is_dangerous_shell_command("cargo publish").is_some());
    assert!(is_dangerous_shell_command("kill -9 1234").is_some());
}

#[test]
fn test_safe_shell_commands() {
    assert!(is_dangerous_shell_command("ls -la").is_none());
    assert!(is_dangerous_shell_command("cargo test").is_none());
    assert!(is_dangerous_shell_command("git status").is_none());
    assert!(is_dangerous_shell_command("echo hello").is_none());
    assert!(is_dangerous_shell_command("cat file.txt").is_none());
    assert!(is_dangerous_shell_command("npm install").is_none());
    assert!(is_dangerous_shell_command("cargo build").is_none());
}

#[test]
fn test_dangerous_shell_case_insensitive() {
    assert!(is_dangerous_shell_command("RM -RF /").is_some());
    assert!(is_dangerous_shell_command("Sudo rm something").is_some());
}

// ── File/directory deletion safety ──

#[test]
fn test_dangerous_deletion_root() {
    assert!(is_dangerous_deletion("/", false).is_some());
    assert!(is_dangerous_deletion("~", false).is_some());
}

#[test]
fn test_dangerous_deletion_system_dirs() {
    assert!(is_dangerous_deletion("/etc/passwd", false).is_some());
    assert!(is_dangerous_deletion("/usr/bin/python", false).is_some());
    assert!(is_dangerous_deletion("/var/log", true).is_some());
}

#[test]
fn test_dangerous_deletion_recursive_project_dirs() {
    assert!(is_dangerous_deletion("node_modules", true).is_some());
    assert!(is_dangerous_deletion("target", true).is_some());
    assert!(is_dangerous_deletion(".git", true).is_some());
    // Path-component matching: nested paths also match
    assert!(is_dangerous_deletion("project/target/debug", true).is_some());
    assert!(is_dangerous_deletion("/home/user/project/node_modules", true).is_some());
}

#[test]
fn test_dangerous_deletion_normal_files() {
    assert!(is_dangerous_deletion("src/main.rs", false).is_none());
    assert!(is_dangerous_deletion("temp.txt", false).is_none());
    assert!(is_dangerous_deletion("build/output.o", false).is_none());
}

#[test]
fn test_dangerous_deletion_hidden_files() {
    assert!(is_dangerous_deletion(".env", false).is_some());
    assert!(is_dangerous_deletion(".gitignore", false).is_some());
}

#[test]
fn test_dangerous_deletion_hidden_test_files_exempt() {
    // Only files ending with _test, .tmp, or _tmp are exempt from the hidden-file check
    assert!(is_dangerous_deletion(".mod_test", false).is_none());
    assert!(is_dangerous_deletion(".cache.tmp", false).is_none());
    assert!(is_dangerous_deletion(".swap_tmp", false).is_none());
}

#[test]
fn test_dangerous_deletion_hidden_files_flagged() {
    // Hidden files without exempt suffixes are flagged
    assert!(is_dangerous_deletion(".test_config", false).is_some());
    assert!(is_dangerous_deletion("tmp/.cache", false).is_some());
}

// ── Snippet deletion safety ──

#[test]
fn test_dangerous_snippet_deletion_config() {
    assert!(is_dangerous_snippet_deletion("Cargo.toml").is_some());
    assert!(is_dangerous_snippet_deletion("package.json").is_some());
    assert!(is_dangerous_snippet_deletion(".env").is_some());
    assert!(is_dangerous_snippet_deletion("config.yaml").is_some());
}

#[test]
fn test_dangerous_snippet_deletion_source_files() {
    assert!(is_dangerous_snippet_deletion("main.rs").is_none());
    assert!(is_dangerous_snippet_deletion("src/lib.ts").is_none());
    assert!(is_dangerous_snippet_deletion("test.txt").is_none());
}
