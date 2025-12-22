//! Application state and message types for Roxy UI
//!
//! This module contains the core state management types including
//! the application state, UI messages, and proxy status.

use roxy_core::{ClickHouseConfig, HostSummary, HttpRequestRecord, RoxyClickHouse};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tokio::sync::mpsc;

/// Messages from background tasks to the UI
#[derive(Debug)]
pub enum UiMessage {
    /// New requests fetched from ClickHouse
    RequestsUpdated(Vec<HttpRequestRecord>),
    /// New hosts summary fetched from ClickHouse
    HostsUpdated(Vec<HostSummary>),
    /// Proxy server started successfully
    ProxyStarted,
    /// Proxy server failed to start
    ProxyFailed(String),
    /// General error message
    Error(String),
}

/// Proxy server status
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProxyStatus {
    /// Proxy is stopped
    Stopped,
    /// Proxy is starting up
    Starting,
    /// Proxy is running and accepting connections
    Running,
    /// Proxy failed to start with error message
    Failed(String),
}

impl ProxyStatus {
    /// Check if the proxy is currently running
    pub fn is_running(&self) -> bool {
        matches!(self, ProxyStatus::Running)
    }

    /// Check if the proxy is in a failed state
    pub fn is_failed(&self) -> bool {
        matches!(self, ProxyStatus::Failed(_))
    }

    /// Get the status text for display
    pub fn text(&self) -> &str {
        match self {
            ProxyStatus::Stopped => "Stopped",
            ProxyStatus::Starting => "Starting...",
            ProxyStatus::Running => "Running",
            ProxyStatus::Failed(_) => "Failed",
        }
    }
}

/// Application state shared across the UI
pub struct AppState {
    /// ClickHouse client for querying data
    pub clickhouse: RoxyClickHouse,

    /// Whether the proxy is currently running (shared with proxy thread)
    pub proxy_running: Arc<AtomicBool>,

    /// Current proxy status
    pub proxy_status: ProxyStatus,

    /// List of hosts from ClickHouse
    pub hosts: Vec<HostSummary>,

    /// List of recent requests
    pub requests: Vec<HttpRequestRecord>,

    /// Currently selected host filter
    pub selected_host: Option<String>,

    /// Currently selected request for detail view
    pub selected_request: Option<HttpRequestRecord>,

    /// Error message to display in status bar
    pub error_message: Option<String>,

    /// Available update version (if any)
    pub update_available: Option<String>,

    /// Channel receiver for UI messages from background tasks
    pub message_rx: Option<mpsc::UnboundedReceiver<UiMessage>>,

    /// Channel sender for UI messages (cloned for background tasks)
    pub message_tx: mpsc::UnboundedSender<UiMessage>,
}

impl AppState {
    /// Create a new application state with default values
    pub fn new() -> Self {
        let clickhouse = RoxyClickHouse::new(ClickHouseConfig::default());
        let (message_tx, message_rx) = mpsc::unbounded_channel();

        Self {
            clickhouse,
            proxy_running: Arc::new(AtomicBool::new(false)),
            proxy_status: ProxyStatus::Stopped,
            hosts: Vec::new(),
            requests: Vec::new(),
            selected_host: None,
            selected_request: None,
            error_message: None,
            update_available: None,
            message_rx: Some(message_rx),
            message_tx,
        }
    }

    /// Get the message sender for background tasks
    pub fn message_sender(&self) -> mpsc::UnboundedSender<UiMessage> {
        self.message_tx.clone()
    }

    /// Process all pending messages from background tasks
    pub fn process_messages(&mut self) {
        // Take the receiver temporarily
        let mut rx = match self.message_rx.take() {
            Some(rx) => rx,
            None => return,
        };

        // Process all available messages
        while let Ok(msg) = rx.try_recv() {
            self.handle_message(msg);
        }

        // Put the receiver back
        self.message_rx = Some(rx);
    }

    /// Handle a single UI message
    fn handle_message(&mut self, msg: UiMessage) {
        match msg {
            UiMessage::RequestsUpdated(requests) => {
                self.requests = requests;
            }
            UiMessage::HostsUpdated(hosts) => {
                self.hosts = hosts;
            }
            UiMessage::ProxyStarted => {
                self.proxy_status = ProxyStatus::Running;
                self.error_message = None;
            }
            UiMessage::ProxyFailed(err) => {
                self.proxy_status = ProxyStatus::Failed(err.clone());
                self.error_message = Some(err);
            }
            UiMessage::Error(err) => {
                self.error_message = Some(err);
            }
        }
    }

    /// Get the number of captured requests
    pub fn request_count(&self) -> usize {
        self.requests.len()
    }

    /// Get the number of unique hosts
    pub fn host_count(&self) -> usize {
        self.hosts.len()
    }

    /// Clear all captured requests and hosts
    pub fn clear(&mut self) {
        self.requests.clear();
        self.hosts.clear();
        self.selected_request = None;
        self.selected_host = None;
    }

    /// Select a request for detail view
    pub fn select_request(&mut self, request: HttpRequestRecord) {
        self.selected_request = Some(request);
    }

    /// Clear the selected request
    pub fn clear_selected_request(&mut self) {
        self.selected_request = None;
    }

    /// Select a host filter
    pub fn select_host(&mut self, host: String) {
        self.selected_host = Some(host);
    }

    /// Clear the host filter
    pub fn clear_host_filter(&mut self) {
        self.selected_host = None;
    }

    /// Get filtered requests based on selected host
    pub fn filtered_requests(&self) -> Vec<&HttpRequestRecord> {
        match &self.selected_host {
            Some(host) => self.requests.iter().filter(|r| &r.host == host).collect(),
            None => self.requests.iter().collect(),
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_proxy_status_text() {
        assert_eq!(ProxyStatus::Stopped.text(), "Stopped");
        assert_eq!(ProxyStatus::Starting.text(), "Starting...");
        assert_eq!(ProxyStatus::Running.text(), "Running");
        assert_eq!(ProxyStatus::Failed("error".into()).text(), "Failed");
    }

    #[test]
    fn test_proxy_status_is_running() {
        assert!(!ProxyStatus::Stopped.is_running());
        assert!(!ProxyStatus::Starting.is_running());
        assert!(ProxyStatus::Running.is_running());
        assert!(!ProxyStatus::Failed("error".into()).is_running());
    }

    #[test]
    fn test_app_state_new() {
        let state = AppState::new();
        assert_eq!(state.proxy_status, ProxyStatus::Stopped);
        assert!(state.requests.is_empty());
        assert!(state.hosts.is_empty());
        assert!(state.selected_request.is_none());
        assert!(state.error_message.is_none());
    }

    #[test]
    fn test_app_state_clear() {
        let mut state = AppState::new();
        state.error_message = Some("test".into());
        state.clear();
        assert!(state.requests.is_empty());
        assert!(state.hosts.is_empty());
    }
}
