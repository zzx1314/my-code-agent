use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

/// Connection status enum
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionStatus {
    Unknown = 0,
    Connecting = 1,
    Connected = 2,
    Disconnected = 3,
    Error = 4,
}

/// Thread-safe connection state manager
#[derive(Clone)]
pub struct ConnectionState {
    status: Arc<AtomicU8>,
}

impl ConnectionState {
    /// Create a new connection state manager (initial status: Unknown)
    pub fn new() -> Self {
        Self {
            status: Arc::new(AtomicU8::new(ConnectionStatus::Unknown as u8)),
        }
    }

    /// Set the connection status
    pub fn set(&self, status: ConnectionStatus) {
        self.status.store(status as u8, Ordering::Release);
    }

    /// Get the current connection status
    pub fn get(&self) -> ConnectionStatus {
        match self.status.load(Ordering::Acquire) {
            1 => ConnectionStatus::Connecting,
            2 => ConnectionStatus::Connected,
            3 => ConnectionStatus::Disconnected,
            4 => ConnectionStatus::Error,
            _ => ConnectionStatus::Unknown,
        }
    }
}

impl Default for ConnectionState {
    fn default() -> Self {
        Self::new()
    }
}
