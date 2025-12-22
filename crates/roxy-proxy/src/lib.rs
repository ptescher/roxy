//! Roxy Proxy Library - HTTP/HTTPS proxy with OpenTelemetry instrumentation
//!
//! This library provides the core proxy server functionality that can be
//! embedded in other applications or run as a standalone binary.

use anyhow::{Context, Result};
use chrono::Utc;
use http_body_util::BodyExt;
use hudsucker::{
    certificate_authority::RcgenAuthority,
    hyper::{Request, Response},
    Body, HttpContext, HttpHandler, Proxy, RequestOrResponse,
};
use rcgen::{CertificateParams, KeyPair};
use roxy_core::{services::ServiceManager, ClickHouseConfig, HttpRequestRecord, RoxyClickHouse};
use rustls::crypto::aws_lc_rs;
use serde_json;
use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// Default proxy port
pub const DEFAULT_PROXY_PORT: u16 = 8080;

/// Proxy configuration
#[derive(Debug, Clone)]
pub struct ProxyConfig {
    /// Port to listen on
    pub port: u16,
    /// Whether to configure system proxy on startup
    pub configure_system_proxy: bool,
    /// Whether to start backend services (ClickHouse, OTel)
    pub start_services: bool,
    /// ClickHouse configuration
    pub clickhouse: ClickHouseConfig,
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            port: DEFAULT_PROXY_PORT,
            configure_system_proxy: false,
            start_services: true,
            clickhouse: ClickHouseConfig::default(),
        }
    }
}

/// Pending request data stored while waiting for response
#[derive(Debug, Clone)]
struct PendingRequest {
    id: String,
    trace_id: String,
    span_id: String,
    timestamp: i64,
    method: String,
    url: String,
    host: String,
    path: String,
    query: String,
    request_headers: String,
    request_body: String,
    request_body_size: i64,
    client_ip: String,
    protocol: String,
}

/// Shared state for the proxy handler
struct ProxyState {
    clickhouse: RoxyClickHouse,
    request_count: AtomicU64,
    /// Pending requests keyed by client address, stored as a queue for keep-alive connections
    pending_requests: RwLock<HashMap<SocketAddr, VecDeque<PendingRequest>>>,
}

/// HTTP handler that intercepts and logs requests
#[derive(Clone)]
struct RoxyHttpHandler {
    state: Arc<ProxyState>,
}

impl RoxyHttpHandler {
    fn new(state: Arc<ProxyState>) -> Self {
        Self { state }
    }

    /// Collect headers into a JSON string
    fn headers_to_json(headers: &http::HeaderMap) -> String {
        let map: HashMap<String, String> = headers
            .iter()
            .filter_map(|(k, v)| {
                v.to_str()
                    .ok()
                    .map(|val| (k.as_str().to_string(), val.to_string()))
            })
            .collect();
        serde_json::to_string(&map).unwrap_or_else(|_| "{}".to_string())
    }

    /// Extract full body bytes for logging
    async fn extract_body(body: Body) -> (Vec<u8>, Body) {
        match body.collect().await {
            Ok(collected) => {
                let bytes = collected.to_bytes();
                let captured = bytes.to_vec();
                // Recreate the body for forwarding using Full
                let new_body = Body::from(http_body_util::Full::new(bytes));
                (captured, new_body)
            }
            Err(e) => {
                warn!("Failed to read body: {}", e);
                (Vec::new(), Body::empty())
            }
        }
    }

    /// Convert body bytes to string, handling binary data
    fn body_to_string(bytes: &[u8]) -> String {
        // Try to interpret as UTF-8, fall back to base64 for binary
        match std::str::from_utf8(bytes) {
            Ok(s) => s.to_string(),
            Err(_) => {
                use base64::{engine::general_purpose::STANDARD, Engine};
                format!("base64:{}", STANDARD.encode(bytes))
            }
        }
    }
}

