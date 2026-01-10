//! Roxy Proxy Library - HTTP forwarding proxy with request logging
//!
//! This library provides a simple forwarding proxy that:
//! - Forwards HTTP requests and logs them to ClickHouse
//! - Supports TLS interception for configured hosts (with CA certificate generation)
//! - Tunnels HTTPS (CONNECT) requests transparently for non-intercepted hosts
//! - Provides a SOCKS5 proxy for TCP connections (PostgreSQL, Kafka, etc.)
//!
//! ## TLS Interception
//!
//! By default, HTTPS connections are tunneled through without interception.
//! To enable TLS interception:
//!
//! 1. Enable TLS in the proxy configuration
//! 2. Add hosts to the interception list
//! 3. Trust the generated CA certificate on your clients
//!
//! The CA certificate is generated on first run and stored in `~/.roxy/ca.crt`.

use std::sync::Once;

static CRYPTO_INIT: Once = Once::new();

/// Initialize the TLS crypto provider (ring).
///
/// This must be called before any TLS operations. It's safe to call multiple
/// times - only the first call will have any effect.
pub fn init_crypto_provider() {
    CRYPTO_INIT.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

pub mod gateway;
pub mod kubectl;
pub mod process;
pub mod protocol;
pub mod socks;
pub mod tls;

pub use gateway::GatewayRouter;
pub use kubectl::{K8sService, K8sStream, KubectlPortForwardManager};
pub use process::{identify_process, ProcessInfo};
pub use socks::{SocksConfig, SocksProxy, DEFAULT_SOCKS_PORT};
pub use tls::{CertificateAuthority, TlsConfig, TlsInterceptionMode, TlsManager};

// Re-export MCP types
pub use roxy_mcp::{SseMcpServer, SseServerConfig as McpConfig, DEFAULT_MCP_SSE_PORT};

use std::collections::HashMap;
use tokio::sync::RwLock;

/// How long to keep K8s connection entries after the last connection closes
const K8S_CONNECTION_RETAIN_SECS: i64 = 300; // 5 minutes

/// Information about an active K8s connection through the proxy
#[derive(Debug, Clone)]
pub struct ActiveK8sConnectionInfo {
    /// Service name (e.g., "postgres")
    pub service_name: String,
    /// Namespace (e.g., "database")
    pub namespace: String,
    /// Remote port
    pub port: u16,
    /// Full DNS name
    pub dns_name: String,
    /// When the connection was first established
    pub connected_at: chrono::DateTime<chrono::Utc>,
    /// When the connection was last used (for cleanup)
    pub last_used: chrono::DateTime<chrono::Utc>,
    /// Number of active connections to this service
    pub connection_count: u32,
}

impl ActiveK8sConnectionInfo {
    /// Check if this entry has expired (no active connections and past retention time)
    pub fn is_expired(&self) -> bool {
        if self.connection_count > 0 {
            return false;
        }
        let now = chrono::Utc::now();
        let elapsed = now.signed_duration_since(self.last_used);
        elapsed.num_seconds() > K8S_CONNECTION_RETAIN_SECS
    }
}

/// Shared state for tracking active K8s connections
///
/// This is shared between the proxy and UI to show real-time port forward status.
#[derive(Debug, Default)]
pub struct ActiveK8sConnections {
    /// Map of service DNS name to connection info
    connections: RwLock<HashMap<String, ActiveK8sConnectionInfo>>,
}

impl ActiveK8sConnections {
    /// Create a new empty connections tracker
    pub fn new() -> Self {
        Self {
            connections: RwLock::new(HashMap::new()),
        }
    }

    /// Record a new connection to a K8s service
    pub async fn add_connection(&self, service: &K8sService) {
        let mut conns = self.connections.write().await;
        let dns_name = service.dns_name();
        let now = chrono::Utc::now();

        if let Some(info) = conns.get_mut(&dns_name) {
            info.connection_count += 1;
            info.last_used = now;
        } else {
            conns.insert(
                dns_name.clone(),
                ActiveK8sConnectionInfo {
                    service_name: service.name.clone(),
                    namespace: service.namespace.clone(),
                    port: service.port,
                    dns_name,
                    connected_at: now,
                    last_used: now,
                    connection_count: 1,
                },
            );
        }

        // Clean up expired entries while we have the lock
        conns.retain(|_, info| !info.is_expired());
    }

    /// Record a connection being closed
    ///
    /// The entry is kept for 5 minutes after the last connection closes
    /// to allow the UI to show recently used services and enable reuse.
    pub async fn remove_connection(&self, service: &K8sService) {
        let mut conns = self.connections.write().await;
        let dns_name = service.dns_name();

        if let Some(info) = conns.get_mut(&dns_name) {
            if info.connection_count > 0 {
                info.connection_count -= 1;
            }
            // Update last_used time - entry will be kept for 5 minutes
            info.last_used = chrono::Utc::now();
        }

        // Clean up expired entries while we have the lock
        conns.retain(|_, info| !info.is_expired());
    }

    /// Get all active and recent connections (within retention period)
    pub async fn list(&self) -> Vec<ActiveK8sConnectionInfo> {
        // First clean up expired entries
        {
            let mut conns = self.connections.write().await;
            conns.retain(|_, info| !info.is_expired());
        }

        // Then return the remaining entries
        let conns = self.connections.read().await;
        conns.values().cloned().collect()
    }

    /// Get the number of active K8s services
    pub async fn count(&self) -> usize {
        let conns = self.connections.read().await;
        conns.len()
    }
}

use anyhow::{Context, Result};
use chrono::Utc;
use http_body_util::{BodyExt, Empty, Full};
use hyper::body::Bytes;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use roxy_core::{services::ServiceManager, ClickHouseConfig, HttpRequestRecord, RoxyClickHouse};

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// Default proxy port
pub const DEFAULT_PROXY_PORT: u16 = 8080;

/// Proxy configuration
#[derive(Debug, Clone)]
pub struct ProxyConfig {
    /// Port to listen on for HTTP proxy
    pub port: u16,
    /// Whether to configure system proxy on startup
    pub configure_system_proxy: bool,
    /// Whether to start backend services (ClickHouse, OTel)
    pub start_services: bool,
    /// ClickHouse configuration
    pub clickhouse: ClickHouseConfig,
    /// Whether to enable SOCKS5 proxy
    pub enable_socks: bool,
    /// SOCKS5 proxy configuration
    pub socks: SocksConfig,
    /// Whether to enable MCP server
    pub enable_mcp: bool,
    /// MCP server configuration
    pub mcp: McpConfig,
    /// TLS interception configuration
    pub tls: TlsConfig,
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            port: DEFAULT_PROXY_PORT,
            configure_system_proxy: false,
            start_services: true,
            clickhouse: ClickHouseConfig::default(),
            enable_socks: true,
            socks: SocksConfig::default(),
            enable_mcp: true,
            mcp: McpConfig::default(),
            tls: TlsConfig::default(),
        }
    }
}

