use colored::*;
use std::io::Write;

/// Shell command patterns that are considered dangerous and require user confirmation.
const DANGEROUS_SHELL_PATTERNS: &[&str] = &[
    // Filesystem destruction
    "rm -rf /",
    "rm -rf /*",
    "rm -rf ~",
    "rm -fr /",
    "rm -fr ~",
    // Force push / rewrite history
    "git push --force",
    "git push -f",
    "git push --force-with-lease",
    // Destructive git operations
    "git reset --hard",
    "git clean -f",
    "git clean -fd",
    // Package publishes (irreversible)
    "npm publish",
    "cargo publish",
    "pip upload",
    // System-level changes
    "sudo ",
    "chmod -R",
    "chown ",
    // Process killing
    "kill -9",
    "killall",
    // Disk operations
    "mkfs",
    "dd if=",
    // Overwrite redirect to system paths
    "> /etc/",
    ">> /etc/",
];

/// Checks whether a shell command matches any dangerous pattern.
/// Returns the first matching pattern, if any.
pub fn is_dangerous_shell_command(command: &str) -> Option<&'static str> {
    let lower = command.to_lowercase();
    DANGEROUS_SHELL_PATTERNS
        .iter()
        .find(|&&pattern| lower.contains(&pattern.to_lowercase()))
        .copied()
}

/// Checks whether a file/directory deletion target is potentially dangerous.
/// Returns a reason string if the deletion looks risky.
pub fn is_dangerous_deletion(path: &str, recursive: bool) -> Option<&'static str> {
    let p = path.trim();

    // Root or home directory
    if p == "/" || p == "/home" || p == "~" || p == "/root" {
        return Some("refusing to delete root or home directory");
    }

    // Critical system directories
    let critical_dirs = [
        "/etc", "/usr", "/bin", "/sbin", "/lib", "/var", "/sys", "/proc", "/dev", "/boot", "/opt",
        "/snap",
    ];
    for dir in &critical_dirs {
        if p == *dir || p.starts_with(&format!("{}/", dir)) {
            return Some("refusing to delete system directory");
        }
    }

    // Recursive deletion of large/common directories
    if recursive {
        let risky_recursive = ["node_modules", "vendor", ".git", "target", "dist", "build"];
        for dir in &risky_recursive {
            // Match by path component so both "target" and "project/target" are caught
            if p.split('/').any(|component| component == *dir) {
                return Some("recursive deletion of common project directory — please confirm");
            }
        }
    }

    // Hidden files/directories (often contain config)
    let filename = p.rsplit('/').next().unwrap_or(p);
    if filename.starts_with('.')
        && !filename.ends_with("_test")
        && !filename.ends_with(".tmp")
        && !filename.ends_with("_tmp")
    {
        return Some("deleting hidden/config file — please confirm");
    }

    None
}

/// Checks whether a snippet deletion in a file looks potentially dangerous.
/// Returns a reason string if the deletion looks risky.
pub fn is_dangerous_snippet_deletion(path: &str) -> Option<&'static str> {
    let filename = path.rsplit('/').next().unwrap_or(path);

    // Deleting from config/lock files is risky
    let config_patterns = [
        "Cargo.toml",
        "package.json",
        ".env",
        "config.",
        "settings.",
        "docker-compose",
        "Dockerfile",
        "Makefile",
        ".gitignore",
        ".bashrc",
        ".zshrc",
        ".profile",
    ];
    for pattern in &config_patterns {
        if filename.contains(pattern) {
            return Some("modifying config file — please confirm");
        }
    }

    None
}

/// Prompts the user for confirmation of a dangerous action.
/// Returns `true` if the user approves, `false` otherwise.
pub async fn confirm_action(reason: &str, detail: &str) -> bool {
    println!();
    println!(
        "  {} {}",
        "⚠".bright_yellow().bold(),
        reason.bright_yellow()
    );
    println!(
        "  {} {}",
        "→".bright_white(),
        detail.bright_white().dimmed()
    );
    print!(
        "  {} {}",
        "?".bright_cyan().bold(),
        "Proceed? [y/N] ".bright_cyan()
    );
    let _ = std::io::stdout().flush();

    let response = read_confirmation().await;

    if response {
        println!(
            "  {} {}",
            "✓".bright_green(),
            "Proceeding with action.".dimmed()
        );
    } else {
        println!("  {} {}", "✗".bright_red(), "Action cancelled.".dimmed());
    }

    response
}

/// Reads a yes/no confirmation from stdin on a blocking thread.
async fn read_confirmation() -> bool {
    tokio::task::spawn_blocking(|| {
        let mut buf = String::new();
        match std::io::stdin().read_line(&mut buf) {
            Ok(_) => {
                let answer = buf.trim().to_lowercase();
                answer == "y" || answer == "yes"
            }
            Err(_) => false,
        }
    })
    .await
    .unwrap_or(false)
}