impl HttpHandler for RoxyHttpHandler {
    async fn handle_request(&mut self, ctx: &HttpContext, req: Request<Body>) -> RequestOrResponse {
        let request_id = Uuid::new_v4();
        let trace_id = format!("{:032x}", rand::random::<u128>());
        let span_id = format!("{:016x}", rand::random::<u64>());
        let timestamp = Utc::now().timestamp_millis();

        // Extract request metadata
        let method = req.method().to_string();
        let uri = req.uri().clone();
        let host = uri
            .host()
            .or_else(|| req.headers().get("host").and_then(|h| h.to_str().ok()))
            .unwrap_or("unknown")
            .to_string();

        let path = uri.path().to_string();
        let query = uri.query().unwrap_or("").to_string();

        let url = if uri.scheme().is_some() {
            uri.to_string()
        } else {
            format!(
                "https://{}{}{}",
                host,
                path,
                if query.is_empty() {
                    "".to_string()
                } else {
                    format!("?{}", query)
                }
            )
        };

        let client_ip = ctx.client_addr.ip().to_string();
        let protocol = format!("{:?}", req.version());

        // Capture headers
        let request_headers = Self::headers_to_json(req.headers());

        // Capture body
        let (parts, body) = req.into_parts();
        let (body_bytes, new_body) = Self::extract_body(body).await;
        let request_body_size = body_bytes.len() as i64;
        let request_body = Self::body_to_string(&body_bytes);

        // Reconstruct request
        let new_req = Request::from_parts(parts, new_body);

        // Store pending request
        let pending = PendingRequest {
            id: request_id.to_string(),
            trace_id,
            span_id,
            timestamp,
            method: method.clone(),
            url: url.clone(),
            host: host.clone(),
            path,
            query,
            request_headers,
            request_body,
            request_body_size,
            client_ip,
            protocol,
        };

        {
            let mut pending_requests = self.state.pending_requests.write().await;
            pending_requests
                .entry(ctx.client_addr)
                .or_insert_with(VecDeque::new)
                .push_back(pending);
        }

        self.state.request_count.fetch_add(1, Ordering::Relaxed);

        debug!(
            request_id = %request_id,
            method = %method,
            host = %host,
            url = %url,
            "Intercepting request"
        );

        // Store request_id in extensions for response correlation
        let mut new_req = new_req;
        new_req.extensions_mut().insert(request_id);

        RequestOrResponse::Request(new_req)
    }

    async fn handle_response(&mut self, ctx: &HttpContext, res: Response<Body>) -> Response<Body> {
        // Get the pending request for this client connection
        // HTTP/1.1 requests on a keep-alive connection are sequential,
        // so we use a queue per client address and pop the oldest request
        let pending = {
            let mut pending_requests = self.state.pending_requests.write().await;
            pending_requests
                .get_mut(&ctx.client_addr)
                .and_then(|queue| queue.pop_front())
        };

        let Some(pending) = pending else {
            debug!(
                client = %ctx.client_addr,
                "No pending request found for response"
            );
            return res;
        };

        // Calculate duration
        let response_timestamp = Utc::now().timestamp_millis();
        let duration_ms = (response_timestamp - pending.timestamp) as f64;

        // Capture response metadata
        let response_status = res.status().as_u16();
        let response_headers = Self::headers_to_json(res.headers());

        // Capture response body
        let (parts, body) = res.into_parts();
        let (body_bytes, new_body) = Self::extract_body(body).await;
        let response_body_size = body_bytes.len() as i64;
        let response_body = Self::body_to_string(&body_bytes);

        // Get TLS info if available
        let tls_version = String::new(); // Would need TLS context to get this

        // Create the record
        let record = HttpRequestRecord {
            id: pending.id,
            trace_id: pending.trace_id,
            span_id: pending.span_id,
            timestamp: pending.timestamp,
            method: pending.method.clone(),
            url: pending.url.clone(),
            host: pending.host.clone(),
            path: pending.path,
            query: pending.query,
            request_headers: pending.request_headers,
            request_body: pending.request_body,
            request_body_size: pending.request_body_size,
            response_status,
            response_headers,
            response_body,
            response_body_size,
            duration_ms,
            error: String::new(),
            client_ip: pending.client_ip,
            server_ip: ctx.client_addr.ip().to_string(),
            protocol: pending.protocol,
            tls_version,
        };

        // Store in ClickHouse asynchronously
        let clickhouse = self.state.clickhouse.clone();
        let method = pending.method.clone();
        let url = pending.url.clone();
        tokio::spawn(async move {
            match clickhouse.insert_http_request(&record).await {
                Ok(_) => {
                    debug!(
                        method = %method,
                        url = %url,
                        status = response_status,
                        duration_ms = duration_ms,
                        "Stored request in ClickHouse"
                    );
                }
                Err(e) => {
                    warn!(
                        method = %method,
                        url = %url,
                        error = %e,
                        "Failed to store request in ClickHouse"
                    );
                }
            }
        });

        info!(
            status = response_status,
            duration_ms = duration_ms,
            method = %pending.method,
            url = %pending.url,
            "Response captured"
        );

        // Reconstruct response
        Response::from_parts(parts, new_body)
    }
}