/// Shared state for the proxy
struct ProxyState {
    clickhouse: RoxyClickHouse,
    request_count: AtomicU64,
    /// Active tunnel count for monitoring
    tunnel_count: AtomicU64,
    /// Kubectl port-forward manager for K8s service routing
    kubectl_manager: tokio::sync::Mutex<KubectlPortForwardManager>,
    /// Gateway router for HTTPRoute-based routing
    gateway_router: tokio::sync::Mutex<GatewayRouter>,
    /// Shared state for tracking active K8s connections (for UI display)
    active_k8s_connections: Arc<ActiveK8sConnections>,
    /// TLS manager for certificate generation and interception
    tls_manager: Option<Arc<TlsManager>>,
}

/// HTTP body type alias
type BoxBody = http_body_util::combinators::BoxBody<Bytes, hyper::Error>;

fn empty() -> BoxBody {
    Empty::<Bytes>::new()
        .map_err(|never| match never {})
        .boxed()
}

fn full<T: Into<Bytes>>(chunk: T) -> BoxBody {
    Full::new(chunk.into())
        .map_err(|never| match never {})
        .boxed()
}

/// Handle incoming proxy requests
async fn handle_request(
    req: Request<hyper::body::Incoming>,
    client_addr: SocketAddr,
    state: Arc<ProxyState>,
) -> Result<Response<BoxBody>, hyper::Error> {
    let method = req.method().clone();
    let _uri = req.uri().clone();

    // Handle CONNECT requests (HTTPS tunneling)
    if method == Method::CONNECT {
        return handle_connect(req, client_addr, state).await;
    }

    // Handle regular HTTP requests (forward proxy)
    handle_http_forward(req, client_addr, state).await
}

/// Handle HTTP CONNECT requests by establishing a tunnel
///
/// If TLS interception is enabled for the target host, we perform TLS
/// termination and re-encryption. Otherwise, we tunnel raw bytes through.
async fn handle_connect(
    req: Request<hyper::body::Incoming>,
    client_addr: SocketAddr,
    state: Arc<ProxyState>,
) -> Result<Response<BoxBody>, hyper::Error> {
    let authority = req.uri().authority().map(|a| a.to_string());

    if let Some(addr) = authority {
        // Extract host (without port) for interception check
        let host = addr.split(':').next().unwrap_or(&addr).to_string();
        let target_port = addr
            .split(':')
            .nth(1)
            .and_then(|p| p.parse::<u16>().ok())
            .unwrap_or(443);

        // Check if we should intercept TLS for this host
        let should_intercept = if let Some(ref tls_manager) = state.tls_manager {
            tls_manager.should_intercept(&host).await
        } else {
            false
        };

        state.tunnel_count.fetch_add(1, Ordering::Relaxed);

        if should_intercept {
            debug!(
                client = %client_addr,
                target = %addr,
                "Intercepting TLS connection"
            );

            // Get TLS manager for interception
            let tls_manager = state.tls_manager.clone().unwrap();
            let clickhouse = state.clickhouse.clone();

            // Spawn a task to handle the intercepted connection
            tokio::task::spawn(async move {
                match hyper::upgrade::on(req).await {
                    Ok(upgraded) => {
                        if let Err(e) = handle_tls_intercept(
                            upgraded,
                            &host,
                            target_port,
                            client_addr,
                            tls_manager,
                            clickhouse,
                        )
                        .await
                        {
                            error!(
                                target = %addr,
                                error = %e,
                                "TLS interception failed"
                            );
                        }
                    }
                    Err(e) => {
                        error!(error = %e, "Upgrade failed for TLS interception");
                    }
                }

                state.tunnel_count.fetch_sub(1, Ordering::Relaxed);
            });
        } else {
            debug!(
                client = %client_addr,
                target = %addr,
                "Establishing HTTPS tunnel (passthrough)"
            );

            // Spawn a task to handle the transparent tunnel
            tokio::task::spawn(async move {
                match hyper::upgrade::on(req).await {
                    Ok(upgraded) => {
                        // Connect to the target server
                        match TcpStream::connect(&addr).await {
                            Ok(mut target_stream) => {
                                let mut upgraded = TokioIo::new(upgraded);

                                // Bidirectional copy
                                let (mut client_reader, mut client_writer) =
                                    tokio::io::split(&mut upgraded);
                                let (mut target_reader, mut target_writer) =
                                    tokio::io::split(&mut target_stream);

                                let client_to_target =
                                    tokio::io::copy(&mut client_reader, &mut target_writer);
                                let target_to_client =
                                    tokio::io::copy(&mut target_reader, &mut client_writer);

                                match tokio::try_join!(client_to_target, target_to_client) {
                                    Ok((to_target, to_client)) => {
                                        debug!(
                                            target = %addr,
                                            bytes_to_target = to_target,
                                            bytes_to_client = to_client,
                                            "Tunnel closed"
                                        );
                                    }
                                    Err(e) => {
                                        debug!(target = %addr, error = %e, "Tunnel error");
                                    }
                                }
                            }
                            Err(e) => {
                                error!(target = %addr, error = %e, "Failed to connect to target");
                            }
                        }
                    }
                    Err(e) => {
                        error!(error = %e, "Upgrade failed");
                    }
                }

                state.tunnel_count.fetch_sub(1, Ordering::Relaxed);
            });
        }

        // Return 200 OK to indicate tunnel established
        Ok(Response::new(empty()))
    } else {
        let mut resp = Response::new(full("CONNECT must specify a host"));
        *resp.status_mut() = StatusCode::BAD_REQUEST;
        Ok(resp)
    }
}

