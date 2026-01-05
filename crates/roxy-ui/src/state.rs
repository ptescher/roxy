//! Application state and message types for Roxy UI
//!
//! This module contains the core state management types including
//! the application state, UI messages, and proxy status.

use crate::components::{
    ConnectionDetailTab, DatabaseDetailTab, DetailTab, HttpRouteInfo, K8sBackendRef, K8sGateway,
    K8sGatewayListener, K8sHttpRoute, K8sIngress, K8sParentRef, K8sService, K8sServicePort,
    KubeContext, KubeNamespace, LeftDockTab, MessagingDetailTab, PortForwardInfo, ServiceSummary,
};
use gpui::ScrollHandle;
use k8s_openapi::api::core::v1::{Namespace, Service};
use k8s_openapi::api::networking::v1::Ingress;
use kube::api::{DynamicObject, GroupVersionKind};
use kube::config::Kubeconfig;
use kube::discovery::ApiResource;
use kube::{Api, Client, Config};
use roxy_core::{
    ClickHouseConfig, DatabaseQueryRow, HostSummary, HttpRequestRecord, KafkaMessageRow,
    RoxyClickHouse, TcpConnectionRow,
};
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

// =============================================================================
// Port Forwards Config File
// =============================================================================

/// Port forwards configuration file format
#[derive(Debug, Clone, Deserialize)]
struct PortForwardsConfig {
    port_forwards: Vec<PortForwardEntry>,
}

/// A single port forward entry in the config file
#[derive(Debug, Clone, Deserialize)]
struct PortForwardEntry {
    name: String,
    namespace: String,
    service: String,
    remote_port: u16,
    local_port: u16,
    #[serde(default)]
    auto_start: bool,
}

impl From<PortForwardEntry> for PortForwardInfo {
    fn from(entry: PortForwardEntry) -> Self {
        Self {
            service_dns: format!("{}.{}.svc.cluster.local", entry.service, entry.namespace),
            namespace: entry.namespace,
            service_name: entry.service,
            remote_port: entry.remote_port,
            local_port: entry.local_port,
            active: entry.auto_start,
        }
    }
}

/// Load port forwards from config file
///
/// Searches for config in:
/// 1. ~/.roxy/port-forwards.yaml
/// 2. ./roxy-config/port-forwards.yaml (for development)
pub fn load_port_forwards_config() -> Vec<PortForwardInfo> {
    let config_paths = [
        dirs::home_dir().map(|h| h.join(".roxy").join("port-forwards.yaml")),
        Some(PathBuf::from("roxy-config/port-forwards.yaml")),
        Some(PathBuf::from(
            "examples/fullstack-k8s-dev/roxy-config/port-forwards.yaml",
        )),
    ];

    for path in config_paths.iter().flatten() {
        if path.exists() {
            match std::fs::read_to_string(path) {
                Ok(contents) => match serde_yaml::from_str::<PortForwardsConfig>(&contents) {
                    Ok(config) => {
                        info!(
                            "Loaded {} port forwards from {:?}",
                            config.port_forwards.len(),
                            path
                        );
                        return config.port_forwards.into_iter().map(Into::into).collect();
                    }
                    Err(e) => {
                        warn!("Failed to parse port forwards config {:?}: {}", path, e);
                    }
                },
                Err(e) => {
                    debug!("Could not read {:?}: {}", path, e);
                }
            }
        }
    }

    debug!("No port forwards config file found");
    Vec::new()
}

/// The main view mode for the application
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewMode {
    /// Show HTTP requests
    #[default]
    Requests,
    /// Show database queries (PostgreSQL, MySQL, etc.)
    Database,
    /// Show messaging (Kafka, RabbitMQ, etc.)
    Messaging,
    /// Show raw TCP connections (unknown protocols)
    TCP,
    /// Show Kubernetes overview
    Kubernetes,
}

impl ViewMode {
    pub fn label(&self) -> &'static str {
        match self {
            ViewMode::Requests => "HTTP",
            ViewMode::Database => "Database",
            ViewMode::Messaging => "Messaging",
            ViewMode::TCP => "TCP",
            ViewMode::Kubernetes => "Kubernetes",
        }
    }
}

