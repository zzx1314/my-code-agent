//! `/review` 命令 — 手动触发代码审查
//!
//! 语法:
//!   /review              — 审查当前对话中涉及的代码
//!   /review <path>       — 审查指定路径的代码
//!   /review --auto       — 切换自动审查模式的开关状态

use crate::app::App;
use crate::core::context::context_manager::ContextManager;
use crate::core::types::review::{ChangedFile, ChangeType};

/// 处理 `/review` 命令
pub fn handle(app: &mut App, input: &str, _context_manager: &mut ContextManager) -> bool {
    let args = input.trim();
    let parts: Vec<&str> = args.split_whitespace().collect();

    match parts.get(1).copied() {
        Some("--auto" | "-a") => {
            toggle_auto_review(app);
            true
        }
        Some("--help" | "-h") => {
            show_help(app);
            true
        }
        Some(path) => {
            let path_str = path.to_string();
            app.chat_history.push(crate::app::ChatEntry::assistant(
                format!("🔍 正在审查 `{}`...", path_str),
            ));
            spawn_review(app, Some(path_str));
            false
        }
        None => {
            app.chat_history.push(crate::app::ChatEntry::assistant(
                "🔍 正在审查最近的代码变更...".to_string(),
            ));
            spawn_review(app, None);
            false
        }
    }
}

/// 切换自动审查开关
fn toggle_auto_review(app: &mut App) {
    let new_state = {
        let orchestrator = match app.orchestrator.as_mut() {
            Some(o) => o,
            None => {
                app.chat_history.push(crate::app::ChatEntry::assistant(
                    "⚠️ 审查系统未初始化。请重启应用。".to_string(),
                ));
                return;
            }
        };

        // 通过 Arc::get_mut 获取唯一所有权（此时 refcount 应为 1）
        let orch = std::sync::Arc::get_mut(orchestrator)
            .expect("Orchestrator should have unique ownership at this point");
        let new_state = !orch.auto_review_enabled;
        orch.auto_review_enabled = new_state;
        new_state
    };

    let status = if new_state { "✅ 已开启" } else { "❌ 已关闭" };
    app.chat_history.push(crate::app::ChatEntry::assistant(
        format!("**自动代码审查** {} 自动审查", status),
    ));
}

/// 显示帮助
fn show_help(app: &mut App) {
    app.chat_history.push(crate::app::ChatEntry::assistant(
        "\
/review 命令 — 代码审查

**用法:**
- `/review` — 审查当前对话中涉及的代码变更
- `/review <path>` — 审查指定文件或目录
- `/review --auto` 或 `/review -a` — 切换自动审查模式
- `/review --help` 或 `/review -h` — 显示此帮助

**自动审查:**
当主 Agent 完成代码修改后，审查 Agent 会自动分析变更的代码。
可以通过 `/review --auto` 开启或关闭此功能。"
            .trim(),
    ));
}

/// 异步执行审查
fn spawn_review(app: &mut App, path: Option<String>) {
    let orchestrator = match app.orchestrator.clone() {
        Some(o) => o,
        None => {
            app.chat_history.push(crate::app::ChatEntry::assistant(
                "⚠️ 审查系统未初始化。请重启应用。".to_string(),
            ));
            return;
        }
    };

    // 从 chat_history 取快照
    let history_snapshot: Vec<crate::app::ChatEntry> = app.chat_history.clone();

    let (result_tx, result_rx) = tokio::sync::mpsc::channel::<String>(1);
    let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel::<ReviewEvent>();

    app.review_event_rx = Some(event_rx);
    app.is_reviewing = true;

    // 保存 result_rx 以便后续检查结果
    app.review_result_rx = Some(result_rx);

    tokio::spawn(async move {
        let changed_files = if let Some(ref path) = path {
            vec![ChangedFile {
                path: path.clone(),
                change_type: ChangeType::Modified,
                lines_added: 0,
                lines_removed: 0,
                diff: String::new(),
            }]
        } else {
            // 从对话历史快照中检测变更
            let messages: Vec<crate::core::types::Message> = history_snapshot
                .iter()
                .map(|e| crate::core::types::Message {
                    role: e.role.clone(),
                    content: e.content.clone(),
                    reasoning_content: e.reasoning_content.clone(),
                    tool_calls: e.tool_calls.clone(),
                    tool_call_id: e.tool_call_id.clone(),
                })
                .collect();
            orchestrator.detect_changed_files(&messages)
        };

        if changed_files.is_empty() {
            let msg = if path.is_some() {
                "未找到指定路径的代码文件。请确认路径是否正确。".to_string()
            } else {
                "未检测到需要审查的代码变更。请先让主 Agent 修改代码，或使用 `/review <path>` 指定路径。"
                    .to_string()
            };
            let _ = result_tx.send(msg.clone()).await;
            let _ = event_tx.send(ReviewEvent::Error {
                message: msg,
            });
            return;
        }

        let _ = event_tx.send(ReviewEvent::Started {
            file_count: changed_files.len(),
        });

        match orchestrator.review(changed_files, None).await {
            Ok(report) => {
                let output = orchestrator.format_review_report(&report);
                let _ = result_tx.send(output).await;
                let _ = event_tx.send(ReviewEvent::Completed { report });
            }
            Err(e) => {
                let err_msg = format!("⚠️ 审查失败: {}", e);
                let _ = result_tx.send(err_msg).await;
                let err_str: String = format!("{}", e);
                let _ = event_tx.send(ReviewEvent::Error {
                    message: err_str,
                });
            }
        }
    });
}

// Re-export for app
pub use crate::core::agent::review_agent::ReviewEvent;
