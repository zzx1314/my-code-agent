use std::collections::HashSet;
use std::fs;
use std::path::Path;

/// Subdirectory name under the app directory where skill markdown files are stored.
const SKILLS_DIR: &str = "skills";

/// A reusable, named behavior package that modifies how the agent operates.
///
/// Skills can inject specialized instructions into the system prompt (preamble)
/// and/or register custom slash commands for quick invocation.
///
/// If `keywords` are specified, the skill will be automatically activated when
/// the user's prompt contains any of the keywords. Skills can also be triggered
/// by typing `@skill-name` in the input, or via a custom slash command.
#[derive(Debug, Clone)]
pub struct SkillConfig {
    /// Unique skill name (e.g. "rust-review", "security-audit").
    pub name: String,
    /// Short description shown in /skill list.
    pub description: String,
    /// The prompt text injected into the system preamble when this skill is active.
    pub prompt: String,
    /// Whether to inject this skill's prompt into the system preamble automatically.
    pub inject_into_preamble: bool,
    /// Optional slash command to trigger this skill (e.g. "/rust-review").
    /// When set, the skill can be activated via this command.
    pub command: Option<String>,
    /// Keywords for automatic activation. When the user's message contains any
    /// of these keywords, the skill is auto-activated. (Comma-separated in frontmatter.)
    pub keywords: Vec<String>,
}

/// Manages skill activation, deactivation, and preamble injection.
///
/// Skills are discovered by scanning markdown (`.md`) files in the
/// `<app_dir>/skills/` directory. Each markdown file contains YAML-style
/// frontmatter with metadata and a body that serves as the skill's prompt.
/// Active skills with `inject_into_preamble = true` have their prompts
/// injected into the system prompt.
#[derive(Debug, Clone)]
pub struct SkillManager {
    /// All skills loaded from the skills directory.
    skills: Vec<SkillConfig>,
    /// Names of currently active skills.
    active: HashSet<String>,
}

impl SkillManager {
    /// Discovers skills by scanning all `.md` files in the skills directory
    /// under the application base directory.
    ///
    /// Creates the skills directory if it doesn't exist.
    /// Returns an empty `SkillManager` if no skills are found.
    pub fn load() -> Self {
        let dir = crate::core::paths::app_file(SKILLS_DIR);
        if !dir.exists() {
            if let Err(e) = fs::create_dir_all(&dir) {
                tracing::error!(
                    path = %dir.display(),
                    error = %e,
                    "Failed to create skills directory. Using empty skills."
                );
                return Self::empty();
            }
            tracing::info!(path = %dir.display(), "Created skills directory");
            return Self::empty();
        }

        Self::load_from_dir(&dir)
    }

    /// Loads skills from all `.md` files in the given directory.
    pub fn load_from_dir(dir: &Path) -> Self {
        let mut skills = Vec::new();

        let entries = match fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(e) => {
                tracing::error!(
                    path = %dir.display(),
                    error = %e,
                    "Failed to read skills directory. Using empty skills."
                );
                return Self::empty();
            }
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }

            match parse_skill_file(&path) {
                Ok(skill) => {
                    tracing::info!(name = %skill.name, path = %path.display(), "Loaded skill");
                    skills.push(skill);
                }
                Err(e) => {
                    tracing::error!(
                        path = %path.display(),
                        error = %e,
                        "Skipping skill file with errors"
                    );
                }
            }
        }

        // Sort by name for deterministic display order
        let mut skills = skills;
        skills.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

        Self {
            skills,
            active: HashSet::new(),
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

    /// Activates skills whose keywords match the given text (case-insensitive).
    /// Only activates skills that are not already active.
    /// Returns a list of (skill_name, prompt) for newly activated skills.
    pub fn auto_activate_matching(&mut self, text: &str) -> Vec<(String, String)> {
        let lower_text = text.to_lowercase();

        // Collect matches first to avoid borrow conflicts
        let matches: Vec<(String, String)> = self
            .skills
            .iter()
            .filter(|s| {
                !self.active.contains(&s.name.to_lowercase())
                    && s.keywords
                        .iter()
                        .any(|kw| lower_text.contains(&kw.to_lowercase()))
            })
            .map(|s| (s.name.clone(), s.prompt.clone()))
            .collect();

        let mut activated = Vec::new();
        for (name, prompt) in &matches {
            self.activate(name);
            activated.push((name.clone(), prompt.clone()));
        }
        activated
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
            return vec!["  No skills found. Add `.md` files to the skills/ directory.".to_string()];
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

    /// Reloads skills by re-scanning the skills directory, preserving active
    /// skills that still exist in the discovered files.
    ///
    /// Returns `Ok((total_loaded, preserved_count, dropped_names))` on success,
    /// or `Err` with a descriptive message on failure.
    pub fn reload(&mut self) -> Result<(usize, usize, Vec<String>), String> {
        let dir = crate::core::paths::app_file(SKILLS_DIR);
        if !dir.exists() {
            return Err(format!("Skills directory not found: {}", dir.display()));
        }

        let old_active = self.active.clone();
        let new_mgr = Self::load_from_dir(&dir);

        let new_names: HashSet<String> =
            new_mgr.skills.iter().map(|s| s.name.to_lowercase()).collect();

        self.skills = new_mgr.skills;
        self.active.retain(|name| new_names.contains(name));

        let dropped: Vec<String> = old_active.difference(&self.active).cloned().collect();

        Ok((self.skills.len(), self.active.len(), dropped))
    }
}

/// Expands `@skill-name` references in the prompt text and auto-activates
/// skills based on keyword matching.
///
/// This function does two things:
/// 1. Finds `@skill-name` patterns in the text and activates those skills,
///    then removes the `@skill-name` text.
/// 2. Checks all skill keywords against the remaining text and auto-activates
///    matching skills that are not already active.
///
/// If any skills were activated, their prompts are prepended to the returned
/// text with a brief header so the LLM receives the skill context.
pub fn expand_skill_refs(prompt: &str, skill_manager: &mut SkillManager) -> String {
    let mut activated_names: Vec<String> = Vec::new();
    let mut has_at_ref = false;

    // Step 1: Find @skill-name references
    // Build a mapping from lowercase name → original name for quick lookup
    let name_map: std::collections::HashMap<String, String> = skill_manager
        .all_skills()
        .iter()
        .map(|s| (s.name.to_lowercase(), s.name.clone()))
        .collect();

    let words: Vec<&str> = prompt.split_whitespace().collect();
    let mut cleaned_words: Vec<&str> = Vec::new();

    for word in &words {
        if word.starts_with('@') {
            let name = word[1..].trim_end_matches(|c: char| {
                c.is_ascii_punctuation() && !matches!(c, '-' | '_')
            });
            let name_lower = name.to_lowercase();
            if let Some(full_name) = name_map.get(&name_lower) {
                if !skill_manager.is_active(full_name) {
                    skill_manager.activate(full_name);
                    activated_names.push(full_name.clone());
                }
                has_at_ref = true;
                continue; // Remove this word
            }
        }
        cleaned_words.push(word);
    }

    let cleaned = cleaned_words.join(" ");

    // Step 2: Auto-activate based on keyword matching
    let auto_activated = skill_manager.auto_activate_matching(&cleaned);
    for (name, _) in &auto_activated {
        if !activated_names.contains(name) {
            activated_names.push(name.clone());
        }
    }

    // Build the final result
    if activated_names.is_empty() {
        if has_at_ref {
            cleaned
        } else {
            prompt.to_string()
        }
    } else {
        // Collect prompts for newly activated skills
        let skill_names_set: std::collections::HashSet<&String> =
            activated_names.iter().collect();
        let skill_prompts: Vec<&SkillConfig> = skill_manager
            .all_skills()
            .iter()
            .filter(|s| skill_names_set.contains(&&s.name))
            .collect();

        let prompt_block: String = skill_prompts
            .iter()
            .map(|s| format!("[Skill activated: {}]\n\n{}", s.name, s.prompt))
            .collect::<Vec<_>>()
            .join("\n\n");

        format!("{}\n\n{}", prompt_block, cleaned)
    }
}

/// Parses a skill from a markdown file with YAML-style frontmatter.
///
/// Expected format:
/// ```markdown
/// ---
/// name: skill-name
/// description: Short description of the skill
/// inject_into_preamble: true
/// command: /skill-command
/// ---
///
/// Skill prompt content goes here...
/// ```
///
/// Returns `Ok(SkillConfig)` on success, or `Err` with a description of the parse failure.
pub fn parse_skill_file(path: &Path) -> Result<SkillConfig, String> {
    let content = fs::read_to_string(path)
        .map_err(|e| format!("Cannot read {}: {}", path.display(), e))?;

    let content = content.trim();

    // Expect frontmatter delimited by `---` lines
    if !content.starts_with("---") {
        return Err(format!(
            "{}: missing frontmatter (expected `---` delimiter)",
            path.display()
        ));
    }

    // Find the closing `---`
    let after_first = content.strip_prefix("---").unwrap().trim_start();
    let closing_pos = after_first.find("\n---").or_else(|| after_first.find("\n---\r"));

    let (frontmatter_block, body) = match closing_pos {
        Some(pos) => {
            let front = &after_first[..pos];
            let after = after_first[pos + 4..].trim(); // skip past `\n---`
            (front, after)
        }
        None => {
            return Err(format!(
                "{}: unclosed frontmatter (missing closing `---`)",
                path.display()
            ));
        }
    };

    let mut name = String::new();
    let mut description = String::new();
    let mut inject_into_preamble = true;
    let mut command: Option<String> = None;
    let mut keywords: Vec<String> = Vec::new();

    for line in frontmatter_block.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = match line.split_once(':') {
            Some((k, v)) => (k.trim(), v.trim()),
            None => continue,
        };

        match key.to_lowercase().as_str() {
            "name" => name = value.trim().to_string(),
            "description" => description = value.trim().to_string(),
            "inject_into_preamble" => {
                inject_into_preamble = matches!(value.to_lowercase().as_str(), "true" | "yes" | "1")
            }
            "command" => {
                let v = value.to_string();
                if !v.is_empty() {
                    command = Some(v);
                }
            }
            "keywords" => {
                keywords = value
                    .split(',')
                    .map(|k| k.trim().to_string())
                    .filter(|k| !k.is_empty())
                    .collect();
            }
            _ => {
                tracing::warn!(
                    path = %path.display(),
                    key = key,
                    "Unknown frontmatter field in skill file"
                );
            }
        }
    }

    if name.is_empty() {
        return Err(format!(
            "{}: missing required 'name' field in frontmatter",
            path.display()
        ));
    }

    if description.is_empty() {
        return Err(format!(
            "{}: missing required 'description' field in frontmatter",
            path.display()
        ));
    }

    if body.is_empty() {
        return Err(format!(
            "{}: skill body (prompt) is empty after frontmatter",
            path.display()
        ));
    }

    Ok(SkillConfig {
        name,
        description,
        prompt: body.to_string(),
        inject_into_preamble,
        command,
        keywords,
    })
}