/// Messages from background tasks to the UI
#[derive(Debug)]
pub enum UiMessage {
    /// New requests fetched from ClickHouse
    RequestsUpdated(Vec<HttpRequestRecord>),
    /// New hosts summary fetched from ClickHouse
    HostsUpdated(Vec<HostSummary>),
    /// New TCP connections fetched from ClickHouse
    ConnectionsUpdated(Vec<TcpConnectionRow>),
    /// New database queries fetched from ClickHouse
    DatabaseQueriesUpdated(Vec<DatabaseQueryRow>),
    /// New Kafka messages fetched from ClickHouse
    KafkaMessagesUpdated(Vec<KafkaMessageRow>),
    /// Proxy server started successfully
    ProxyStarted,
    /// Proxy server failed to start
    ProxyFailed(String),
    /// General error message
    Error(String),
    /// Kubernetes port forwards updated
    PortForwardsUpdated(Vec<PortForwardInfo>),
    /// Kubernetes HTTP routes updated
    HttpRoutesUpdated(Vec<HttpRouteInfo>),
    /// Kubernetes namespaces loaded
    KubeNamespacesLoaded(Vec<KubeNamespace>),
    /// Kubernetes services loaded
    KubeServicesLoaded(Vec<K8sService>),
    /// Kubernetes ingresses loaded
    KubeIngressesLoaded(Vec<K8sIngress>),
    /// Kubernetes Gateways loaded (Gateway API)
    KubeGatewaysLoaded(Vec<K8sGateway>),
    /// Kubernetes HTTPRoutes loaded
    KubeHttpRoutesLoaded(Vec<K8sHttpRoute>),
    /// Kubernetes resources loading complete
    KubeLoadingComplete,
    /// Kubernetes client failed to initialize
    KubeClientFailed(String),
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

    /// Whether the system proxy is enabled (routes all macOS traffic through Roxy)
    pub system_proxy_enabled: bool,

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

    /// Current view mode (Requests or Kubernetes)
    pub view_mode: ViewMode,

    /// Kubernetes Ingress resources
    pub k8s_ingresses: Vec<K8sIngress>,

    /// Kubernetes Gateway resources (Gateway API)
    pub k8s_gateways: Vec<K8sGateway>,

    /// Kubernetes HTTPRoute resources (Gateway API)
    pub k8s_http_routes: Vec<K8sHttpRoute>,

    /// Kubernetes Services
    pub k8s_services: Vec<K8sService>,

    /// Active Kubernetes port forwards from Roxy
    pub k8s_port_forwards: Vec<PortForwardInfo>,

    /// Legacy HTTP routes (kept for compatibility)
    pub k8s_http_routes_legacy: Vec<HttpRouteInfo>,

    /// Scroll handle for the Kubernetes panel
    pub kubernetes_scroll_handle: ScrollHandle,

    /// List of recent TCP connections (unknown protocols only)
    pub tcp_connections: Vec<TcpConnectionRow>,

    /// List of recent database queries (PostgreSQL, MySQL, etc.)
    pub database_queries: Vec<DatabaseQueryRow>,

    /// List of recent Kafka/messaging records
    pub kafka_messages: Vec<KafkaMessageRow>,

    /// Currently selected TCP connection for detail view
    pub selected_connection: Option<TcpConnectionRow>,

    /// Currently selected database query for detail view
    pub selected_database_query: Option<DatabaseQueryRow>,

    /// Currently selected Kafka message for detail view
    pub selected_kafka_message: Option<KafkaMessageRow>,

    /// Scroll handle for the connections list
    pub connections_scroll_handle: ScrollHandle,

    /// Scroll handle for the database query list
    pub database_list_scroll_handle: ScrollHandle,

    /// Scroll handle for the messaging list
    pub messaging_list_scroll_handle: ScrollHandle,

    /// Currently active tab in connection detail panel
    pub active_connection_detail_tab: ConnectionDetailTab,