/// Handle TLS interception for a CONNECT tunnel
///
/// This performs:
/// 1. TLS termination with a generated certificate for the client
/// 2. TLS connection to the actual target server
/// 3. HTTP request/response forwarding with logging
async fn handle_tls_intercept(
    upgraded: hyper::upgrade::Upgraded,
    host: &str,
    port: u16,
    client_addr: SocketAddr,
    tls_manager: Arc<TlsManager>,
    clickhouse: RoxyClickHouse,
) -> anyhow::Result<()> {
    use tokio_rustls::TlsAcceptor;

    // Get or generate certificate for this host
    let server_config = tls_manager.get_certificate(host).await?;
    let acceptor = TlsAcceptor::from(server_config);

    // Accept TLS from the client - wrap in TokioIo for AsyncRead/AsyncWrite
    let client_stream = TokioIo::new(upgraded);
    let tls_stream = acceptor.accept(client_stream).await?;

    debug!(host = %host, "TLS handshake complete with client");

    // Connect to the actual target server with TLS
    let target_addr = format!("{}:{}", host, port);
    let target_stream = TcpStream::connect(&target_addr).await?;

    // Create TLS connector for the target
    let mut root_store = rustls::RootCertStore::empty();
    root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

    let client_config = rustls::ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();

    let connector = tokio_rustls::TlsConnector::from(Arc::new(client_config));
    let server_name = rustls::pki_types::ServerName::try_from(host.to_string())?;
    let target_tls = connector.connect(server_name, target_stream).await?;

    debug!(host = %host, "TLS handshake complete with target server");

    // Now we have decrypted streams on both sides - run HTTP proxy between them
    run_https_proxy(tls_stream, target_tls, host, client_addr, clickhouse).await
}

/// Run HTTP proxy between decrypted TLS streams
async fn run_https_proxy<C, S>(
    client_stream: C,
    server_stream: S,
    host: &str,
    client_addr: SocketAddr,
    clickhouse: RoxyClickHouse,
) -> anyhow::Result<()>
where
    C: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let (client_read, mut client_write) = tokio::io::split(client_stream);
    let (server_read, mut server_write) = tokio::io::split(server_stream);

    let mut client_reader = BufReader::new(client_read);
    let mut server_reader = BufReader::new(server_read);

    let host = host.to_string();

    // Simple bidirectional proxy with basic HTTP request/response logging
    // For a full implementation, we would parse HTTP and log like handle_http_forward
    loop {
        let mut request_line = String::new();

        // Read request from client
        match client_reader.read_line(&mut request_line).await {
            Ok(0) => {
                debug!(host = %host, "Client closed connection");
                break;
            }
            Ok(_) => {
                // Parse basic request info
                let parts: Vec<&str> = request_line.trim().split_whitespace().collect();
                let (method, path) = if parts.len() >= 2 {
                    (parts[0], parts[1])
                } else {
                    ("UNKNOWN", "/")
                };

                debug!(
                    host = %host,
                    method = %method,
                    path = %path,
                    "Intercepted HTTPS request"
                );

                // Forward request line to server
                server_write.write_all(request_line.as_bytes()).await?;

                // Read and forward headers
                loop {
                    let mut header_line = String::new();
                    client_reader.read_line(&mut header_line).await?;
                    server_write.write_all(header_line.as_bytes()).await?;

                    if header_line.trim().is_empty() {
                        break;
                    }
                }
                server_write.flush().await?;

                // Read response from server and forward to client
                let mut response_line = String::new();
                server_reader.read_line(&mut response_line).await?;
                client_write.write_all(response_line.as_bytes()).await?;

                // Parse response status
                let status: u16 = response_line
                    .split_whitespace()
                    .nth(1)
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);

                debug!(
                    host = %host,
                    method = %method,
                    path = %path,
                    status = %status,
                    "Intercepted HTTPS response"
                );

                // Log to ClickHouse (simplified - full implementation would capture body)
                let record = HttpRequestRecord {
                    id: Uuid::new_v4().to_string(),
                    trace_id: format!("{:032x}", rand::random::<u128>()),
                    span_id: format!("{:016x}", rand::random::<u64>()),
                    timestamp: Utc::now().timestamp_millis(),
                    method: method.to_string(),
                    url: format!("https://{}{}", host, path),
                    host: host.clone(),
                    path: path.to_string(),
                    query: String::new(),
                    request_headers: "{}".to_string(),
                    request_body: String::new(),
                    request_body_size: 0,
                    response_status: status,
                    response_headers: "{}".to_string(),
                    response_body: String::new(),
                    response_body_size: 0,
                    duration_ms: 0.0,
                    error: String::new(),
                    client_ip: client_addr.ip().to_string(),
                    server_ip: host.clone(),
                    protocol: "HTTP/1.1".to_string(),
                    tls_version: "TLS 1.3".to_string(),
                    client_name: String::new(),
                };

                let ch = clickhouse.clone();
                tokio::spawn(async move {
                    if let Err(e) = ch.insert_http_request(&record).await {
                        tracing::warn!(error = %e, "Failed to log intercepted request");
                    }
                });

                // Forward response headers
                loop {
                    let mut header_line = String::new();
                    server_reader.read_line(&mut header_line).await?;
                    client_write.write_all(header_line.as_bytes()).await?;

                    if header_line.trim().is_empty() {
                        break;
                    }
                }
                client_write.flush().await?;
            }
            Err(e) => {
                debug!(host = %host, error = %e, "Error reading from client");
                break;
            }
        }
    }

    Ok(())
}

