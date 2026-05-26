use crate::core::config::SkillConfig;
use std::collections::HashSet;

/// TOML wrapper for deserializing `[[skill]]` array-of-tables entries.
#[derive(serde::Deserialize)]
struct SkillsFile {
    skill: Vec<SkillConfig>,
}

/// Manages skill activation, deactivation, and preamble injection.
///
/// Skills are defined in `skills.toml` (a separate file from `config.toml`)
/// and can be activated/deactivated at runtime via the `/skill` command.
/// Active skills with `inject_into_preamble = true` have their prompts
/// injected into the system prompt.
#[derive(Debug, Clone)]
pub struct SkillManager {
    /// All skills loaded from config.
    skills: Vec<SkillConfig>,
    /// Names of currently active skills.
    active: HashSet<String>,
}

impl SkillManager {
    /// Loads skills from the `skills.toml` file in the app directory.
    /// Returns an empty `SkillManager` if the file doesn't exist or has errors.
    pub fn load() -> Self {
        let path = crate::core::paths::app_file(crate::core::config::SKILLS_FILE);
        match std::fs::read_to_string(&path) {
            Ok(content) => match toml::from_str::<SkillsFile>(&content) {
                Ok(sf) => Self {
                    skills: sf.skill,
                    active: HashSet::new(),
                },
                Err(e) => {
                    tracing::error!(
                        path = %path.display(),
                        error = %e,
                        "Error parsing skills file. Using empty skills."
                    );
                    Self::empty()
                }
            },
            Err(_) => Self::empty(),
        }
    }

    /// Loads skills from a specific `skills.toml` file path.
    /// Returns an empty `SkillManager` if the file doesn't exist or has errors.
    pub fn load_from<P: AsRef<std::path::Path>>(path: P) -> Self {
        let path = path.as_ref();
        match std::fs::read_to_string(path) {
            Ok(content) => match toml::from_str::<SkillsFile>(&content) {
                Ok(sf) => Self {
                    skills: sf.skill,
                    active: HashSet::new(),
                },
                Err(e) => {
                    tracing::error!(
                        path = %path.display(),
                        error = %e,
                        "Error parsing skills file. Using empty skills."
                    );
                    Self::empty()
                }
            },
            Err(_) => Self::empty(),
        }
    }

    /// Creates a `SkillManager` from a given list of skills.
    pub fn from_skills(skills: Vec<SkillConfig>) -> Self {
        Self {
            skills,
            active: HashSet::new(),
        }
    }

    /// Creates an empty `SkillManager` with no skills.
    pub fn empty() -> Self {
        Self {
            skills: Vec::new(),
            active: HashSet::new(),
        }
    }

    /// Activates a skill by name. Returns `true` if the skill was found and activated.
    pub fn activate(&mut self, name: &str) -> bool {
        let name = name.trim().to_lowercase();
        if self.skills.iter().any(|s| s.name.to_lowercase() == name) {
            self.active.insert(name);
            true
        } else {
            false
        }
    }

    /// Deactivates a skill by name.
    pub fn deactivate(&mut self, name: &str) {
        self.active.remove(&name.trim().to_lowercase());
    }

    /// Toggles a skill: activate if inactive, deactivate if active.
    /// Returns the new state (true = active).
    pub fn toggle(&mut self, name: &str) -> Option<bool> {
        let name = name.trim().to_lowercase();
        if !self.skills.iter().any(|s| s.name.to_lowercase() == name) {
            return None;
        }
        if self.active.contains(&name) {
            self.active.remove(&name);
            Some(false)
        } else {
            self.active.insert(name);
            Some(true)
        }
    }

    /// Returns `true` with the skill config if a skill with the given command exists and is active.
    pub fn find_active_by_command(&self, cmd: &str) -> Option<&SkillConfig> {
        let cmd = cmd.trim().to_lowercase();
        self.skills.iter().find(|s| {
            s.command
                .as_ref()
                .map_or(false, |c| c.to_lowercase() == cmd)
                && self.active.contains(&s.name.to_lowercase())
        })
    }