    /// Currently active tab in database detail panel
    pub active_database_detail_tab: DatabaseDetailTab,

    /// Currently active tab in messaging detail panel
    pub active_messaging_detail_tab: MessagingDetailTab,

    /// Scroll handle for connection detail panel
    pub connection_detail_scroll_handle: ScrollHandle,

    /// Scroll handle for database detail panel
    pub database_detail_scroll_handle: ScrollHandle,

    /// Scroll handle for messaging detail panel
    pub messaging_detail_scroll_handle: ScrollHandle,

    // =========================================================================
    // Left Dock State
    // =========================================================================
    /// Currently active tab in the left dock
    pub left_dock_tab: LeftDockTab,

    /// List of connected services/apps
    pub services: Vec<ServiceSummary>,

    /// Currently selected service filter
    pub selected_service: Option<String>,

    /// Scroll handle for services list
    pub services_scroll_handle: ScrollHandle,

    /// Available Kubernetes contexts
    pub kube_contexts: Vec<KubeContext>,

    /// Currently selected Kubernetes context
    pub selected_kube_context: Option<String>,

    /// Kubernetes namespaces for the selected context
    pub kube_namespaces: Vec<KubeNamespace>,

    /// Currently selected namespace (None = all namespaces)
    pub selected_kube_namespace: Option<String>,

    /// Scroll handle for kubernetes namespace list in left dock
    pub kube_namespaces_scroll_handle: ScrollHandle,

    /// Whether the context dropdown in the Kubernetes tab is expanded
    pub context_dropdown_expanded: bool,

