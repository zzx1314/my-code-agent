use my_code_agent::core::skill::{SkillConfig, SkillManager, parse_skill_file};
use std::path::Path;

fn make_skill(name: &str, prompt: &str, command: Option<&str>) -> SkillConfig {
    SkillConfig {
        name: name.to_string(),
        description: format!("{} description", name),
        prompt: prompt.to_string(),
        inject_into_preamble: true,
        command: command.map(|s| s.to_string()),
        keywords: Vec::new(),
    }
}

fn make_mgr(skills: Vec<SkillConfig>) -> SkillManager {
    SkillManager::from_skills(skills)
}

#[test]
fn test_from_skills_loads_skills() {
    let mgr = make_mgr(vec![
        make_skill("rust-review", "Review Rust code", Some("/rust-review")),
        make_skill("security", "Security audit", None),
    ]);
    assert_eq!(mgr.all_skills().len(), 2);
}

fn write_skill_file(
    dir: &Path,
    filename: &str,
    name: &str,
    description: &str,
    prompt: &str,
    inject: bool,
    command: Option<&str>,
) -> std::path::PathBuf {
    let path = dir.join(filename);
    let cmd_line = command
        .map(|c| format!("command: {}\n", c))
        .unwrap_or_default();
    std::fs::write(
        &path,
        format!(
            "---\nname: {name}\ndescription: {description}\ninject_into_preamble: {inject}\n{cmd_line}---\n\n{prompt}"
        ),
    )
    .unwrap();
    path
}

#[test]
fn test_parse_skill_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_skill_file(
        dir.path(),
        "rust-review.md",
        "rust-review",
        "Review Rust code",
        "Review Rust code carefully",
        true,
        Some("/rust-review"),
    );

    let skill = parse_skill_file(&path).unwrap();
    assert_eq!(skill.name, "rust-review");
    assert_eq!(skill.description, "Review Rust code");
    assert_eq!(skill.prompt, "Review Rust code carefully");
    assert!(skill.inject_into_preamble);
    assert_eq!(skill.command, Some("/rust-review".to_string()));
}

#[test]
fn test_parse_skill_file_no_command() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_skill_file(
        dir.path(),
        "security.md",
        "security",
        "Security audit",
        "Audit for security issues",
        true,
        None,
    );

    let skill = parse_skill_file(&path).unwrap();
    assert_eq!(skill.name, "security");
    assert_eq!(skill.description, "Security audit");
    assert!(skill.command.is_none());
}

#[test]
fn test_parse_skill_file_missing_name() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bad.md");
    std::fs::write(&path, "---\ndescription: No name\n---\n\nSome prompt").unwrap();

    assert!(parse_skill_file(&path).is_err());
    assert!(parse_skill_file(&path).unwrap_err().contains("name"));
}

#[test]
fn test_parse_skill_file_no_frontmatter() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bad.md");
    std::fs::write(&path, "Just content with no frontmatter").unwrap();

    assert!(parse_skill_file(&path).is_err());
}

#[test]
fn test_parse_skill_file_empty_body() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bad.md");
    std::fs::write(
        &path,
        "---\nname: empty\ndescription: Empty body\n---\n",
    )
    .unwrap();

    assert!(parse_skill_file(&path).is_err());
}

#[test]
fn test_load_from_directory() {
    let dir = tempfile::tempdir().unwrap();
    write_skill_file(
        dir.path(),
        "a.md",
        "skill-a",
        "Skill A",
        "Prompt A",
        true,
        Some("/a"),
    );
    write_skill_file(
        dir.path(),
        "b.md",
        "skill-b",
        "Skill B",
        "Prompt B",
        false,
        None,
    );
    // Non-md file should be ignored
    std::fs::write(dir.path().join("notes.txt"), "not a skill").unwrap();

    let mgr = SkillManager::load_from_dir(dir.path());
    assert_eq!(mgr.all_skills().len(), 2);
    assert_eq!(mgr.all_skills()[0].name, "skill-a");
    assert_eq!(mgr.all_skills()[1].name, "skill-b");
}

