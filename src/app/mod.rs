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
                .title(" Input (Enter to send, Shift+Enter for newline, Esc to exit) ")
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
        }
    }
}