    /// Whether Kubernetes resources are currently loading
    pub kube_loading: bool,
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
            system_proxy_enabled: false,
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
            view_mode: ViewMode::default(),
            k8s_ingresses: Vec::new(),
            k8s_gateways: Vec::new(),
            k8s_http_routes: Vec::new(),
            k8s_services: Vec::new(),
            k8s_port_forwards: load_port_forwards_config(),
            k8s_http_routes_legacy: Vec::new(),
            kubernetes_scroll_handle: ScrollHandle::new(),
            tcp_connections: Vec::new(),
            database_queries: Vec::new(),
            kafka_messages: Vec::new(),
            selected_connection: None,
            selected_database_query: None,
            selected_kafka_message: None,
            connections_scroll_handle: ScrollHandle::new(),
            database_list_scroll_handle: ScrollHandle::new(),
            messaging_list_scroll_handle: ScrollHandle::new(),
            active_connection_detail_tab: ConnectionDetailTab::default(),
            active_database_detail_tab: DatabaseDetailTab::default(),
            active_messaging_detail_tab: MessagingDetailTab::default(),
            connection_detail_scroll_handle: ScrollHandle::new(),
            database_detail_scroll_handle: ScrollHandle::new(),
            messaging_detail_scroll_handle: ScrollHandle::new(),
            // Left dock state
            left_dock_tab: LeftDockTab::default(),
            services: Vec::new(),
            selected_service: None,
            services_scroll_handle: ScrollHandle::new(),
            kube_contexts: Vec::new(),
            selected_kube_context: None,
            kube_namespaces: Vec::new(),
            selected_kube_namespace: None,
            kube_namespaces_scroll_handle: ScrollHandle::new(),
            context_dropdown_expanded: false,
            kube_loading: false,
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
            UiMessage::ConnectionsUpdated(connections) => {
                self.tcp_connections = connections;
            }
            UiMessage::DatabaseQueriesUpdated(queries) => {
                self.database_queries = queries;
            }
            UiMessage::KafkaMessagesUpdated(messages) => {
                self.kafka_messages = messages;
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
            UiMessage::PortForwardsUpdated(forwards) => {
                self.k8s_port_forwards = forwards;
            }
            UiMessage::HttpRoutesUpdated(routes) => {
                self.k8s_http_routes_legacy = routes;
            }
            UiMessage::KubeNamespacesLoaded(namespaces) => {
                self.kube_namespaces = namespaces;
            }
            UiMessage::KubeServicesLoaded(services) => {
                self.k8s_services = services;
            }
            UiMessage::KubeIngressesLoaded(ingresses) => {
                self.k8s_ingresses = ingresses;
            }
            UiMessage::KubeGatewaysLoaded(gateways) => {
                self.k8s_gateways = gateways;
            }
            UiMessage::KubeHttpRoutesLoaded(routes) => {
                self.k8s_http_routes = routes;
            }
            UiMessage::KubeLoadingComplete => {
                self.kube_loading = false;
                debug!("Kubernetes resources loaded");
            }
            UiMessage::KubeClientFailed(err) => {
                self.kube_loading = false;
                self.error_message = Some(format!("Kubernetes: {}", err));
                warn!("Kubernetes client failed: {}", err);
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

    /// Start resizing the sidebar/left dock
    pub fn start_resizing_sidebar(&mut self) {
        self.is_resizing_sidebar = true;
    }

    /// Stop resizing the sidebar/left dock
    pub fn stop_resizing_sidebar(&mut self) {
        self.is_resizing_sidebar = false;
    }

    /// Start resizing the left dock (alias for sidebar)
    pub fn start_resizing_left_dock(&mut self) {
        self.is_resizing_sidebar = true;
    }

    /// Stop resizing the left dock (alias for sidebar)
    pub fn stop_resizing_left_dock(&mut self) {
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

    /// Set the current view mode
    pub fn set_view_mode(&mut self, mode: ViewMode) {
        self.view_mode = mode;
    }

    /// Toggle between view modes
    pub fn toggle_view_mode(&mut self) {
        self.view_mode = match self.view_mode {
            ViewMode::Requests => ViewMode::Database,
            ViewMode::Database => ViewMode::Messaging,
            ViewMode::Messaging => ViewMode::TCP,
            ViewMode::TCP => ViewMode::Kubernetes,
            ViewMode::Kubernetes => ViewMode::Requests,
        };
    }

    /// Update the Kubernetes port forwards
    pub fn set_port_forwards(&mut self, forwards: Vec<PortForwardInfo>) {
        self.k8s_port_forwards = forwards;
    }

    /// Update the Kubernetes HTTP routes (legacy)
    pub fn set_http_routes(&mut self, routes: Vec<HttpRouteInfo>) {
        self.k8s_http_routes_legacy = routes;
    }

    /// Toggle system proxy on/off
    pub fn set_system_proxy_enabled(&mut self, enabled: bool) {
        self.system_proxy_enabled = enabled;
    }

    pub fn is_system_proxy_enabled(&self) -> bool {
        self.system_proxy_enabled
    }

    // =========================================================================
    // Left Dock Methods
    // =========================================================================

    /// Set the active left dock tab
    pub fn set_left_dock_tab(&mut self, tab: LeftDockTab) {
        self.left_dock_tab = tab;
    }

    /// Select a service to filter by
    pub fn select_service(&mut self, service: String) {
        self.selected_service = Some(service);
    }

    /// Clear the service filter
    pub fn clear_service_filter(&mut self) {
        self.selected_service = None;
    }

    /// Set the selected Kubernetes context and reload resources
    pub fn select_kube_context(&mut self, context: String) {
        self.selected_kube_context = Some(context.clone());
        // Clear namespace selection when context changes
        self.selected_kube_namespace = None;
        // Clear existing resources
        self.kube_namespaces.clear();
        self.k8s_services.clear();
        self.k8s_ingresses.clear();
        self.k8s_http_routes.clear();
        // Start loading
        self.kube_loading = true;
        // Spawn background thread with its own tokio runtime to load resources
        let tx = self.message_tx.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
            rt.block_on(load_kubernetes_resources(context, None, tx));
        });
    }

    /// Toggle the context dropdown expanded state
    pub fn toggle_context_dropdown(&mut self) {
        self.context_dropdown_expanded = !self.context_dropdown_expanded;
    }

    /// Set the selected Kubernetes namespace and reload namespace-specific resources
    pub fn select_kube_namespace(&mut self, namespace: Option<String>) {
        self.selected_kube_namespace = namespace.clone();

        // Reload resources for the new namespace
        if let Some(context) = self.selected_kube_context.clone() {
            let tx = self.message_tx.clone();
            std::thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
                rt.block_on(load_namespace_resources(context, namespace, tx));
            });
        }
    }

    /// Load Kubernetes contexts from the system kubeconfig
    pub fn load_kube_contexts(&mut self) {
        // Try to load kubeconfig from default location (~/.kube/config)
        match Kubeconfig::read() {
            Ok(kubeconfig) => {
                let current_context = kubeconfig.current_context.as_deref();
                debug!(
                    "Loaded kubeconfig with {} contexts, current: {:?}",
                    kubeconfig.contexts.len(),
                    current_context
                );

                self.kube_contexts = kubeconfig
                    .contexts
                    .iter()
                    .map(|named_ctx| {
                        let ctx = &named_ctx.context.as_ref();
                        let cluster_name = ctx
                            .map(|c| c.cluster.clone())
                            .unwrap_or_else(|| "unknown".to_string());

                        KubeContext {
                            name: named_ctx.name.clone(),
                            is_current: current_context == Some(named_ctx.name.as_str()),
                            cluster: cluster_name,
                        }
                    })
                    .collect();

                // Auto-select the current context and load its resources
                if let Some(current) = current_context {
                    self.selected_kube_context = Some(current.to_string());
                    self.kube_loading = true;
                    let tx = self.message_tx.clone();
                    let context = current.to_string();
                    std::thread::spawn(move || {
                        let rt =
                            tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
                        rt.block_on(load_kubernetes_resources(context, None, tx));
                    });
                }
            }
            Err(e) => {
                warn!("Failed to load kubeconfig: {}", e);
                // Leave contexts empty - user can see there's no k8s config
                self.kube_contexts = Vec::new();
                self.selected_kube_context = None;
            }
        }
    }

    /// Refresh Kubernetes resources for the current context and namespace
    pub fn refresh_kubernetes_resources(&mut self) {
        if let Some(context) = self.selected_kube_context.clone() {
            self.kube_loading = true;
            let namespace = self.selected_kube_namespace.clone();
            let tx = self.message_tx.clone();
            std::thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
                rt.block_on(load_kubernetes_resources(context, namespace, tx));
            });
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Kubernetes Loading Functions
// =============================================================================