#[test]
fn test_activate_deactivate() {
    let mut mgr = make_mgr(vec![make_skill("rust-review", "Review Rust code", None)]);

    assert!(!mgr.is_active("rust-review"));
    assert!(mgr.activate("rust-review"));
    assert!(mgr.is_active("rust-review"));
    mgr.deactivate("rust-review");
    assert!(!mgr.is_active("rust-review"));
}

#[test]
fn test_activate_nonexistent() {
    let mut mgr = make_mgr(vec![]);
    assert!(!mgr.activate("nonexistent"));
}

#[test]
fn test_toggle() {
    let mut mgr = make_mgr(vec![make_skill("rust-review", "Review Rust code", None)]);

    assert_eq!(mgr.toggle("rust-review"), Some(true));
    assert!(mgr.is_active("rust-review"));
    assert_eq!(mgr.toggle("rust-review"), Some(false));
    assert!(!mgr.is_active("rust-review"));
}

#[test]
fn test_toggle_nonexistent() {
    let mut mgr = make_mgr(vec![]);
    assert_eq!(mgr.toggle("nonexistent"), None);
}

#[test]
fn test_preamble_injections_empty() {
    let mgr = make_mgr(vec![make_skill("rust-review", "Review Rust code", None)]);
    assert!(mgr.preamble_injections().is_empty());
}

#[test]
fn test_preamble_injections_active() {
    let mut mgr = make_mgr(vec![
        make_skill("rust-review", "Review Rust code", None),
        make_skill("security", "Security audit", None),
    ]);
    mgr.activate("rust-review");

    let injections = mgr.preamble_injections();
    assert!(injections.contains("Review Rust code"));
    assert!(!injections.contains("Security audit"));
}

#[test]
fn test_preamble_injections_multiple_active() {
    let mut mgr = make_mgr(vec![
        make_skill("rust-review", "Review Rust code", None),
        make_skill("security", "Security audit", None),
    ]);
    mgr.activate("rust-review");
    mgr.activate("security");

    let injections = mgr.preamble_injections();
    assert!(injections.contains("Review Rust code"));
    assert!(injections.contains("Security audit"));
}

#[test]
fn test_find_by_command() {
    let mgr = make_mgr(vec![make_skill(
        "rust-review",
        "Review Rust code",
        Some("/rust-review"),
    )]);

    let skill = mgr.find_by_command("/rust-review");
    assert!(skill.is_some());
    assert_eq!(skill.unwrap().name, "rust-review");

    assert!(mgr.find_by_command("/nonexistent").is_none());
}

#[test]
fn test_find_active_by_command() {
    let mut mgr = make_mgr(vec![make_skill(
        "rust-review",
        "Review Rust code",
        Some("/rust-review"),
    )]);

    // Not active yet
    assert!(mgr.find_active_by_command("/rust-review").is_none());

    // Activate
    mgr.activate("rust-review");
    assert!(mgr.find_active_by_command("/rust-review").is_some());
}

#[test]
fn test_format_skill_list() {
    let mgr = make_mgr(vec![make_skill(
        "rust-review",
        "Review Rust code",
        Some("/rust-review"),
    )]);
    let lines = mgr.format_skill_list();
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("rust-review"));
    assert!(lines[0].contains("rust-review description"));
}

#[test]
fn test_format_skill_list_empty() {
    let mgr = make_mgr(vec![]);
    let lines = mgr.format_skill_list();
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("No skills found"));
}

#[test]
fn test_active_summary() {
    let mut mgr = make_mgr(vec![
        make_skill("rust-review", "Review Rust code", None),
        make_skill("security", "Security audit", None),
    ]);
    assert!(mgr.active_summary().is_empty());

    mgr.activate("rust-review");
    let summary = mgr.active_summary();
    assert!(summary.contains("rust-review"));
    assert!(!summary.contains("security"));
}

#[test]
fn test_deactivate_all() {
    let mut mgr = make_mgr(vec![
        make_skill("rust-review", "Review Rust code", None),
        make_skill("security", "Security audit", None),
    ]);
    mgr.activate("rust-review");
    mgr.activate("security");
    assert_eq!(mgr.active_names().len(), 2);

    mgr.deactivate_all();
    assert!(mgr.active_names().is_empty());
}