/// Handle regular HTTP forward proxy requests
/// Extract client application name from request headers
///
/// Checks headers in order of preference:
/// 1. X-Client-Name - explicit client identifier
/// 2. X-Application-Name - common alternative
/// 3. User-Agent - fallback, extracts app name before version/details
fn extract_client_name(headers: &hyper::HeaderMap) -> String {
    // Check explicit client name headers first
    if let Some(name) = headers.get("x-client-name") {
        if let Ok(s) = name.to_str() {
            return s.to_string();
        }
    }

    if let Some(name) = headers.get("x-application-name") {
        if let Ok(s) = name.to_str() {
            return s.to_string();
        }
    }

    // Fall back to User-Agent, extract the main app name
    if let Some(ua) = headers.get("user-agent") {
        if let Ok(s) = ua.to_str() {
            // Extract first component before / or space
            // e.g., "MyApp/1.0.0" -> "MyApp", "curl/7.68.0" -> "curl"
            let name = s.split(|c| c == '/' || c == ' ').next().unwrap_or(s).trim();
            if !name.is_empty() {
                return name.to_string();
            }
        }
    }

    String::new()
}

async fn handle_http_forward(
    req: Request<hyper::body::Incoming>,
    client_addr: SocketAddr,
    state: Arc<ProxyState>,
) -> Result<Response<BoxBody>, hyper::Error> {
    let request_id = Uuid::new_v4();
    let trace_id = format!("{:032x}", rand::random::<u128>());
    let span_id = format!("{:016x}", rand::random::<u64>());
    let timestamp = Utc::now().timestamp_millis();

    // Extract client name from headers before consuming the request
    let mut client_name = extract_client_name(req.headers());

    // If no client name from headers, try to identify the process
    let process_info = process::identify_process(&client_addr);
    if client_name.is_empty() {
        client_name = process_info.display_name();
    }

    // Extract request metadata
    let method = req.method().to_string();
    let uri = req.uri().clone();

    // Determine target host from URI or Host header
    let host = uri
        .host()
        .map(|h| h.to_string())
        .or_else(|| {
            req.headers()
                .get("host")
                .and_then(|h| h.to_str().ok())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| "unknown".to_string());

    let port = uri.port_u16().unwrap_or(80);
    let path = uri.path().to_string();
    let query = uri.query().unwrap_or("").to_string();

    let url = if uri.scheme().is_some() {
        uri.to_string()
    } else {
        format!(
            "http://{}{}{}",
            host,
            path,
            if query.is_empty() {
                "".to_string()
            } else {
                format!("?{}", query)
            }
        )
    };

    let protocol = format!("{:?}", req.version());

    // Capture headers
    let request_headers = headers_to_json(req.headers());

    // Capture body
    let (parts, body) = req.into_parts();
    let body_bytes = body
        .collect()
        .await
        .map(|c| c.to_bytes())
        .unwrap_or_default();
    let request_body_size = body_bytes.len() as i64;
    let request_body = body_to_string(&body_bytes);

    state.request_count.fetch_add(1, Ordering::Relaxed);

    debug!(
        request_id = %request_id,
        method = %method,
        host = %host,
        url = %url,
        "Forwarding request"
    );

    // Connect to target - first try HTTPRoute resolution, then K8s DNS, then direct
    let target_addr = format!("{}:{}", host, port);

    // Try to resolve via HTTPRoute first
    let gateway_result = {
        let mut router = state.gateway_router.lock().await;
        router.resolve(&host, &path, &method, &parts.headers).await
    };

    let connect_addr = match gateway_result {
        Ok(Some((backend_dns, backend_port))) => {
            // HTTPRoute matched - route to the backend service via port-forward
            if let Some(k8s_service) = K8sService::from_dns_name(&backend_dns, backend_port) {
                info!(
                    request_id = %request_id,
                    route_host = %host,
                    service = %k8s_service.name,
                    namespace = %k8s_service.namespace,
                    port = backend_port,
                    "Routing via HTTPRoute to K8s service"
                );
                // Track this K8s connection for UI display
                state
                    .active_k8s_connections
                    .add_connection(&k8s_service)
                    .await;

                let mut manager = state.kubectl_manager.lock().await;
                match manager.get_or_create_forward(k8s_service).await {
                    Ok(local_addr) => {
                        info!(
                            request_id = %request_id,
                            local_addr = %local_addr,
                            "K8s port-forward ready via HTTPRoute"
                        );
                        local_addr.to_string()
                    }
                    Err(e) => {
                        error!(
                            request_id = %request_id,
                            error = %e,
                            "Failed to create K8s port-forward for HTTPRoute backend"
                        );
                        target_addr.clone()
                    }
                }
            } else {
                // Backend is not a K8s service DNS, try direct connection
                format!("{}:{}", backend_dns, backend_port)
            }
        }
        Ok(None) => {
            // No HTTPRoute match - check if this is a direct K8s service DNS
            let is_k8s = KubectlPortForwardManager::is_k8s_service(&host);
            if is_k8s {
                if let Some(k8s_service) = K8sService::from_dns_name(&host, port) {
                    info!(
                        request_id = %request_id,
                        service = %k8s_service.name,
                        namespace = %k8s_service.namespace,
                        port = port,
                        "Routing HTTP request to K8s service via port-forward"
                    );
                    // Track this K8s connection for UI display
                    state
                        .active_k8s_connections
                        .add_connection(&k8s_service)
                        .await;

                    let mut manager = state.kubectl_manager.lock().await;
                    match manager.get_or_create_forward(k8s_service).await {
                        Ok(local_addr) => {
                            info!(
                                request_id = %request_id,
                                local_addr = %local_addr,
                                "K8s port-forward ready, connecting to local address"
                            );
                            local_addr.to_string()
                        }
                        Err(e) => {
                            error!(
                                request_id = %request_id,
                                error = %e,
                                "Failed to create K8s port-forward"
                            );
                            target_addr.clone()
                        }
                    }
                } else {
                    target_addr.clone()
                }
            } else {
                target_addr.clone()
            }
        }
        Err(e) => {
            warn!(
                request_id = %request_id,
                error = %e,
                "HTTPRoute resolution failed, falling back to direct connection"
            );
            target_addr.clone()
        }
    };

    match TcpStream::connect(&connect_addr).await {
        Ok(stream) => {
            let io = TokioIo::new(stream);

            // Create client connection
            let (mut sender, conn) = match hyper::client::conn::http1::handshake(io).await {
                Ok(conn) => conn,
                Err(e) => {
                    error!(error = %e, "Client handshake failed");
                    let mut resp = Response::new(full(format!("Connection error: {}", e)));
                    *resp.status_mut() = StatusCode::BAD_GATEWAY;
                    return Ok(resp);
                }
            };

            // Spawn connection driver
            tokio::task::spawn(async move {
                if let Err(err) = conn.await {
                    debug!(error = %err, "Connection error");
                }
            });

            // Reconstruct request for target
            let mut target_req = Request::builder()
                .method(parts.method.clone())
                .uri(
                    parts
                        .uri
                        .path_and_query()
                        .map(|pq| pq.as_str())
                        .unwrap_or("/"),
                )
                .version(parts.version);

            // Copy headers
            for (key, value) in parts.headers.iter() {
                if key != "proxy-connection" {
                    target_req = target_req.header(key, value);
                }
            }

            let target_req = target_req
                .body(Full::new(body_bytes.clone()))
                .expect("Failed to build request");

            // Send request and get response
            let response_timestamp = Utc::now().timestamp_millis();

            match sender.send_request(target_req).await {
                Ok(resp) => {
                    let response_status = resp.status().as_u16();
                    let response_headers = headers_to_json(resp.headers());

                    // Capture response body
                    let (resp_parts, resp_body) = resp.into_parts();
                    let resp_body_bytes = resp_body
                        .collect()
                        .await
                        .map(|c| c.to_bytes())
                        .unwrap_or_default();
                    let response_body_size = resp_body_bytes.len() as i64;
                    let response_body = body_to_string(&resp_body_bytes);

                    let duration_ms = (response_timestamp - timestamp) as f64;

                    // Log to ClickHouse
                    let record = HttpRequestRecord {
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
                        response_status,
                        response_headers,
                        response_body,
                        response_body_size,
                        duration_ms,
                        error: String::new(),
                        client_ip: client_addr.ip().to_string(),
                        server_ip: target_addr.clone(),
                        protocol,
                        tls_version: String::new(),
                        client_name: client_name.clone(),
                    };

                    info!(
                        status = response_status,
                        duration_ms = duration_ms,
                        method = %method,
                        url = %url,
                        "Response captured"
                    );

                    let clickhouse = state.clickhouse.clone();
                    tokio::spawn(async move {
                        if let Err(e) = clickhouse.insert_http_request(&record).await {
                            tracing::warn!(
                                error = %e,
                                "Failed to store request in ClickHouse"
                            );
                        }
                    });

                    // Reconstruct response
                    let mut response = Response::builder()
                        .status(resp_parts.status)
                        .version(resp_parts.version);

                    for (key, value) in resp_parts.headers.iter() {
                        response = response.header(key, value);
                    }

                    Ok(response.body(full(resp_body_bytes)).unwrap())
                }
                Err(e) => {
                    error!(error = %e, url = %url, "Request failed");
                    let mut resp = Response::new(full(format!("Request failed: {}", e)));
                    *resp.status_mut() = StatusCode::BAD_GATEWAY;
                    Ok(resp)
                }
            }
        }
        Err(e) => {
            error!(error = %e, target = %target_addr, "Failed to connect");
            let mut resp = Response::new(full(format!("Connection failed: {}", e)));
            *resp.status_mut() = StatusCode::BAD_GATEWAY;
            Ok(resp)
        }
    }
}

/// Convert headers to JSON string
fn headers_to_json(headers: &hyper::HeaderMap) -> String {
    let map: HashMap<String, String> = headers
        .iter()
        .map(|(k, v)| {
            (
                k.as_str().to_string(),
                v.to_str().unwrap_or("<binary>").to_string(),
            )
        })
        .collect();
    serde_json::to_string(&map).unwrap_or_else(|_| "{}".to_string())
}

/// Convert body bytes to string
fn body_to_string(bytes: &Bytes) -> String {
    if bytes.is_empty() {
        return String::new();
    }

    // Try to interpret as UTF-8, truncate if too large
    let max_size = 64 * 1024; // 64KB max
    let slice = if bytes.len() > max_size {
        &bytes[..max_size]
    } else {
        &bytes[..]
    };

    String::from_utf8_lossy(slice).to_string()
}

// Platform-specific system proxy configuration
#[cfg(target_os = "macos")]
pub mod system_proxy {
    use anyhow::{Context, Result};
    use std::process::Command;
    use tracing::info;

    /// Hosts to bypass when using the proxy (localhost, local addresses)
    const BYPASS_HOSTS: &str = "localhost, 127.0.0.1, ::1, *.local";

    /// Set the system HTTP proxy
    pub fn set_system_proxy(port: u16) -> Result<()> {
        // Get the primary network service
        let output = Command::new("networksetup")
            .args(["-listallnetworkservices"])
            .output()
            .context("Failed to list network services")?;

        let services = String::from_utf8_lossy(&output.stdout);
        let primary_service = services
            .lines()
            .skip(1) // Skip header
            .find(|s| !s.starts_with('*') && (s.contains("Wi-Fi") || s.contains("Ethernet")))
            .unwrap_or("Wi-Fi");

        info!(service = %primary_service, "Setting system proxy");

        // Set HTTP proxy
        Command::new("networksetup")
            .args([
                "-setwebproxy",
                primary_service,
                "127.0.0.1",
                &port.to_string(),
            ])
            .output()
            .context("Failed to set HTTP proxy")?;

        // Enable HTTP proxy
        Command::new("networksetup")
            .args(["-setwebproxystate", primary_service, "on"])
            .output()
            .context("Failed to enable HTTP proxy")?;

        // Set HTTPS proxy
        Command::new("networksetup")
            .args([
                "-setsecurewebproxy",
                primary_service,
                "127.0.0.1",
                &port.to_string(),
            ])
            .output()
            .context("Failed to set HTTPS proxy")?;

        // Enable HTTPS proxy
        Command::new("networksetup")
            .args(["-setsecurewebproxystate", primary_service, "on"])
            .output()
            .context("Failed to enable HTTPS proxy")?;

        // Set proxy bypass domains (localhost, etc.)
        // This ensures requests to localhost:3000 don't go through the proxy
        Command::new("networksetup")
            .args(["-setproxybypassdomains", primary_service, BYPASS_HOSTS])
            .output()
            .context("Failed to set proxy bypass domains")?;

        info!(
            port = port,
            bypass = BYPASS_HOSTS,
            "System proxy configured"
        );
        Ok(())
    }

    /// Clear the system proxy settings
    pub fn clear_system_proxy() -> Result<()> {
        let output = Command::new("networksetup")
            .args(["-listallnetworkservices"])
            .output()
            .context("Failed to list network services")?;

        let services = String::from_utf8_lossy(&output.stdout);
        let primary_service = services
            .lines()
            .skip(1)
            .find(|s| !s.starts_with('*') && (s.contains("Wi-Fi") || s.contains("Ethernet")))
            .unwrap_or("Wi-Fi");

        info!(service = %primary_service, "Clearing system proxy");

        // Disable HTTP proxy
        Command::new("networksetup")
            .args(["-setwebproxystate", primary_service, "off"])
            .output()
            .context("Failed to disable HTTP proxy")?;

        // Disable HTTPS proxy
        Command::new("networksetup")
            .args(["-setsecurewebproxystate", primary_service, "off"])
            .output()
            .context("Failed to disable HTTPS proxy")?;

        // Clear bypass domains
        Command::new("networksetup")
            .args(["-setproxybypassdomains", primary_service, ""])
            .output()
            .context("Failed to clear proxy bypass domains")?;

        info!("System proxy cleared");
        Ok(())
    }
}

#[cfg(not(target_os = "macos"))]
pub mod system_proxy {
    use anyhow::Result;

    pub fn set_system_proxy(_port: u16) -> Result<()> {
        tracing::warn!("System proxy configuration not implemented for this platform");
        Ok(())
    }

    pub fn clear_system_proxy() -> Result<()> {
        Ok(())
    }
}

/// Proxy server
pub struct ProxyServer {
    config: ProxyConfig,
    service_manager: ServiceManager,
    state: Arc<ProxyState>,
    running: Arc<AtomicBool>,
    /// Optional SOCKS proxy for TCP connections
    socks_proxy: Option<Arc<SocksProxy>>,
    /// Shared state for active K8s connections
    active_k8s_connections: Arc<ActiveK8sConnections>,
    /// Optional MCP server for AI agent integration
    mcp_server: Option<SseMcpServer>,
    /// TLS manager for certificate generation (if TLS interception enabled)
    tls_manager: Option<Arc<TlsManager>>,
}

impl ProxyServer {
    /// Create a new proxy server with the given configuration
    pub async fn new(config: ProxyConfig) -> Result<Self> {
        Self::new_with_connections(config, Arc::new(ActiveK8sConnections::new())).await
    }

    /// Create a new proxy server with shared K8s connection tracking
    pub async fn new_with_connections(
        config: ProxyConfig,
        active_k8s_connections: Arc<ActiveK8sConnections>,
    ) -> Result<Self> {
        let clickhouse = RoxyClickHouse::new(config.clickhouse.clone());

        // Initialize TLS crypto provider
        init_crypto_provider();

        // Initialize TLS manager if TLS interception is enabled
        let tls_manager = if config.tls.enabled {
            match TlsManager::new(config.tls.clone()).await {
                Ok(manager) => {
                    info!(
                        ca_cert = %manager.ca_cert_path().await,
                        "TLS interception enabled"
                    );
                    Some(Arc::new(manager))
                }
                Err(e) => {
                    warn!(error = %e, "Failed to initialize TLS manager, interception disabled");
                    None
                }
            }
        } else {
            None
        };

        let state = Arc::new(ProxyState {
            clickhouse,
            request_count: AtomicU64::new(0),
            tunnel_count: AtomicU64::new(0),
            kubectl_manager: tokio::sync::Mutex::new(KubectlPortForwardManager::new()),
            gateway_router: tokio::sync::Mutex::new(GatewayRouter::new()),
            active_k8s_connections: active_k8s_connections.clone(),
            tls_manager,
        });

        // Create SOCKS5 proxy if enabled, with ClickHouse for protocol interception
        // and shared K8s connection tracking
        let socks_proxy = if config.enable_socks {
            let socks = SocksProxy::with_clickhouse_and_tracking(
                config.socks.clone(),
                RoxyClickHouse::new(config.clickhouse.clone()),
                active_k8s_connections.clone(),
            );
            Some(Arc::new(socks))
        } else {
            None
        };

        // Clone tls_manager from state for the ProxyServer struct
        let tls_manager = state.tls_manager.clone();

        Ok(Self {
            config,
            service_manager: ServiceManager::new(),
            state,
            running: Arc::new(AtomicBool::new(false)),
            socks_proxy,
            active_k8s_connections,
            mcp_server: None,
            tls_manager,
        })
    }

    /// Create a new proxy server with default configuration
    pub async fn with_defaults() -> Result<Self> {
        Self::new(ProxyConfig::default()).await
    }

    /// Setup and start backend services (ClickHouse, OTel)
    pub async fn setup_services(&mut self) -> Result<()> {
        if !self.config.start_services {
            info!("Skipping service startup (disabled in config)");
            return Ok(());
        }

        info!("Starting backend services...");

        // Start all backend services
        self.service_manager
            .setup()
            .await
            .context("Failed to setup service directories")?;

        self.service_manager
            .install_all()
            .await
            .context("Failed to install services")?;

        self.service_manager
            .start_all()
            .await
            .context("Failed to start services")?;

        // Wait for ClickHouse to be ready
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        // Initialize ClickHouse schema
        self.state
            .clickhouse
            .initialize_schema()
            .await
            .context("Failed to initialize ClickHouse schema")?;

        info!("Backend services started successfully");
        Ok(())
    }

    /// Run the proxy server
    pub async fn run(&mut self) -> Result<()> {
        self.running.store(true, Ordering::SeqCst);

        // Start MCP server if enabled
        if self.config.enable_mcp {
            match roxy_mcp::run_sse(self.config.mcp.clone()).await {
                Ok(mcp_server) => {
                    self.mcp_server = Some(mcp_server);
                }
                Err(e) => {
                    warn!("Failed to start MCP server: {}", e);
                }
            }
        }

        let addr = SocketAddr::from(([127, 0, 0, 1], self.config.port));
        let listener = TcpListener::bind(addr)
            .await
            .context("Failed to bind to address")?;

        if self.config.configure_system_proxy {
            if let Err(e) = system_proxy::set_system_proxy(self.config.port) {
                warn!("Failed to configure system proxy: {}", e);
            }
        }

        // Start SOCKS5 proxy in a separate task if enabled
        if let Some(socks) = &self.socks_proxy {
            let socks = socks.clone();
            tokio::spawn(async move {
                if let Err(e) = socks.run().await {
                    error!("SOCKS5 proxy error: {}", e);
                }
            });
        }

        info!("=================================================");
        info!("  Roxy Proxy is running!");
        info!("  HTTP Proxy: http://127.0.0.1:{}", self.config.port);
        if self.config.enable_socks {
            info!(
                "  SOCKS5:     socks5://127.0.0.1:{}",
                self.config.socks.port
            );
        }
        if self.config.enable_mcp {
            info!(
                "  MCP:        http://127.0.0.1:{}/sse",
                self.config.mcp.bind_address.port()
            );
        }
        info!("  ClickHouse: http://127.0.0.1:8123");
        info!("  OTel:       http://127.0.0.1:4317 (gRPC)");
        if let Some(tls) = &self.tls_manager {
            info!("  TLS:        Interception enabled (for configured hosts)");
            info!("  CA Cert:    {}", tls.ca_cert_path().await);
            let hosts = tls.intercept_hosts().await;
            if !hosts.is_empty() {
                info!("  Intercept:  {} host(s) configured", hosts.len());
            }
        } else {
            info!("  TLS:        Passthrough (no interception)");
        }
        info!("=================================================");

        loop {
            let (stream, client_addr) = match listener.accept().await {
                Ok(conn) => conn,
                Err(e) => {
                    error!(error = %e, "Failed to accept connection");
                    continue;
                }
            };

            let io = TokioIo::new(stream);
            let state = self.state.clone();

            tokio::task::spawn(async move {
                let service = service_fn(move |req| {
                    let state = state.clone();
                    async move { handle_request(req, client_addr, state).await }
                });

                if let Err(err) = http1::Builder::new()
                    .preserve_header_case(true)
                    .title_case_headers(true)
                    .serve_connection(io, service)
                    .with_upgrades()
                    .await
                {
                    debug!(error = %err, "Connection error");
                }
            });
        }
    }

    /// Check if the proxy is currently running
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Get the current request count
    pub fn request_count(&self) -> u64 {
        self.state.request_count.load(Ordering::Relaxed)
    }

    /// Get the current tunnel count
    pub fn tunnel_count(&self) -> u64 {
        self.state.tunnel_count.load(Ordering::Relaxed)
    }

    /// Get the configured port
    pub fn port(&self) -> u16 {
        self.config.port
    }

    /// Get the ClickHouse client for querying data
    /// Get the ClickHouse client
    pub fn clickhouse(&self) -> &RoxyClickHouse {
        &self.state.clickhouse
    }

    /// Get the SOCKS proxy (if enabled)
    pub fn socks_proxy(&self) -> Option<&Arc<SocksProxy>> {
        self.socks_proxy.as_ref()
    }

    /// Get the shared active K8s connections tracker
    pub fn active_k8s_connections(&self) -> &Arc<ActiveK8sConnections> {
        &self.active_k8s_connections
    }

    /// Add a port forward mapping to the SOCKS5 proxy
    ///
    /// This allows routing connections to Kubernetes DNS names (e.g.,
    /// `postgres.database.svc.cluster.local`) to local addresses.
    pub async fn add_socks_port_forward(&self, dns_name: String, local_addr: SocketAddr) {
        if let Some(socks) = &self.socks_proxy {
            socks.add_port_forward(dns_name, local_addr).await;
        } else {
            warn!("Cannot add port forward: SOCKS5 proxy is not enabled");
        }
    }

    /// Remove a port forward mapping from the SOCKS5 proxy
    pub async fn remove_socks_port_forward(&self, dns_name: &str) {
        if let Some(socks) = &self.socks_proxy {
            socks.remove_port_forward(dns_name).await;
        }
    }

    /// Get the TLS manager (if TLS interception is enabled)
    pub fn tls_manager(&self) -> Option<&Arc<TlsManager>> {
        self.tls_manager.as_ref()
    }

    /// Add a host to the TLS interception list
    ///
    /// Traffic to this host will be decrypted, inspected, and re-encrypted.
    /// The client must trust the Roxy CA certificate.
    pub async fn add_tls_intercept_host(&self, host: String) {
        if let Some(tls) = &self.tls_manager {
            tls.add_intercept_host(host.clone()).await;
            info!(host = %host, "Added host to TLS interception list");
        } else {
            warn!("Cannot add TLS intercept host: TLS interception is not enabled");
        }
    }

    /// Remove a host from the TLS interception list
    ///
    /// Traffic to this host will be tunneled through without interception.
    pub async fn remove_tls_intercept_host(&self, host: &str) {
        if let Some(tls) = &self.tls_manager {
            tls.remove_intercept_host(host).await;
            info!(host = %host, "Removed host from TLS interception list");
        }
    }

    /// Get the list of hosts configured for TLS interception
    pub async fn tls_intercept_hosts(&self) -> Vec<String> {
        if let Some(tls) = &self.tls_manager {
            tls.intercept_hosts().await
        } else {
            Vec::new()
        }
    }

    /// Get the CA certificate in PEM format
    ///
    /// This certificate should be installed in client trust stores
    /// to enable TLS interception without certificate warnings.
    pub fn ca_cert_pem(&self) -> Option<&str> {
        self.tls_manager.as_ref().map(|tls| tls.ca_cert_pem())
    }

    /// Get the path to the CA certificate file
    pub async fn ca_cert_path(&self) -> Option<String> {
        if let Some(tls) = &self.tls_manager {
            Some(tls.ca_cert_path().await)
        } else {
            None
        }
    }

    /// Check if TLS interception is enabled
    pub fn tls_interception_enabled(&self) -> bool {
        self.tls_manager.is_some()
    }

    /// Stop backend services
    pub async fn stop_services(&mut self) -> Result<()> {
        // Stop MCP server if running
        if let Some(mcp_server) = self.mcp_server.take() {
            info!("Stopping MCP server...");
            mcp_server.stop().await;
        }

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

/// Convenience function to run a proxy with default configuration
pub async fn run_proxy(config: ProxyConfig) -> Result<()> {
    let mut server = ProxyServer::new(config).await?;
    server.setup_services().await?;

    // Handle Ctrl+C gracefully
    let running = server.running.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        info!("Shutting down...");
        running.store(false, Ordering::SeqCst);
    });

    server.run().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = ProxyConfig::default();
        assert_eq!(config.port, DEFAULT_PROXY_PORT);
        assert!(!config.configure_system_proxy);
        assert!(config.start_services);
        assert!(config.enable_socks);
        assert_eq!(config.socks.port, DEFAULT_SOCKS_PORT);
    }

    #[tokio::test]
    async fn test_active_k8s_connections_add_remove() {
        let tracker = ActiveK8sConnections::new();
        let service = K8sService {
            name: "postgres".to_string(),
            namespace: "database".to_string(),
            port: 5432,
        };

        // Initially empty
        assert_eq!(tracker.count().await, 0);

        // Add a connection
        tracker.add_connection(&service).await;
        assert_eq!(tracker.count().await, 1);

        let conns = tracker.list().await;
        assert_eq!(conns.len(), 1);
        assert_eq!(conns[0].service_name, "postgres");
        assert_eq!(conns[0].namespace, "database");
        assert_eq!(conns[0].port, 5432);
        assert_eq!(conns[0].connection_count, 1);

        // Add another connection to the same service
        tracker.add_connection(&service).await;
        assert_eq!(tracker.count().await, 1); // Still 1 service
        let conns = tracker.list().await;
        assert_eq!(conns[0].connection_count, 2);

        // Remove one connection
        tracker.remove_connection(&service).await;
        assert_eq!(tracker.count().await, 1);
        let conns = tracker.list().await;
        assert_eq!(conns[0].connection_count, 1);

        // Remove the last connection - entry is kept for 5 minutes
        tracker.remove_connection(&service).await;
        assert_eq!(tracker.count().await, 1); // Entry still exists
        let conns = tracker.list().await;
        assert_eq!(conns[0].connection_count, 0); // But with 0 active connections
    }

    #[tokio::test]
    async fn test_active_k8s_connections_multiple_services() {
        let tracker = ActiveK8sConnections::new();
        let postgres = K8sService {
            name: "postgres".to_string(),
            namespace: "database".to_string(),
            port: 5432,
        };
        let kafka = K8sService {
            name: "kafka".to_string(),
            namespace: "messaging".to_string(),
            port: 9092,
        };

        tracker.add_connection(&postgres).await;
        tracker.add_connection(&kafka).await;

        assert_eq!(tracker.count().await, 2);

        let conns = tracker.list().await;
        assert_eq!(conns.len(), 2);

        // Remove one - entry is kept for 5 minutes with connection_count = 0
        tracker.remove_connection(&postgres).await;
        assert_eq!(tracker.count().await, 2); // Both entries still exist

        let conns = tracker.list().await;
        assert_eq!(conns.len(), 2);

        // Find the postgres entry and verify it has 0 connections
        let postgres_conn = conns.iter().find(|c| c.service_name == "postgres").unwrap();
        assert_eq!(postgres_conn.connection_count, 0);

        // Kafka should still have 1 connection
        let kafka_conn = conns.iter().find(|c| c.service_name == "kafka").unwrap();
        assert_eq!(kafka_conn.connection_count, 1);
    }

    #[test]
    fn test_headers_to_json() {
        let mut headers = hyper::HeaderMap::new();
        headers.insert("content-type", "application/json".parse().unwrap());
        headers.insert("x-custom", "test".parse().unwrap());

        let json = headers_to_json(&headers);
        assert!(json.contains("content-type"));
        assert!(json.contains("application/json"));
    }

    #[test]
    fn test_body_to_string() {
        let bytes = Bytes::from("hello world");
        let result = body_to_string(&bytes);
        assert_eq!(result, "hello world");

        let empty = Bytes::new();
        let result = body_to_string(&empty);
        assert_eq!(result, "");
    }

    #[test]
    fn test_extract_client_name_x_client_name() {
        let mut headers = hyper::HeaderMap::new();
        headers.insert("x-client-name", "my-frontend".parse().unwrap());
        headers.insert("user-agent", "curl/7.68.0".parse().unwrap());

        let name = extract_client_name(&headers);
        assert_eq!(name, "my-frontend");
    }

    #[test]
    fn test_extract_client_name_x_application_name() {
        let mut headers = hyper::HeaderMap::new();
        headers.insert("x-application-name", "backend-api".parse().unwrap());
        headers.insert("user-agent", "curl/7.68.0".parse().unwrap());

        let name = extract_client_name(&headers);
        assert_eq!(name, "backend-api");
    }

    #[test]
    fn test_extract_client_name_user_agent() {
        let mut headers = hyper::HeaderMap::new();
        headers.insert("user-agent", "MyApp/1.0.0".parse().unwrap());

        let name = extract_client_name(&headers);
        assert_eq!(name, "MyApp");
    }

    #[test]
    fn test_extract_client_name_user_agent_with_space() {
        let mut headers = hyper::HeaderMap::new();
        headers.insert("user-agent", "Mozilla compatible".parse().unwrap());

        let name = extract_client_name(&headers);
        assert_eq!(name, "Mozilla");
    }

    #[test]
    fn test_extract_client_name_empty() {
        let headers = hyper::HeaderMap::new();
        let name = extract_client_name(&headers);
        assert_eq!(name, "");
    }
}