/// Create a Kubernetes client for a specific context
async fn create_kube_client(context_name: &str) -> Result<Client, String> {
    let kubeconfig = Kubeconfig::read().map_err(|e| format!("Failed to read kubeconfig: {}", e))?;

    let config = Config::from_custom_kubeconfig(
        kubeconfig,
        &kube::config::KubeConfigOptions {
            context: Some(context_name.to_string()),
            ..Default::default()
        },
    )
    .await
    .map_err(|e| format!("Failed to create config: {}", e))?;

    Client::try_from(config).map_err(|e| format!("Failed to create client: {}", e))
}

/// Load all Kubernetes resources for a context
async fn load_kubernetes_resources(
    context: String,
    namespace: Option<String>,
    tx: mpsc::UnboundedSender<UiMessage>,
) {
    info!(
        "Loading Kubernetes resources for context: {}, namespace: {:?}",
        context, namespace
    );

    let client = match create_kube_client(&context).await {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to create Kubernetes client: {}", e);
            let _ = tx.send(UiMessage::KubeClientFailed(e));
            return;
        }
    };

    // Load namespaces first
    if let Err(e) = load_namespaces(&client, &tx).await {
        warn!("Failed to load namespaces: {}", e);
    }

    // Load namespace-scoped resources
    load_namespace_resources_with_client(&client, namespace, &tx).await;

    let _ = tx.send(UiMessage::KubeLoadingComplete);
}

/// Load namespace-specific resources (services, ingresses, routes)
async fn load_namespace_resources(
    context: String,
    namespace: Option<String>,
    tx: mpsc::UnboundedSender<UiMessage>,
) {
    let client = match create_kube_client(&context).await {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to create Kubernetes client: {}", e);
            let _ = tx.send(UiMessage::KubeClientFailed(e));
            return;
        }
    };

    load_namespace_resources_with_client(&client, namespace, &tx).await;
}

