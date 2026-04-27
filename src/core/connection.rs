use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

/// 连接状态枚举
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ConnectionStatus {
    Unknown = 0,      // 未知状态（初始状态）
    Connecting = 1,   // 正在连接
    Connected = 2,    // 已连接
    Disconnected = 3, // 连接断开
    Error = 4,        // 连接错误
}

impl ConnectionStatus {
    /// 获取状态对应的 emoji 图标
    pub fn emoji(&self) -> &'static str {
        match self {
            ConnectionStatus::Unknown => "⚪",
            ConnectionStatus::Connecting => "🟡",
            ConnectionStatus::Connected => "🟢",
            ConnectionStatus::Disconnected => "🔴",
            ConnectionStatus::Error => "❌",
        }
    }

    /// 获取状态对应的文本描述
    pub fn text(&self) -> &'static str {
        match self {
            ConnectionStatus::Unknown => "Unknown",
            ConnectionStatus::Connecting => "Connecting...",
            ConnectionStatus::Connected => "Connected",
            ConnectionStatus::Disconnected => "Disconnected",
            ConnectionStatus::Error => "Connection Error",
        }
    }

    /// 获取状态的简短描述（用于提示符）
    pub fn short_text(&self) -> &'static str {
        match self {
            ConnectionStatus::Unknown => "?",
            ConnectionStatus::Connecting => "...",
            ConnectionStatus::Connected => "OK",
            ConnectionStatus::Disconnected => "OFF",
            ConnectionStatus::Error => "ERR",
        }
    }
}

/// 线程安全的连接状态管理器
#[derive(Clone)]
pub struct ConnectionState {
    status: Arc<AtomicU8>,
}

impl ConnectionState {
    /// 创建新的连接状态管理器
    pub fn new() -> Self {
        Self {
            status: Arc::new(AtomicU8::new(ConnectionStatus::Unknown as u8)),
        }
    }

    /// 设置连接状态
    pub fn set(&self, status: ConnectionStatus) {
        self.status.store(status as u8, Ordering::SeqCst);
    }

    /// 获取当前连接状态
    pub fn get(&self) -> ConnectionStatus {
        match self.status.load(Ordering::SeqCst) {
            1 => ConnectionStatus::Connecting,
            2 => ConnectionStatus::Connected,
            3 => ConnectionStatus::Disconnected,
            4 => ConnectionStatus::Error,
            _ => ConnectionStatus::Unknown,
        }
    }

    /// 标记为正在连接
    pub fn set_connecting(&self) {
        self.set(ConnectionStatus::Connecting);
    }

    /// 标记为已连接
    pub fn set_connected(&self) {
        self.set(ConnectionStatus::Connected);
    }

    /// 标记为断开连接
    pub fn set_disconnected(&self) {
        self.set(ConnectionStatus::Disconnected);
    }

    /// 标记为连接错误
    pub fn set_error(&self) {
        self.set(ConnectionStatus::Error);
    }

    /// 检查是否已连接
    pub fn is_connected(&self) -> bool {
        self.get() == ConnectionStatus::Connected
    }

    /// 检查是否正在连接
    pub fn is_connecting(&self) -> bool {
        self.get() == ConnectionStatus::Connecting
    }
}

impl Default for ConnectionState {
    fn default() -> Self {
        Self::new()
    }
}