/// Generate a self-signed CA certificate for MITM
pub fn generate_ca() -> Result<RcgenAuthority> {
    let key_pair = KeyPair::generate().context("Failed to generate key pair")?;

    let mut params = CertificateParams::new(vec!["Roxy Proxy CA".to_string()])
        .context("Failed to create certificate params")?;
    params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    params
        .distinguished_name
        .push(rcgen::DnType::CommonName, "Roxy Proxy CA");
    params
        .distinguished_name
        .push(rcgen::DnType::OrganizationName, "Roxy");

    let cert = params
        .self_signed(&key_pair)
        .context("Failed to generate CA certificate")?;

    let crypto_provider = aws_lc_rs::default_provider();
    let ca = RcgenAuthority::new(key_pair, cert, 1000, crypto_provider);

    Ok(ca)
}

/// Configure macOS system proxy settings
#[cfg(target_os = "macos")]
pub mod system_proxy {
    use anyhow::Result;
    use std::process::Command;

    pub fn set_system_proxy(port: u16) -> Result<()> {
        let output = Command::new("networksetup")
            .args(["-listallnetworkservices"])
            .output()?;

        let services = String::from_utf8_lossy(&output.stdout);

        for service in ["Wi-Fi", "Ethernet", "USB 10/100/1000 LAN"] {
            if services.contains(service) {
                let _ = Command::new("networksetup")
                    .args(["-setwebproxy", service, "127.0.0.1", &port.to_string()])
                    .output();

                let _ = Command::new("networksetup")
                    .args([
                        "-setsecurewebproxy",
                        service,
                        "127.0.0.1",
                        &port.to_string(),
                    ])
                    .output();

                let _ = Command::new("networksetup")
                    .args(["-setwebproxystate", service, "on"])
                    .output();

                let _ = Command::new("networksetup")
                    .args(["-setsecurewebproxystate", service, "on"])
                    .output();

                tracing::info!("Configured system proxy for {}", service);
            }
        }

        Ok(())
    }

    pub fn clear_system_proxy() -> Result<()> {
        let output = Command::new("networksetup")
            .args(["-listallnetworkservices"])
            .output()?;

        let services = String::from_utf8_lossy(&output.stdout);

        for service in ["Wi-Fi", "Ethernet", "USB 10/100/1000 LAN"] {
            if services.contains(service) {
                let _ = Command::new("networksetup")
                    .args(["-setwebproxystate", service, "off"])
                    .output();

                let _ = Command::new("networksetup")
                    .args(["-setsecurewebproxystate", service, "off"])
                    .output();

                tracing::info!("Cleared system proxy for {}", service);
            }
        }

        Ok(())
    }
}

#[cfg(not(target_os = "macos"))]
pub mod system_proxy {
    use anyhow::Result;

    pub fn set_system_proxy(_port: u16) -> Result<()> {
        tracing::warn!("System proxy configuration not supported on this platform");
        Ok(())
    }

    pub fn clear_system_proxy() -> Result<()> {
        Ok(())
    }
}

/// The main proxy server that can be run embedded or standalone
pub struct ProxyServer {
    config: ProxyConfig,
    service_manager: ServiceManager,
    state: Arc<ProxyState>,
    running: Arc<AtomicBool>,
}