/// Load namespace-specific resources using an existing client
async fn load_namespace_resources_with_client(
    client: &Client,
    namespace: Option<String>,
    tx: &mpsc::UnboundedSender<UiMessage>,
) {
    // Load services
    if let Err(e) = load_services(client, namespace.as_deref(), tx).await {
        warn!("Failed to load services: {}", e);
    }

    // Load ingresses
    if let Err(e) = load_ingresses(client, namespace.as_deref(), tx).await {
        warn!("Failed to load ingresses: {}", e);
    }

    // Load Gateway API resources (Gateways and HTTPRoutes)
    // These are CRDs, so we use dynamic API and gracefully handle if they don't exist
    if let Err(e) = load_gateways(client, namespace.as_deref(), tx).await {
        debug!("Failed to load Gateways (CRD may not be installed): {}", e);
    }

    if let Err(e) = load_http_routes(client, namespace.as_deref(), tx).await {
        debug!(
            "Failed to load HTTPRoutes (CRD may not be installed): {}",
            e
        );
    }
}

/// Load Kubernetes namespaces
async fn load_namespaces(
    client: &Client,
    tx: &mpsc::UnboundedSender<UiMessage>,
) -> Result<(), kube::Error> {
    let namespaces_api: Api<Namespace> = Api::all(client.clone());
    let namespaces = namespaces_api.list(&Default::default()).await?;

    let mut kube_namespaces = Vec::new();

    for ns in namespaces.items {
        let name = ns.metadata.name.unwrap_or_default();
        if name.is_empty() {
            continue;
        }

        // Get service count for this namespace
        let services_api: Api<Service> = Api::namespaced(client.clone(), &name);
        let service_count = services_api
            .list(&Default::default())
            .await
            .map(|list| list.items.len())
            .unwrap_or(0);

        kube_namespaces.push(KubeNamespace {
            name,
            service_count: service_count as u32,
            pod_count: 0, // Skip pod count to avoid extra API calls
        });
    }

    // Sort: user namespaces first (alphabetically), then system namespaces
    kube_namespaces.sort_by(|a, b| {
        let a_system = a.name.starts_with("kube-") || a.name == "default";
        let b_system = b.name.starts_with("kube-") || b.name == "default";
        match (a_system, b_system) {
            (true, false) => std::cmp::Ordering::Greater,
            (false, true) => std::cmp::Ordering::Less,
            _ => a.name.cmp(&b.name),
        }
    });

    debug!("Loaded {} namespaces", kube_namespaces.len());
    let _ = tx.send(UiMessage::KubeNamespacesLoaded(kube_namespaces));

    Ok(())
}

/// Load Kubernetes services
async fn load_services(
    client: &Client,
    namespace: Option<&str>,
    tx: &mpsc::UnboundedSender<UiMessage>,
) -> Result<(), kube::Error> {
    let services: Vec<Service> = match namespace {
        Some(ns) => {
            let api: Api<Service> = Api::namespaced(client.clone(), ns);
            api.list(&Default::default()).await?.items
        }
        None => {
            let api: Api<Service> = Api::all(client.clone());
            api.list(&Default::default()).await?.items
        }
    };

    let k8s_services: Vec<K8sService> = services
        .into_iter()
        .filter_map(|svc| {
            let metadata = svc.metadata;
            let spec = svc.spec?;

            let name = metadata.name?;
            let namespace = metadata.namespace.unwrap_or_else(|| "default".to_string());

            let ports: Vec<K8sServicePort> = spec
                .ports
                .unwrap_or_default()
                .into_iter()
                .map(|p| {
                    let target_port = p
                        .target_port
                        .map(|tp| match tp {
                            k8s_openapi::apimachinery::pkg::util::intstr::IntOrString::Int(i) => {
                                i as u16
                            }
                            k8s_openapi::apimachinery::pkg::util::intstr::IntOrString::String(
                                _,
                            ) => p.port as u16,
                        })
                        .unwrap_or(p.port as u16);

                    K8sServicePort {
                        name: p.name,
                        port: p.port as u16,
                        target_port,
                        protocol: p.protocol.unwrap_or_else(|| "TCP".to_string()),
                    }
                })
                .collect();

            Some(K8sService {
                name,
                namespace,
                service_type: spec.type_.unwrap_or_else(|| "ClusterIP".to_string()),
                ports,
                ready_endpoints: 0, // Would need to query Endpoints resource
                total_endpoints: 0,
            })
        })
        .collect();

    debug!("Loaded {} services", k8s_services.len());
    let _ = tx.send(UiMessage::KubeServicesLoaded(k8s_services));

    Ok(())
}

