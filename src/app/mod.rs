use std::sync::Arc;
use tokio::sync::mpsc;
use tui_textarea::TextArea;
use crate::core::config::Config;
use crate::core::token_usage::TokenUsage;
use crate::core::streaming::{StreamResult, StreamEvent};
use crate::core::preamble::Agent;

pub mod ui;
pub mod event_handler;
pub mod conversion;

/// Application state
pub struct App {
    pub chat_history: Vec<(String, String)>,
    pub current_response: String,
    pub input: TextArea<'static>,
    pub scroll: u16,
    pub total_lines: u16,
    pub token_usage: TokenUsage,
    pub last_reasoning: String,
    pub config: Config,
    pub should_exit: bool,
    pub is_streaming: bool,
    pub response_rx: Option<mpsc::Receiver<StreamResult>>,
    pub streaming_events_rx: Option<mpsc::UnboundedReceiver<StreamEvent>>,
    pub streaming_text: String,
    pub streaming_reasoning: String,
    pub status_messages: Vec<String>,
    pub turn_usage_line: Option<String>,
    /// Agent for processing requests
    pub agent: Arc<Agent>,
    /// Broadcast channel for interrupt signals
    pub interrupt_tx: tokio::sync::broadcast::Sender<()>,
    /// 是否显示思考区域
    pub show_reasoning: bool,
    /// 思考区域的滚动位置
    pub reasoning_scroll: u16,
    /// 思考区域的总行数
    pub reasoning_total_lines: u16,
    /// 是否自动滚动到最新内容
    pub auto_scroll: bool,
    /// 思考区域是否自动滚动
    pub reasoning_auto_scroll: bool,
    /// 是否在启动时显示 banner（首次发送消息后隐藏）
    pub show_banner: bool,
    /// 跑马灯动画帧计数器
    pub marquee_frame: u64,
    // === 补全菜单相关状态 ===
    /// 是否显示补全菜单
    pub show_completion: bool,
    /// 补全项列表
    pub completion_items: Vec<String>,
    /// 当前选中的补全项索引
    pub completion_selected: usize,
    /// 补全类型：'/' 命令补全 或 '@' 文件补全
    pub completion_type: Option<char>,
    /// 补全查询字符串（用于过滤）
    pub completion_query: String,
    /// 补全触发位置（光标位置）
    pub completion_trigger_pos: usize,
    /// 聊天区域高度
    pub chat_area_height: u16,
             // === 模型选择器相关状态 ===
     /// 是否显示模型选择器
     pub show_model_picker: bool,
     /// 可选的模型列表
     pub model_options: Vec<String>,
     /// 当前选中的模型索引
     pub model_selected: usize,
     // === Provider 选择器相关状态 ===
     /// 是否显示 provider 选择器
     pub show_provider_picker: bool,
     /// 可选的 provider 列表
     pub provider_options: Vec<String>,
     /// 当前选中的 provider 索引
     pub provider_selected: usize,
}

impl App {
    /// Create a new App instance
    pub fn new(
        chat_history: Vec<(String, String)>,
        token_usage: TokenUsage,
        last_reasoning: String,
        config: Config,
        agent: Arc<Agent>,
        interrupt_tx: tokio::sync::broadcast::Sender<()>,
    ) -> Self {
        let show_banner = chat_history.is_empty();
        let mut input_area = TextArea::default();
        input_area.set_block(
            ratatui::widgets::Block::default()
                .borders(ratatui::widgets::Borders::ALL)
                .title(" Input (Enter to send, Shift+Enter for newline, Esc: interrupt/exit) ")
        );
        input_area.set_cursor_line_style(ratatui::style::Style::default());

        App {
            chat_history,
            current_response: String::new(),
            input: input_area,
            scroll: 0,
            total_lines: 0,
            token_usage,
            last_reasoning,
            config: config.clone(),
            should_exit: false,
            is_streaming: false,
            response_rx: None,
            streaming_events_rx: None,
            streaming_text: String::new(),
            streaming_reasoning: String::new(),
            status_messages: Vec::new(),
            turn_usage_line: None,
            agent,
            interrupt_tx,
            show_reasoning: true,
            reasoning_scroll: 0,
            reasoning_total_lines: 0,
            auto_scroll: true,
            reasoning_auto_scroll: true,
            show_banner,
            marquee_frame: 0,
            // 补全菜单初始化
            show_completion: false,
            completion_items: Vec::new(),
            completion_selected: 0,
            completion_type: None,
            completion_query: String::new(),
            completion_trigger_pos: 0,
            chat_area_height: 0,
             // 模型选择器初始化
             show_model_picker: false,
             model_options: get_model_options_for_provider(&config.llm.provider),
             model_selected: 0,
             // Provider 选择器初始化
             show_provider_picker: false,
             provider_options: vec!["deepseek".to_string(), "openrouter".to_string()],
             provider_selected: {
                 let p = config.llm.provider.as_str();
                 if p == "openrouter" { 1 } else { 0 }
             },
         }
    }
}

/// 根据 provider 返回对应的模型选项列表
pub fn get_model_options_for_provider(provider: &str) -> Vec<String> {
    match provider {
        "deepseek" => vec![
            "deepseek-chat".to_string(),
            "deepseek-reasoner".to_string(),
        ],
        "openrouter" => vec![
            // OpenRouter 免费模型
            "nvidia/nemotron-3-super-120b-a12b:free".to_string(),
            "tencent/hy3-preview:free".to_string(),
            "meta/llama-3.1-405b-instruct:free".to_string(),
            "openai/gpt-4o-mini:free".to_string(),
            // Poolside 免费模型
            "poolside/laguna-m.1:free".to_string(),
            "poolside/laguna-xs.2:free".to_string(),
        ],
        _ => vec![
            "deepseek-chat".to_string(),
            "deepseek-reasoner".to_string(),
        ],
    }
}
