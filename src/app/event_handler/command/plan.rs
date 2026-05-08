use crate::app::App;
use crate::core::context_manager::ContextManager;

use super::super::streaming::{reset_streaming_state, spawn_llm_stream};

/// Handle the /plan command: analyze task and create implementation plan without executing
pub(super) fn handle(app: &mut App, input: &str, context_manager: &mut ContextManager) -> bool {
    let task = input.trim().strip_prefix("/plan").unwrap_or("").trim();
    app.chat_history
        .push(("user".to_string(), input.to_string()));
    app.show_banner = false;

    if task.is_empty() {
        app.chat_history.push((
            "assistant".to_string(),
            "📋 **Plan Mode**\n\n\
                    Usage: `/plan <task description>`\n\n\
                    Example: `/plan Add user authentication with JWT tokens`\n\n\
                    In plan mode, I will analyze your task and create a detailed plan \
                    without executing any actions. You can review the plan before proceeding."
                .to_string(),
        ));
        app.auto_scroll = true;
        return true;
    }

    let planning_prompt = format!(
        r#"You are in PLAN-ONLY mode. Your task is to analyze the following request and create a comprehensive, actionable plan.

## Rules for Plan Mode:
- Do NOT execute any tools (no file reads, writes, shell commands, etc.)
- Focus ONLY on planning and analysis
- Create a detailed, step-by-step implementation plan
- Identify potential risks, dependencies, and prerequisites
- Estimate complexity for each step
- Suggest a logical execution order

## Output Format:
Structure your plan as follows:

### 🎯 Objective
[Clear summary of what needs to be accomplished]

### 📋 Prerequisites
[Any setup, dependencies, or information needed before starting]

### 📝 Implementation Plan
1. **Step 1: [Action]**
   - Details: [What exactly to do]
   - Files affected: [Which files to create/modify]
   - Complexity: [Low/Medium/High]
   
2. **Step 2: [Action]**
   ...

### ⚠️ Risks & Considerations
[Potential issues, edge cases, or things to watch out for]

### ✅ Success Criteria
[How to verify the task is complete]

---
**Task:** {task}"#
    );

    reset_streaming_state(app);
    spawn_llm_stream(app, context_manager, &planning_prompt);

    true
}
