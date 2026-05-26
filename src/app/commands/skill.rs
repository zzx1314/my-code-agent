use crate::app::App;

/// Handle /skill command — manage skill activation and deactivation.
///
/// Usage:
///   /skill              — list all skills and their active state
///   /skill <name>       — toggle a skill on/off
///   /skill --on <name>  — activate a skill
///   /skill --off <name> — deactivate a skill
///   /skill --all        — activate all skills
///   /skill --none       — deactivate all skills
///   /skill --reload     — reload skills from skills.toml
///   /skill --help       — show this help
pub fn handle(app: &mut App, input: &str) -> bool {
    let args = input.trim().strip_prefix("/skill").unwrap_or("").trim();

    match args {
        "" | "--help" | "-h" => {
            show_list(app);
            if args == "--help" || args == "-h" {
                show_help(app);
            }
            true
        }
        "--all" => {
            for skill in app.skill_manager.all_skills().to_vec() {
                app.skill_manager.activate(&skill.name);
            }
            app.status_messages
                .push("✅ All skills activated".into());
            show_list(app);
            true
        }
        "--none" => {
            app.skill_manager.deactivate_all();
            app.status_messages
                .push("○ All skills deactivated".into());
            show_list(app);
            true
        }
        "--reload" => {
            match app.skill_manager.reload() {
                Ok((total, preserved, dropped)) => {
                    app.status_messages.push(format!(
                        "🔄 Skills reloaded from skills.toml ({} loaded)",
                        total
                    ));
                    if preserved > 0 {
                        app.status_messages
                            .push(format!("  ● {} skills kept active", preserved));
                    }
                    if !dropped.is_empty() {
                        let dropped_list = dropped.join(", ");
                        app.status_messages.push(format!(
                            "  ○ {} skills deactivated (no longer in file): {}",
                            dropped.len(),
                            dropped_list
                        ));
                    }
                    app.status_messages.push(String::new());
                    show_list(app);
                }
                Err(e) => {
                    app.status_messages
                        .push(format!("❌ Reload failed: {}", e));
                }
            }
            true
        }
        s if s.starts_with("--on ") => {
            let name = s.strip_prefix("--on ").unwrap().trim();
            if name.is_empty() {
                app.status_messages
                    .push("Usage: /skill --on <name>".into());
            } else if app.skill_manager.activate(name) {
                app.status_messages
                    .push(format!("✅ Skill '{}' activated", name));
                show_list(app);
            } else {
                app.status_messages
                    .push(format!("❌ Skill '{}' not found. Use /skill to see available skills.", name));
            }
            true
        }
        s if s.starts_with("--off ") => {
            let name = s.strip_prefix("--off ").unwrap().trim();
            if name.is_empty() {
                app.status_messages
                    .push("Usage: /skill --off <name>".into());
            } else {
                app.skill_manager.deactivate(name);
                app.status_messages
                    .push(format!("○ Skill '{}' deactivated", name));
                show_list(app);
            }
            true
        }
        name => {
            // Toggle by name
            match app.skill_manager.toggle(name) {
                Some(true) => {
                    app.status_messages
                        .push(format!("✅ Skill '{}' activated", name));
                    show_list(app);
                }
                Some(false) => {
                    app.status_messages
                        .push(format!("○ Skill '{}' deactivated", name));
                    show_list(app);
                }
                None => {
                    app.status_messages.push(format!(
                        "❌ Skill '{}' not found. Use /skill to see available skills.",
                        name
                    ));
                }
            }
            true
        }
    }
}

fn show_list(app: &mut App) {
    for line in app.skill_manager.format_skill_list() {
        app.status_messages.push(line);
    }
}

fn show_help(app: &mut App) {
    app.status_messages.push("".into());
    app.status_messages.push("  Usage:".into());
    app.status_messages.push("    /skill              — list all skills and their active state".into());
    app.status_messages.push("    /skill <name>       — toggle a skill on/off".into());
    app.status_messages.push("    /skill --on <name>  — activate a skill".into());
    app.status_messages.push("    /skill --off <name> — deactivate a skill".into());
    app.status_messages.push("    /skill --all        — activate all skills".into());
    app.status_messages.push("    /skill --none       — deactivate all skills".into());
    app.status_messages.push("    /skill --reload     — reload skills from skills.toml".into());
    app.status_messages.push("".into());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    use crate::core::agent::preamble::{Agent, build_client};
    use crate::core::config::Config;
    use crate::core::context::token_usage::TokenUsage;
    use std::sync::Arc;
    use tokio::sync::broadcast;

    fn make_app_with_skills(skills: Vec<crate::core::config::SkillConfig>) -> App {
        let config = Config::default();
        let client = build_client(&config);
        let agent = Arc::new(Agent::new(client, "test prompt".to_string(), crate::tools::ToolRegistry::new()));
        let (tx, _) = broadcast::channel(1);
        let mut app = App::new(Vec::new(), TokenUsage::default(), String::new(), config, agent, tx);
        app.skill_manager = crate::core::skill::SkillManager::from_skills(skills);
        app
    }

    #[test]
    fn test_skill_list_empty() {
        let mut app = make_app_with_skills(vec![]);
        handle(&mut app, "/skill");
        assert!(app.status_messages.iter().any(|m| m.contains("No skills configured")));
    }

    #[test]
    fn test_skill_activate() {
        let skill = crate::core::config::SkillConfig {
            name: "rust-review".to_string(),
            description: "Review Rust code".to_string(),
            prompt: "Review Rust code carefully".to_string(),
            inject_into_preamble: true,
            command: None,
        };
        let mut app = make_app_with_skills(vec![skill]);
        handle(&mut app, "/skill rust-review");
        assert!(app.skill_manager.is_active("rust-review"));
    }

    #[test]
    fn test_skill_toggle() {
        let skill = crate::core::config::SkillConfig {
            name: "rust-review".to_string(),
            description: "Review Rust code".to_string(),
            prompt: "Review Rust code carefully".to_string(),
            inject_into_preamble: true,
            command: None,
        };
        let mut app = make_app_with_skills(vec![skill]);
        handle(&mut app, "/skill rust-review");
        assert!(app.skill_manager.is_active("rust-review"));
        handle(&mut app, "/skill rust-review");
        assert!(!app.skill_manager.is_active("rust-review"));
    }

    #[test]
    fn test_skill_all() {
        let s1 = crate::core::config::SkillConfig {
            name: "skill-a".to_string(), description: "A".to_string(),
            prompt: "Prompt A".to_string(), inject_into_preamble: true, command: None,
        };
        let s2 = crate::core::config::SkillConfig {
            name: "skill-b".to_string(), description: "B".to_string(),
            prompt: "Prompt B".to_string(), inject_into_preamble: true, command: None,
        };
        let mut app = make_app_with_skills(vec![s1, s2]);
        handle(&mut app, "/skill --all");
        assert!(app.skill_manager.is_active("skill-a"));
        assert!(app.skill_manager.is_active("skill-b"));
    }

    #[test]
    fn test_skill_none() {
        let s1 = crate::core::config::SkillConfig {
            name: "skill-a".to_string(), description: "A".to_string(),
            prompt: "Prompt A".to_string(), inject_into_preamble: true, command: None,
        };
        let mut app = make_app_with_skills(vec![s1]);
        app.skill_manager.activate("skill-a");
        handle(&mut app, "/skill --none");
        assert!(!app.skill_manager.is_active("skill-a"));
    }
}
