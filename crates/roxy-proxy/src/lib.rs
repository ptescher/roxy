//! Roxy Proxy Library - HTTP forwarding proxy with request logging
//!
//! This library provides a simple forwarding proxy that:
//! - Forwards HTTP requests and logs them to ClickHouse
//! - Tunnels HTTPS (CONNECT) requests transparently without TLS interception
//! - Provides a SOCKS5 proxy for TCP connections (PostgreSQL, Kafka, etc.)
//!
//! This approach avoids certificate issues with iOS/mobile apps.

pub mod kubectl;
pub mod protocol;
pub mod socks;

pub use kubectl::{K8sService, K8sStream, KubectlPortForwardManager};
pub use socks::{SocksConfig, SocksProxy, DEFAULT_SOCKS_PORT};

use anyhow::{Context, Result};
use chrono::Utc;
use http_body_util::{BodyExt, Empty, Full};
use hyper::body::Bytes;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use roxy_core::{services::ServiceManager, ClickHouseConfig, HttpRequestRecord, RoxyClickHouse};
use std::collections::HashMap;
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
        }
    }
}

/// Shared state for the proxy
struct ProxyState {
    clickhouse: RoxyClickHouse,
    request_count: AtomicU64,
    /// Active tunnel count for monitoring
    tunnel_count: AtomicU64,
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
async fn handle_connect(
    req: Request<hyper::body::Incoming>,
    client_addr: SocketAddr,
    state: Arc<ProxyState>,
) -> Result<Response<BoxBody>, hyper::Error> {
    let host = req.uri().authority().map(|a| a.to_string());

    if let Some(addr) = host {
        state.tunnel_count.fetch_add(1, Ordering::Relaxed);

        debug!(
            client = %client_addr,
            target = %addr,
            "Establishing HTTPS tunnel"
        );

        // Spawn a task to handle the tunnel
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

        // Return 200 OK to indicate tunnel established
        Ok(Response::new(empty()))
    } else {
        let mut resp = Response::new(full("CONNECT must specify a host"));
        *resp.status_mut() = StatusCode::BAD_REQUEST;
        Ok(resp)
    }
}

/// Handle regular HTTP forward proxy requests
async fn handle_http_forward(
    req: Request<hyper::body::Incoming>,
    client_addr: SocketAddr,
    state: Arc<ProxyState>,
) -> Result<Response<BoxBody>, hyper::Error> {
    let request_id = Uuid::new_v4();
    let trace_id = format!("{:032x}", rand::random::<u128>());
    let span_id = format!("{:016x}", rand::random::<u64>());
    let timestamp = Utc::now().timestamp_millis();

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

    // Connect to target and forward request
    let target_addr = format!("{}:{}", host, port);

    match TcpStream::connect(&target_addr).await {
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
    /// SOCKS5 proxy for TCP connections
    socks_proxy: Option<Arc<SocksProxy>>,
}

impl ProxyServer {
    /// Create a new proxy server with the given configuration
    pub async fn new(config: ProxyConfig) -> Result<Self> {
        let clickhouse = RoxyClickHouse::new(config.clickhouse.clone());

        let state = Arc::new(ProxyState {
            clickhouse,
            request_count: AtomicU64::new(0),
            tunnel_count: AtomicU64::new(0),
        });

        // Create SOCKS5 proxy if enabled, with ClickHouse for protocol interception
        let socks_proxy = if config.enable_socks {
            let socks = SocksProxy::with_clickhouse(
                config.socks.clone(),
                RoxyClickHouse::new(config.clickhouse.clone()),
            );
            Some(Arc::new(socks))
        } else {
            None
        };

        Ok(Self {
            config,
            service_manager: ServiceManager::new(),
            state,
            running: Arc::new(AtomicBool::new(false)),
            socks_proxy,
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
    pub async fn run(&self) -> Result<()> {
        self.running.store(true, Ordering::SeqCst);

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
        info!("  ClickHouse: http://127.0.0.1:8123");
        info!("  OTel:       http://127.0.0.1:4317 (gRPC)");
        info!("  Mode:       Forwarding (no TLS interception)");
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
    pub fn clickhouse(&self) -> &RoxyClickHouse {
        &self.state.clickhouse
    }

    /// Get the SOCKS5 proxy (if enabled)
    pub fn socks_proxy(&self) -> Option<&Arc<SocksProxy>> {
        self.socks_proxy.as_ref()
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
}