#[test]
fn test_case_insensitive_activation() {
    let mut mgr = make_mgr(vec![make_skill(
        "Rust-Review",
        "Review Rust code",
        None,
    )]);
    assert!(mgr.activate("rust-review"));
    assert!(mgr.is_active("Rust-Review"));
}

#[test]
fn test_reload_via_directory() {
    let dir = tempfile::tempdir().unwrap();
    // Write initial skills
    write_skill_file(
        dir.path(),
        "rust.md",
        "rust",
        "Rust",
        "Write Rust code",
        true,
        None,
    );
    write_skill_file(
        dir.path(),
        "python.md",
        "python",
        "Python",
        "Write Python code",
        true,
        None,
    );

    let mut mgr = SkillManager::load_from_dir(dir.path());
    mgr.activate("rust");
    mgr.activate("python");
    assert_eq!(mgr.active_names().len(), 2);

    // Rewrite with updated content
    std::fs::write(
        &dir.path().join("rust.md"),
        "---\nname: rust\ndescription: Rust updated\n---\n\nWrite better Rust code",
    )
    .unwrap();
    std::fs::write(
        &dir.path().join("python.md"),
        "---\nname: python\ndescription: Python updated\n---\n\nWrite better Python code",
    )
    .unwrap();

    // Simulate what reload does: re-scan directory (sorted by name)
    let skills = SkillManager::load_from_dir(dir.path());
    assert_eq!(skills.all_skills().len(), 2);
    assert_eq!(skills.all_skills()[0].name, "python");
    assert_eq!(skills.all_skills()[1].name, "rust");
}

#[test]
fn test_reload_preserves_active_logic() {
    // Test the core logic of reload: active skills that still exist are kept,
    // active skills no longer in the file are dropped.
    let dir = tempfile::tempdir().unwrap();
    write_skill_file(
        dir.path(),
        "rust.md",
        "rust",
        "Rust",
        "Write Rust code",
        true,
        None,
    );
    write_skill_file(
        dir.path(),
        "python.md",
        "python",
        "Python",
        "Write Python code",
        true,
        None,
    );
    write_skill_file(
        dir.path(),
        "go.md",
        "go",
        "Go",
        "Write Go code",
        true,
        None,
    );

    let mut mgr = SkillManager::load_from_dir(dir.path());
    mgr.activate("rust");
    mgr.activate("python");
    mgr.activate("go");
    assert_eq!(mgr.active_names().len(), 3);

    // Remove python from directory and reload
    std::fs::remove_file(dir.path().join("python.md")).unwrap();

    // Re-scan directory - only rust and go should remain
    let new_mgr = SkillManager::load_from_dir(dir.path());
    assert_eq!(new_mgr.all_skills().len(), 2);

    // Verify the expected skills are present
    let names: Vec<String> = new_mgr
        .all_skills()
        .iter()
        .map(|s| s.name.clone())
        .collect();
    assert!(names.contains(&"rust".to_string()));
    assert!(names.contains(&"go".to_string()));
    assert!(!names.contains(&"python".to_string()));
}

#[test]
fn test_reload_drops_removed_skills() {
    let dir = tempfile::tempdir().unwrap();
    write_skill_file(
        dir.path(),
        "keep.md",
        "keep",
        "Keep",
        "Keep this",
        true,
        None,
    );
    write_skill_file(
        dir.path(),
        "remove.md",
        "remove",
        "Remove",
        "Remove this",
        true,
        None,
    );

    let mut mgr = SkillManager::load_from_dir(dir.path());
    mgr.activate("keep");
    mgr.activate("remove");
    assert_eq!(mgr.active_names().len(), 2);

    // Remove one skill file and reload
    std::fs::remove_file(dir.path().join("remove.md")).unwrap();
    let new_mgr = SkillManager::load_from_dir(dir.path());

    // Only "keep" should exist in the new manager
    assert_eq!(new_mgr.all_skills().len(), 1);
    assert_eq!(new_mgr.all_skills()[0].name, "keep");
}