/// Load Kubernetes ingresses
async fn load_ingresses(
    client: &Client,
    namespace: Option<&str>,
    tx: &mpsc::UnboundedSender<UiMessage>,
) -> Result<(), kube::Error> {
    let ingresses: Vec<Ingress> = match namespace {
        Some(ns) => {
            let api: Api<Ingress> = Api::namespaced(client.clone(), ns);
            api.list(&Default::default()).await?.items
        }
        None => {
            let api: Api<Ingress> = Api::all(client.clone());
            api.list(&Default::default()).await?.items
        }
    };

    let k8s_ingresses: Vec<K8sIngress> = ingresses
        .into_iter()
        .filter_map(|ing| {
            let metadata = ing.metadata;
            let spec = ing.spec?;

            let name = metadata.name?;
            let namespace = metadata.namespace.unwrap_or_else(|| "default".to_string());

            // Extract hosts from rules
            let hosts: Vec<String> = spec
                .rules
                .unwrap_or_default()
                .into_iter()
                .filter_map(|rule| rule.host)
                .collect();

            let ingress_class = spec.ingress_class_name;

            Some(K8sIngress {
                name,
                namespace,
                hosts,
                ingress_class,
            })
        })
        .collect();

    debug!("Loaded {} ingresses", k8s_ingresses.len());
    let _ = tx.send(UiMessage::KubeIngressesLoaded(k8s_ingresses));

    Ok(())
}