impl ProxyServer {
    /// Create a new proxy server with the given configuration
    pub async fn new(config: ProxyConfig) -> Result<Self> {
        let service_manager = ServiceManager::new();
        let clickhouse = RoxyClickHouse::new(config.clickhouse.clone());

        let state = Arc::new(ProxyState {
            clickhouse,
            request_count: AtomicU64::new(0),
            pending_requests: RwLock::new(HashMap::new()),
        });

        Ok(Self {
            config,
            service_manager,
            state,
            running: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Create a new proxy server with default configuration
    pub async fn with_defaults() -> Result<Self> {
        Self::new(ProxyConfig::default()).await
    }

    /// Set up and start backend services (ClickHouse, OTel)
    pub async fn setup_services(&mut self) -> Result<()> {
        info!("Setting up Roxy directories...");
        self.service_manager
            .setup()
            .await
            .context("Failed to set up service directories")?;

        if self.config.start_services {
            info!("Checking/installing backend services...");
            self.service_manager
                .install_all()
                .await
                .context("Failed to install backend services")?;

            info!("Starting backend services...");
            self.service_manager
                .start_all()
                .await
                .context("Failed to start backend services")?;
        }

        // Wait a moment for ClickHouse to be ready
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        // Initialize ClickHouse schema
        match self.state.clickhouse.initialize_schema().await {
            Ok(_) => info!("ClickHouse schema initialized"),
            Err(e) => warn!(
                "Failed to initialize ClickHouse schema: {}. Storage may be limited.",
                e
            ),
        }

        Ok(())
    }

    /// Start the proxy server (blocking)
    pub async fn run(&self) -> Result<()> {
        self.running.store(true, Ordering::SeqCst);

        let ca = generate_ca().context("Failed to generate CA certificate")?;
        info!("Generated CA certificate for TLS interception");

        let handler = RoxyHttpHandler::new(self.state.clone());
        let addr = SocketAddr::from(([127, 0, 0, 1], self.config.port));

        let proxy = Proxy::builder()
            .with_addr(addr)
            .with_ca(ca)
            .with_rustls_client(aws_lc_rs::default_provider())
            .with_http_handler(handler)
            .build()
            .context("Failed to build proxy")?;

        if self.config.configure_system_proxy {
            if let Err(e) = system_proxy::set_system_proxy(self.config.port) {
                warn!("Failed to configure system proxy: {}", e);
            }
        }

        info!("=================================================");
        info!("  Roxy Proxy is running!");
        info!("  Proxy:      http://127.0.0.1:{}", self.config.port);
        info!("  ClickHouse: http://127.0.0.1:8123");
        info!("  OTel:       http://127.0.0.1:4317 (gRPC)");
        info!("=================================================");

        if let Err(e) = proxy.start().await {
            error!("Proxy error: {}", e);
        }

        self.running.store(false, Ordering::SeqCst);
        Ok(())
    }

    /// Check if the proxy is currently running
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Get the current request count
    pub fn request_count(&self) -> u64 {
        self.state.request_count.load(Ordering::Relaxed)
    }

    /// Get the configured port
    pub fn port(&self) -> u16 {
        self.config.port
    }

    /// Get the ClickHouse client for querying data
    pub fn clickhouse(&self) -> &RoxyClickHouse {
        &self.state.clickhouse
    }

    /// Stop backend services
    pub async fn stop_services(&mut self) -> Result<()> {
        if self.config.start_services {
            self.service_manager
                .stop_all()
                .await
                .context("Error stopping backend services")?;
        }

        if self.config.configure_system_proxy {
            if let Err(e) = system_proxy::clear_system_proxy() {
                warn!("Failed to clear system proxy: {}", e);
            }
        }

        Ok(())
    }
}

/// Run the proxy server with full lifecycle management
/// This is a convenience function for running the proxy standalone
pub async fn run_proxy(config: ProxyConfig) -> Result<()> {
    let mut server = ProxyServer::new(config).await?;

    server.setup_services().await?;

    let shutdown = tokio::signal::ctrl_c();

    tokio::select! {
        result = server.run() => {
            result?;
        }
        _ = shutdown => {
            info!("Shutdown signal received...");
        }
    }

    let count = server.request_count();
    info!("Total requests processed: {}", count);

    info!("Cleaning up...");
    server.stop_services().await?;

    info!("Goodbye!");
    Ok(())
}