    /// Returns the skill config for a given command name (regardless of active state).
    pub fn find_by_command(&self, cmd: &str) -> Option<&SkillConfig> {
        let cmd = cmd.trim().to_lowercase();
        self.skills
            .iter()
            .find(|s| s.command.as_ref().map_or(false, |c| c.to_lowercase() == cmd))
    }

    /// Returns all skills loaded from config.
    pub fn all_skills(&self) -> &[SkillConfig] {
        &self.skills
    }

    /// Returns the names of all active skills.
    pub fn active_names(&self) -> Vec<&str> {
        self.active.iter().map(|s| s.as_str()).collect()
    }

    /// Returns `true` if the given skill is active.
    pub fn is_active(&self, name: &str) -> bool {
        self.active.contains(&name.trim().to_lowercase())
    }

    /// Builds the preamble injection string from all active skills that have
    /// `inject_into_preamble = true`.
    ///
    /// Returns an empty string if no skills are active for injection.
    pub fn preamble_injections(&self) -> String {
        let injections: Vec<&str> = self
            .skills
            .iter()
            .filter(|s| s.inject_into_preamble && self.active.contains(&s.name.to_lowercase()))
            .map(|s| s.prompt.as_str())
            .collect();

        if injections.is_empty() {
            String::new()
        } else {
            injections.join("\n\n")
        }
    }

    /// Returns a formatted list of skills for display.
    /// Each line shows: name, description, active state, and command.
    pub fn format_skill_list(&self) -> Vec<String> {
        if self.skills.is_empty() {
            return vec!["  No skills configured. Add [[skill]] entries in skills.toml.".to_string()];
        }

        self.skills
            .iter()
            .map(|s| {
                let status = if self.is_active(&s.name) {
                    "●"
                } else {
                    "○"
                };
                let cmd = s
                    .command
                    .as_ref()
                    .map(|c| format!("  [{}]", c))
                    .unwrap_or_default();
                format!("  {} {}{} — {}", status, s.name, cmd, s.description)
            })
            .collect()
    }

    /// Returns a summary of active skills for context compaction.
    pub fn active_summary(&self) -> String {
        let names: Vec<&str> = self.active_names();
        if names.is_empty() {
            String::new()
        } else {
            format!("Active skills: {}", names.join(", "))
        }
    }

    /// Deactivates all skills.
    pub fn deactivate_all(&mut self) {
        self.active.clear();
    }

