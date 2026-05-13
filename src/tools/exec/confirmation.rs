use tokio::sync::oneshot;

/// A request sent from a tool to the UI to ask for user confirmation.
pub struct ConfirmationRequest {
    pub reason: String,
    pub detail: String,
    pub response_tx: oneshot::Sender<bool>,
}

/// A handle that tools can use to request user confirmation.
/// Cloning creates a new handle pointing to the same channel.
#[derive(Clone, Debug)]
pub struct ConfirmationHandle {
    request_tx: tokio::sync::mpsc::UnboundedSender<ConfirmationRequest>,
}

impl ConfirmationHandle {
    /// Create a new confirmation channel pair.
    /// Returns (handle, receiver) — the receiver should be given to the UI event loop.
    pub fn new() -> (
        Self,
        tokio::sync::mpsc::UnboundedReceiver<ConfirmationRequest>,
    ) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        (Self { request_tx: tx }, rx)
    }

    /// Create a disabled handle that always auto-denies (for testing / non-TUI mode).
    pub fn disabled() -> Self {
        // Use a closed channel so `send` always fails → confirm returns `false`.
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        // Drop the receiver immediately so all sends fail.
        std::mem::drop(_rx);
        Self { request_tx: tx }
    }

    /// Send a confirmation request and wait for the user's response.
    /// Returns `true` if the user confirmed, `false` otherwise.
    pub async fn confirm(&self, reason: &str, detail: &str) -> bool {
        let (response_tx, response_rx) = oneshot::channel();
        let req = ConfirmationRequest {
            reason: reason.to_string(),
            detail: detail.to_string(),
            response_tx,
        };
        if self.request_tx.send(req).is_err() {
            // Receiver dropped — no UI available, deny by default.
            return false;
        }
        response_rx.await.unwrap_or(false)
    }
}