/// Load Kubernetes Gateway resources (Gateway API)
async fn load_gateways(
    client: &Client,
    namespace: Option<&str>,
    tx: &mpsc::UnboundedSender<UiMessage>,
) -> Result<(), kube::Error> {
    let gvk = GroupVersionKind::gvk("gateway.networking.k8s.io", "v1", "Gateway");
    let api_resource = ApiResource::from_gvk(&gvk);

    let gateways: Vec<DynamicObject> = match namespace {
        Some(ns) => {
            let api: Api<DynamicObject> = Api::namespaced_with(client.clone(), ns, &api_resource);
            api.list(&Default::default()).await?.items
        }
        None => {
            let api: Api<DynamicObject> = Api::all_with(client.clone(), &api_resource);
            api.list(&Default::default()).await?.items
        }
    };

    let k8s_gateways: Vec<K8sGateway> = gateways
        .into_iter()
        .filter_map(|gw| {
            let name = gw.metadata.name?;
            let namespace = gw
                .metadata
                .namespace
                .unwrap_or_else(|| "default".to_string());

            let spec = gw.data.get("spec")?.as_object()?;
            let gateway_class = spec
                .get("gatewayClassName")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let listeners: Vec<K8sGatewayListener> = spec
                .get("listeners")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|l| {
                            let obj = l.as_object()?;
                            Some(K8sGatewayListener {
                                name: obj.get("name")?.as_str()?.to_string(),
                                hostname: obj
                                    .get("hostname")
                                    .and_then(|h| h.as_str())
                                    .map(|s| s.to_string()),
                                port: obj.get("port")?.as_u64()? as u16,
                                protocol: obj.get("protocol")?.as_str()?.to_string(),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();

            Some(K8sGateway {
                name,
                namespace,
                gateway_class,
                listeners,
            })
        })
        .collect();

    debug!("Loaded {} gateways", k8s_gateways.len());
    let _ = tx.send(UiMessage::KubeGatewaysLoaded(k8s_gateways));

    Ok(())
}

/// Load Kubernetes HTTPRoute resources (Gateway API)
async fn load_http_routes(
    client: &Client,
    namespace: Option<&str>,
    tx: &mpsc::UnboundedSender<UiMessage>,
) -> Result<(), kube::Error> {
    let gvk = GroupVersionKind::gvk("gateway.networking.k8s.io", "v1", "HTTPRoute");
    let api_resource = ApiResource::from_gvk(&gvk);

    let routes: Vec<DynamicObject> = match namespace {
        Some(ns) => {
            let api: Api<DynamicObject> = Api::namespaced_with(client.clone(), ns, &api_resource);
            api.list(&Default::default()).await?.items
        }
        None => {
            let api: Api<DynamicObject> = Api::all_with(client.clone(), &api_resource);
            api.list(&Default::default()).await?.items
        }
    };

    let k8s_routes: Vec<K8sHttpRoute> = routes
        .into_iter()
        .filter_map(|route| {
            let name = route.metadata.name?;
            let namespace = route
                .metadata
                .namespace
                .unwrap_or_else(|| "default".to_string());

            let spec = route.data.get("spec")?.as_object()?;

            // Extract hostnames
            let hostnames: Vec<String> = spec
                .get("hostnames")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|h| h.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();

            // Extract parent refs (which Gateways this route attaches to)
            let parent_refs: Vec<K8sParentRef> = spec
                .get("parentRefs")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|p| {
                            let obj = p.as_object()?;
                            Some(K8sParentRef {
                                name: obj.get("name")?.as_str()?.to_string(),
                                namespace: obj
                                    .get("namespace")
                                    .and_then(|n| n.as_str())
                                    .map(|s| s.to_string()),
                                section_name: obj
                                    .get("sectionName")
                                    .and_then(|s| s.as_str())
                                    .map(|s| s.to_string()),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();

            // Extract paths and backend refs from rules
            let rules = spec.get("rules").and_then(|v| v.as_array())?;
            let mut paths: Vec<String> = Vec::new();
            let mut backend_refs: Vec<K8sBackendRef> = Vec::new();

            for rule in rules {
                let rule_obj = rule.as_object()?;

                // Extract paths from matches
                if let Some(matches) = rule_obj.get("matches").and_then(|m| m.as_array()) {
                    for m in matches {
                        if let Some(path) = m
                            .as_object()
                            .and_then(|o| o.get("path"))
                            .and_then(|p| p.as_object())
                        {
                            if let Some(value) = path.get("value").and_then(|v| v.as_str()) {
                                paths.push(value.to_string());
                            }
                        }
                    }
                }

                // Extract backend refs
                if let Some(backends) = rule_obj.get("backendRefs").and_then(|b| b.as_array()) {
                    for backend in backends {
                        if let Some(obj) = backend.as_object() {
                            let svc_name = obj
                                .get("name")
                                .and_then(|n| n.as_str())
                                .unwrap_or_default()
                                .to_string();
                            let svc_ns = obj
                                .get("namespace")
                                .and_then(|n| n.as_str())
                                .map(|s| s.to_string())
                                .unwrap_or_else(|| namespace.clone());
                            let port =
                                obj.get("port").and_then(|p| p.as_u64()).unwrap_or(80) as u16;
                            let weight =
                                obj.get("weight").and_then(|w| w.as_u64()).map(|w| w as u32);

                            if !svc_name.is_empty() {
                                backend_refs.push(K8sBackendRef {
                                    service_name: svc_name,
                                    namespace: svc_ns,
                                    port,
                                    weight,
                                });
                            }
                        }
                    }
                }
            }

            Some(K8sHttpRoute {
                name,
                namespace,
                hostnames,
                paths,
                parent_refs,
                backend_refs,
            })
        })
        .collect();

    debug!("Loaded {} HTTP routes", k8s_routes.len());
    let _ = tx.send(UiMessage::KubeHttpRoutesLoaded(k8s_routes));

    Ok(())
}

// =============================================================================
// Tests
// =============================================================================

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