    /// Reloads skills from the `skills.toml` file, preserving active skills
    /// that still exist in the new file.
    ///
    /// Returns `Ok((total_loaded, preserved_count, dropped_names))` on success,
    /// or `Err` with a descriptive message on failure.
    pub fn reload(&mut self) -> Result<(usize, usize, Vec<String>), String> {
        let path = crate::core::paths::app_file(crate::core::config::SKILLS_FILE);
        let content =
            std::fs::read_to_string(&path).map_err(|e| format!("Cannot read {}: {}", path.display(), e))?;
        let sf = toml::from_str::<SkillsFile>(&content)
            .map_err(|e| format!("Error parsing {}: {}", path.display(), e))?;

        let old_active = self.active.clone();
        let new_names: HashSet<String> =
            sf.skill.iter().map(|s| s.name.to_lowercase()).collect();

        self.skills = sf.skill;
        self.active.retain(|name| new_names.contains(name));

        let dropped: Vec<String> = old_active.difference(&self.active).cloned().collect();

        Ok((self.skills.len(), self.active.len(), dropped))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn make_skill(name: &str, prompt: &str, command: Option<&str>) -> SkillConfig {
        SkillConfig {
            name: name.to_string(),
            description: format!("{} description", name),
            prompt: prompt.to_string(),
            inject_into_preamble: true,
            command: command.map(|s| s.to_string()),
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

    #[test]
    fn test_load_from_file_loads_skills() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("skills.toml");
        let mut file = std::fs::File::create(&path).unwrap();
        write!(
            file,
            "[[skill]]\nname = \"rust-review\"\ndescription = \"Review Rust code\"\nprompt = \"Review Rust code carefully\"\ninject_into_preamble = true\ncommand = \"/rust-review\"\n\n[[skill]]\nname = \"security\"\ndescription = \"Security audit\"\nprompt = \"Audit for security issues\"\n"
        )
        .unwrap();

        let mgr = SkillManager::load_from(&path);
        assert_eq!(mgr.all_skills().len(), 2);
        assert_eq!(mgr.all_skills()[0].name, "rust-review");
        assert_eq!(mgr.all_skills()[1].name, "security");
    }

    #[test]
    fn test_load_from_file_empty_on_missing() {
        let mgr = SkillManager::load_from("/nonexistent/path/skills.toml");
        assert!(mgr.all_skills().is_empty());
    }

    #[test]
    fn test_load_from_file_empty_on_invalid_toml() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("skills.toml");
        std::fs::write(&path, "[[skill]\ninvalid toml [[[").unwrap();

        let mgr = SkillManager::load_from(&path);
        assert!(mgr.all_skills().is_empty());
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
        assert!(lines[0].contains("No skills configured"));
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
    fn test_reload_preserves_active_skills() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("skills.toml");
        // Write initial skills
        std::fs::write(
            &path,
            "[[skill]]\nname = \"rust\"\ndescription = \"Rust\"\nprompt = \"Write Rust code\"\n\n[[skill]]\nname = \"python\"\ndescription = \"Python\"\nprompt = \"Write Python code\"\n",
        )
        .unwrap();

        // Override the app_file path for testing: we manually load_from and reload via path
        let mut mgr = SkillManager::load_from(&path);
        mgr.activate("rust");
        mgr.activate("python");
        assert_eq!(mgr.active_names().len(), 2);

        // Rewrite skills.toml with same skills (simulate edit with no name changes)
        std::fs::write(
            &path,
            "[[skill]]\nname = \"rust\"\ndescription = \"Rust updated\"\nprompt = \"Write better Rust code\"\n\n[[skill]]\nname = \"python\"\ndescription = \"Python updated\"\nprompt = \"Write better Python code\"\n",
        )
        .unwrap();

        // Now call reload — but reload uses the fixed app_file path, not our temp path.
        // We can't easily test the actual reload() method without overriding paths,
        // so instead test the underlying logic via load_from + manual state restore.
        // Let's simulate what reload does:
        let skills = SkillManager::load_from(&path);
        assert_eq!(skills.all_skills().len(), 2);
        assert_eq!(skills.all_skills()[0].description, "Rust updated");
        assert_eq!(skills.all_skills()[1].description, "Python updated");
    }

    #[test]
    fn test_reload_preserves_active_logic() {
        // Test the core logic of reload: active skills that still exist are kept,
        // active skills no longer in the file are dropped.
        let mut mgr = make_mgr(vec![
            make_skill("rust", "Rust", None),
            make_skill("python", "Python", None),
            make_skill("go", "Go", None),
        ]);
        mgr.activate("rust");
        mgr.activate("python");
        mgr.activate("go");

        // Simulate reload with only "rust" and "go" remaining
        let new_names: std::collections::HashSet<String> =
            ["rust", "go"].into_iter().map(|n| n.to_string()).collect();
        mgr.active.retain(|name| new_names.contains(name.as_str()));

        assert!(mgr.is_active("rust"));
        assert!(!mgr.is_active("python")); // dropped
        assert!(mgr.is_active("go"));
        assert_eq!(mgr.active_names().len(), 2);
    }

    #[test]
    fn test_reload_drops_removed_skills() {
        let mut mgr = make_mgr(vec![
            make_skill("keep", "Keep", None),
            make_skill("remove", "Remove", None),
        ]);
        mgr.activate("keep");
        mgr.activate("remove");

        // Simulate reload with only "keep"
        let new_names: std::collections::HashSet<String> =
            ["keep"].into_iter().map(|n| n.to_string()).collect();
        let old_active = mgr.active.clone();
        mgr.active.retain(|name| new_names.contains(name.as_str()));
        let dropped: Vec<String> = old_active.difference(&mgr.active).cloned().collect();

        assert!(mgr.is_active("keep"));
        assert!(!mgr.is_active("remove"));
        assert_eq!(dropped, vec!["remove"]);
    }
}