use crate::core::config::SkillConfig;
use std::collections::HashSet;

/// Manages skill activation, deactivation, and preamble injection.
///
/// Skills are defined in `config.toml` and can be activated/deactivated at runtime
/// via the `/skill` command. Active skills with `inject_into_preamble = true`
/// have their prompts injected into the system prompt.
#[derive(Debug, Clone)]
pub struct SkillManager {
    /// All skills loaded from config.
    skills: Vec<SkillConfig>,
    /// Names of currently active skills.
    active: HashSet<String>,
}

impl SkillManager {
    /// Creates a new `SkillManager` from the config's skill definitions.
    pub fn from_config(config: &crate::core::config::Config) -> Self {
        Self {
            skills: config.skills.clone(),
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
            return vec!["  No skills configured. Add [[skill]] entries in config.toml.".to_string()];
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::Config;

    fn make_skill(name: &str, prompt: &str, command: Option<&str>) -> SkillConfig {
        SkillConfig {
            name: name.to_string(),
            description: format!("{} description", name),
            prompt: prompt.to_string(),
            inject_into_preamble: true,
            command: command.map(|s| s.to_string()),
        }
    }

    fn make_config_with_skills(skills: Vec<SkillConfig>) -> Config {
        let mut config = Config::default();
        config.skills = skills;
        config
    }

    #[test]
    fn test_from_config_loads_skills() {
        let config = make_config_with_skills(vec![
            make_skill("rust-review", "Review Rust code", Some("/rust-review")),
            make_skill("security", "Security audit", None),
        ]);
        let mgr = SkillManager::from_config(&config);
        assert_eq!(mgr.all_skills().len(), 2);
    }

    #[test]
    fn test_activate_deactivate() {
        let config = make_config_with_skills(vec![make_skill("rust-review", "Review Rust code", None)]);
        let mut mgr = SkillManager::from_config(&config);

        assert!(!mgr.is_active("rust-review"));
        assert!(mgr.activate("rust-review"));
        assert!(mgr.is_active("rust-review"));
        mgr.deactivate("rust-review");
        assert!(!mgr.is_active("rust-review"));
    }

    #[test]
    fn test_activate_nonexistent() {
        let config = make_config_with_skills(vec![]);
        let mut mgr = SkillManager::from_config(&config);
        assert!(!mgr.activate("nonexistent"));
    }

    #[test]
    fn test_toggle() {
        let config = make_config_with_skills(vec![make_skill("rust-review", "Review Rust code", None)]);
        let mut mgr = SkillManager::from_config(&config);

        assert_eq!(mgr.toggle("rust-review"), Some(true));
        assert!(mgr.is_active("rust-review"));
        assert_eq!(mgr.toggle("rust-review"), Some(false));
        assert!(!mgr.is_active("rust-review"));
    }

    #[test]
    fn test_toggle_nonexistent() {
        let config = make_config_with_skills(vec![]);
        let mut mgr = SkillManager::from_config(&config);
        assert_eq!(mgr.toggle("nonexistent"), None);
    }

    #[test]
    fn test_preamble_injections_empty() {
        let config = make_config_with_skills(vec![make_skill("rust-review", "Review Rust code", None)]);
        let mgr = SkillManager::from_config(&config);
        assert!(mgr.preamble_injections().is_empty());
    }

    #[test]
    fn test_preamble_injections_active() {
        let config = make_config_with_skills(vec![
            make_skill("rust-review", "Review Rust code", None),
            make_skill("security", "Security audit", None),
        ]);
        let mut mgr = SkillManager::from_config(&config);
        mgr.activate("rust-review");

        let injections = mgr.preamble_injections();
        assert!(injections.contains("Review Rust code"));
        assert!(!injections.contains("Security audit"));
    }

    #[test]
    fn test_preamble_injections_multiple_active() {
        let config = make_config_with_skills(vec![
            make_skill("rust-review", "Review Rust code", None),
            make_skill("security", "Security audit", None),
        ]);
        let mut mgr = SkillManager::from_config(&config);
        mgr.activate("rust-review");
        mgr.activate("security");

        let injections = mgr.preamble_injections();
        assert!(injections.contains("Review Rust code"));
        assert!(injections.contains("Security audit"));
    }

    #[test]
    fn test_find_by_command() {
        let config = make_config_with_skills(vec![make_skill(
            "rust-review",
            "Review Rust code",
            Some("/rust-review"),
        )]);
        let mgr = SkillManager::from_config(&config);

        let skill = mgr.find_by_command("/rust-review");
        assert!(skill.is_some());
        assert_eq!(skill.unwrap().name, "rust-review");

        assert!(mgr.find_by_command("/nonexistent").is_none());
    }

    #[test]
    fn test_find_active_by_command() {
        let config = make_config_with_skills(vec![make_skill(
            "rust-review",
            "Review Rust code",
            Some("/rust-review"),
        )]);
        let mut mgr = SkillManager::from_config(&config);

        // Not active yet
        assert!(mgr.find_active_by_command("/rust-review").is_none());

        // Activate
        mgr.activate("rust-review");
        assert!(mgr.find_active_by_command("/rust-review").is_some());
    }

    #[test]
    fn test_format_skill_list() {
        let config = make_config_with_skills(vec![make_skill(
            "rust-review",
            "Review Rust code",
            Some("/rust-review"),
        )]);
        let mgr = SkillManager::from_config(&config);
        let lines = mgr.format_skill_list();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("rust-review"));
        assert!(lines[0].contains("rust-review description"));
    }

    #[test]
    fn test_format_skill_list_empty() {
        let config = make_config_with_skills(vec![]);
        let mgr = SkillManager::from_config(&config);
        let lines = mgr.format_skill_list();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("No skills configured"));
    }

    #[test]
    fn test_active_summary() {
        let config = make_config_with_skills(vec![
            make_skill("rust-review", "Review Rust code", None),
            make_skill("security", "Security audit", None),
        ]);
        let mut mgr = SkillManager::from_config(&config);
        assert!(mgr.active_summary().is_empty());

        mgr.activate("rust-review");
        let summary = mgr.active_summary();
        assert!(summary.contains("rust-review"));
        assert!(!summary.contains("security"));
    }

    #[test]
    fn test_deactivate_all() {
        let config = make_config_with_skills(vec![
            make_skill("rust-review", "Review Rust code", None),
            make_skill("security", "Security audit", None),
        ]);
        let mut mgr = SkillManager::from_config(&config);
        mgr.activate("rust-review");
        mgr.activate("security");
        assert_eq!(mgr.active_names().len(), 2);

        mgr.deactivate_all();
        assert!(mgr.active_names().is_empty());
    }

    #[test]
    fn test_case_insensitive_activation() {
        let config = make_config_with_skills(vec![make_skill(
            "Rust-Review",
            "Review Rust code",
            None,
        )]);
        let mut mgr = SkillManager::from_config(&config);
        assert!(mgr.activate("rust-review"));
        assert!(mgr.is_active("Rust-Review"));
    }
}
