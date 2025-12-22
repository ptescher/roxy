//! Application state and message types for Roxy UI
//!
//! This module contains the core state management types including
//! the application state, UI messages, and proxy status.

use crate::components::DetailTab;
use gpui::ScrollHandle;
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

/// Default sidebar width in pixels
pub const DEFAULT_SIDEBAR_WIDTH: f32 = 220.0;
/// Minimum sidebar width in pixels
pub const MIN_SIDEBAR_WIDTH: f32 = 150.0;
/// Maximum sidebar width in pixels
pub const MAX_SIDEBAR_WIDTH: f32 = 400.0;

/// Default detail panel height in pixels
pub const DEFAULT_DETAIL_PANEL_HEIGHT: f32 = 300.0;
/// Minimum detail panel height in pixels
pub const MIN_DETAIL_PANEL_HEIGHT: f32 = 100.0;
/// Maximum detail panel height in pixels
pub const MAX_DETAIL_PANEL_HEIGHT: f32 = 600.0;

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

    /// Currently active tab in detail panel
    pub active_detail_tab: DetailTab,

    /// Error message to display in status bar
    pub error_message: Option<String>,

    /// Available update version (if any)
    pub update_available: Option<String>,

    /// Channel receiver for UI messages from background tasks
    pub message_rx: Option<mpsc::UnboundedReceiver<UiMessage>>,

    /// Channel sender for UI messages (cloned for background tasks)
    pub message_tx: mpsc::UnboundedSender<UiMessage>,

    /// Current sidebar width in pixels (resizable)
    pub sidebar_width: f32,

    /// Current detail panel height in pixels (resizable)
    pub detail_panel_height: f32,

    /// Whether the sidebar is currently being resized
    pub is_resizing_sidebar: bool,

    /// Whether the detail panel is currently being resized
    pub is_resizing_detail_panel: bool,

    /// Scroll handle for the request list
    pub request_list_scroll_handle: ScrollHandle,

    /// Scroll handle for the sidebar hosts
    pub sidebar_scroll_handle: ScrollHandle,

    /// Scroll handle for the detail panel content
    pub detail_panel_scroll_handle: ScrollHandle,
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
            active_detail_tab: DetailTab::default(),
            error_message: None,
            update_available: None,
            message_rx: Some(message_rx),
            message_tx,
            sidebar_width: DEFAULT_SIDEBAR_WIDTH,
            detail_panel_height: DEFAULT_DETAIL_PANEL_HEIGHT,
            is_resizing_sidebar: false,
            is_resizing_detail_panel: false,
            request_list_scroll_handle: ScrollHandle::new(),
            sidebar_scroll_handle: ScrollHandle::new(),
            detail_panel_scroll_handle: ScrollHandle::new(),
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

    /// Set the active detail tab
    pub fn set_detail_tab(&mut self, tab: DetailTab) {
        self.active_detail_tab = tab;
    }

    /// Set the sidebar width, clamping to min/max bounds
    pub fn set_sidebar_width(&mut self, width: f32) {
        self.sidebar_width = width.clamp(MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH);
    }

    /// Set the detail panel height, clamping to min/max bounds
    pub fn set_detail_panel_height(&mut self, height: f32) {
        self.detail_panel_height = height.clamp(MIN_DETAIL_PANEL_HEIGHT, MAX_DETAIL_PANEL_HEIGHT);
    }

    /// Start resizing the sidebar
    pub fn start_resizing_sidebar(&mut self) {
        self.is_resizing_sidebar = true;
    }

    /// Stop resizing the sidebar
    pub fn stop_resizing_sidebar(&mut self) {
        self.is_resizing_sidebar = false;
    }

    /// Start resizing the detail panel
    pub fn start_resizing_detail_panel(&mut self) {
        self.is_resizing_detail_panel = true;
    }

    /// Stop resizing the detail panel
    pub fn stop_resizing_detail_panel(&mut self) {
        self.is_resizing_detail_panel = false;
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
