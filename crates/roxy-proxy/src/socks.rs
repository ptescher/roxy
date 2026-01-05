//! SOCKS5 Proxy Implementation for Roxy
//!
//! This module implements a SOCKS5 proxy server (RFC 1928) that can handle
//! arbitrary TCP connections, enabling proxying of non-HTTP protocols like
//! PostgreSQL, Kafka, Redis, etc.
//!
//! ## Usage
//!
//! The SOCKS5 proxy runs on a separate port (default: 1080) and can be used
//! by applications that support SOCKS5 proxying.
//!
//! For Node.js/PostgreSQL:
//! ```bash
//! npm install socks
//! ```
//!
//! Then configure your database connection to use the SOCKS proxy.

use crate::kubectl::{K8sService, KubectlPortForwardManager};
use crate::protocol::run_intercepting_tunnel_generic;
use crate::protocol::{run_intercepting_tunnel, InterceptConfig, Protocol};
use roxy_core::RoxyClickHouse;
use std::collections::HashMap;
use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::RwLock;
use tracing::{debug, error, info, trace, warn};

/// Default SOCKS5 proxy port
pub const DEFAULT_SOCKS_PORT: u16 = 1080;

/// SOCKS5 protocol version
const SOCKS_VERSION: u8 = 0x05;

/// SOCKS5 authentication methods
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(u8)]
pub enum AuthMethod {
    /// No authentication required
    NoAuth = 0x00,
    /// GSSAPI
    GssApi = 0x01,
    /// Username/Password
    UsernamePassword = 0x02,
    /// No acceptable methods
    NoAcceptable = 0xFF,
}

/// SOCKS5 commands
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(u8)]
pub enum Command {
    /// Establish a TCP/IP stream connection
    Connect = 0x01,
    /// Establish a TCP/IP port binding
    Bind = 0x02,
    /// Associate a UDP port
    UdpAssociate = 0x03,
}

impl TryFrom<u8> for Command {
    type Error = SocksError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x01 => Ok(Command::Connect),
            0x02 => Ok(Command::Bind),
            0x03 => Ok(Command::UdpAssociate),
            _ => Err(SocksError::InvalidCommand(value)),
        }
    }
}

/// SOCKS5 address types
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(u8)]
pub enum AddressType {
    /// IPv4 address
    IPv4 = 0x01,
    /// Domain name
    DomainName = 0x03,
    /// IPv6 address
    IPv6 = 0x04,
}

impl TryFrom<u8> for AddressType {
    type Error = SocksError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x01 => Ok(AddressType::IPv4),
            0x03 => Ok(AddressType::DomainName),
            0x04 => Ok(AddressType::IPv6),
            _ => Err(SocksError::InvalidAddressType(value)),
        }
    }
}

/// SOCKS5 reply codes
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(u8)]
pub enum Reply {
    Succeeded = 0x00,
    GeneralFailure = 0x01,
    ConnectionNotAllowed = 0x02,
    NetworkUnreachable = 0x03,
    HostUnreachable = 0x04,
    ConnectionRefused = 0x05,
    TtlExpired = 0x06,
    CommandNotSupported = 0x07,
    AddressTypeNotSupported = 0x08,
}

/// Target address for SOCKS5 connection
#[derive(Debug, Clone)]
pub enum TargetAddr {
    /// IPv4 address
    Ip(IpAddr),
    /// Domain name
    Domain(String),
}

impl std::fmt::Display for TargetAddr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TargetAddr::Ip(ip) => write!(f, "{}", ip),
            TargetAddr::Domain(domain) => write!(f, "{}", domain),
        }
    }
}

/// SOCKS5 errors
#[derive(Debug, thiserror::Error)]
pub enum SocksError {
    #[error("Invalid SOCKS version: {0}")]
    InvalidVersion(u8),

    #[error("Invalid command: {0}")]
    InvalidCommand(u8),

    #[error("Invalid address type: {0}")]
    InvalidAddressType(u8),

    #[error("No acceptable authentication method")]
    NoAcceptableAuth,

    #[error("Authentication failed")]
    AuthFailed,

    #[error("Command not supported: {0:?}")]
    CommandNotSupported(Command),

    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    #[error("DNS resolution failed for {0}")]
    DnsResolutionFailed(String),

    #[error("Kubernetes error: {0}")]
    Kube(#[from] crate::kubectl::KubeError),
}

/// Port forward mapping for Kubernetes services
#[derive(Debug, Clone)]
pub struct PortForwardMapping {
    /// The Kubernetes DNS name (e.g., "postgres.database.svc.cluster.local")
    pub dns_name: String,
    /// The local address to forward to
    pub local_addr: SocketAddr,
}

/// SOCKS5 proxy configuration
#[derive(Debug, Clone)]
pub struct SocksConfig {
    /// Port to listen on
    pub port: u16,
    /// Bind address
    pub bind_addr: IpAddr,
    /// Whether to require authentication
    pub require_auth: bool,
    /// Username for authentication (if required)
    pub username: Option<String>,
    /// Password for authentication (if required)
    pub password: Option<String>,
}

impl Default for SocksConfig {
    fn default() -> Self {
        Self {
            port: DEFAULT_SOCKS_PORT,
            bind_addr: IpAddr::V4(Ipv4Addr::LOCALHOST),
            require_auth: false,
            username: None,
            password: None,
        }
    }
}

/// Statistics for the SOCKS5 proxy
#[derive(Debug, Default)]
pub struct SocksStats {
    /// Total connections handled
    pub connections: AtomicU64,
    /// Active connections
    pub active_connections: AtomicU64,
    /// Total bytes sent
    pub bytes_sent: AtomicU64,
    /// Total bytes received
    pub bytes_received: AtomicU64,
    /// Failed connections
    pub failed_connections: AtomicU64,
    /// PostgreSQL connections detected
    pub postgres_connections: AtomicU64,
    /// Kafka connections detected
    pub kafka_connections: AtomicU64,
    /// Database queries recorded
    pub database_queries: AtomicU64,
    /// Kafka messages recorded
    pub kafka_messages: AtomicU64,
}

/// SOCKS5 proxy server
pub struct SocksProxy {
    config: SocksConfig,
    /// Port forward mappings for Kubernetes services
    port_forwards: Arc<RwLock<HashMap<String, SocketAddr>>>,
    /// Statistics
    stats: Arc<SocksStats>,
    /// Kubectl port-forward manager for automatic K8s service forwarding
    kubectl_manager: Arc<RwLock<KubectlPortForwardManager>>,
    /// ClickHouse client for recording protocol data
    clickhouse: Option<Arc<RoxyClickHouse>>,
    /// Configuration for protocol interception
    intercept_config: InterceptConfig,
}

impl SocksProxy {
    /// Create a new SOCKS5 proxy with the given configuration
    pub fn new(config: SocksConfig) -> Self {
        Self {
            config,
            port_forwards: Arc::new(RwLock::new(HashMap::new())),
            stats: Arc::new(SocksStats::default()),
            kubectl_manager: Arc::new(RwLock::new(KubectlPortForwardManager::new())),
            clickhouse: None,
            intercept_config: InterceptConfig::default(),
        }
    }

    /// Create a new SOCKS5 proxy with default configuration
    pub fn with_defaults() -> Self {
        Self::new(SocksConfig::default())
    }

    /// Create a new SOCKS5 proxy with ClickHouse support for protocol recording
    pub fn with_clickhouse(config: SocksConfig, clickhouse: RoxyClickHouse) -> Self {
        Self {
            config,
            port_forwards: Arc::new(RwLock::new(HashMap::new())),
            stats: Arc::new(SocksStats::default()),
            kubectl_manager: Arc::new(RwLock::new(KubectlPortForwardManager::new())),
            clickhouse: Some(Arc::new(clickhouse)),
            intercept_config: InterceptConfig::default(),
        }
    }

    /// Set the ClickHouse client for protocol recording
    pub fn set_clickhouse(&mut self, clickhouse: RoxyClickHouse) {
        self.clickhouse = Some(Arc::new(clickhouse));
    }

    /// Set the intercept configuration
    pub fn set_intercept_config(&mut self, config: InterceptConfig) {
        self.intercept_config = config;
    }

    /// Add a port forward mapping
    ///
    /// When a connection is requested to `dns_name`, it will be forwarded to `local_addr`
    pub async fn add_port_forward(&self, dns_name: String, local_addr: SocketAddr) {
        let mut forwards = self.port_forwards.write().await;
        info!("Adding SOCKS port forward: {} -> {}", dns_name, local_addr);
        forwards.insert(dns_name, local_addr);
    }

    /// Remove a port forward mapping
    pub async fn remove_port_forward(&self, dns_name: &str) {
        let mut forwards = self.port_forwards.write().await;
        forwards.remove(dns_name);
    }

    /// Get all port forward mappings
    pub async fn get_port_forwards(&self) -> HashMap<String, SocketAddr> {
        self.port_forwards.read().await.clone()
    }

    /// Get statistics
    pub fn stats(&self) -> &SocksStats {
        &self.stats
    }

    /// Get the kubectl port-forward manager
    pub fn kubectl_manager(&self) -> &Arc<RwLock<KubectlPortForwardManager>> {
        &self.kubectl_manager
    }

    /// Resolve a target address, checking port forwards first
    #[allow(dead_code)]
    async fn resolve_target(&self, addr: &TargetAddr, port: u16) -> Result<SocketAddr, SocksError> {
        match addr {
            TargetAddr::Ip(ip) => Ok(SocketAddr::new(*ip, port)),
            TargetAddr::Domain(domain) => {
                // Check if we have a port forward for this domain
                let domain_with_port = format!("{}:{}", domain, port);

                let forwards = self.port_forwards.read().await;

                // Try exact match with port
                if let Some(local_addr) = forwards.get(&domain_with_port) {
                    debug!("Port forward match: {} -> {}", domain_with_port, local_addr);
                    return Ok(*local_addr);
                }

                // Try match without port
                if let Some(local_addr) = forwards.get(domain) {
                    debug!("Port forward match: {} -> {}", domain, local_addr);
                    // Use the mapped address but potentially different port
                    return Ok(SocketAddr::new(local_addr.ip(), port));
                }

                // Check for wildcard matches (e.g., *.svc.cluster.local)
                for (pattern, local_addr) in forwards.iter() {
                    if pattern.starts_with("*.") {
                        let suffix = &pattern[1..]; // Remove the '*'
                        if domain.ends_with(suffix) {
                            debug!(
                                "Wildcard port forward match: {} ({}) -> {}",
                                domain, pattern, local_addr
                            );
                            return Ok(SocketAddr::new(local_addr.ip(), port));
                        }
                    }
                }

                drop(forwards);

                // No port forward, try DNS resolution
                debug!("No port forward for {}, attempting DNS resolution", domain);

                let addr_str = format!("{}:{}", domain, port);
                let resolved = tokio::net::lookup_host(addr_str.as_str())
                    .await
                    .ok()
                    .and_then(|mut addrs| addrs.next());

                match resolved {
                    Some(addr) => Ok(addr),
                    None => {
                        warn!("DNS resolution failed for {}", domain);
                        Err(SocksError::DnsResolutionFailed(addr_str))
                    }
                }
            }
        }
    }

    /// Run the SOCKS5 proxy server
    pub async fn run(&self) -> Result<(), SocksError> {
        let addr = SocketAddr::new(self.config.bind_addr, self.config.port);
        let listener = TcpListener::bind(addr).await?;

        info!("SOCKS5 proxy listening on {}", addr);
        if self.clickhouse.is_some() {
            info!("Protocol interception enabled (PostgreSQL, Kafka)");
        }

        loop {
            match listener.accept().await {
                Ok((stream, client_addr)) => {
                    self.stats.connections.fetch_add(1, Ordering::Relaxed);
                    self.stats
                        .active_connections
                        .fetch_add(1, Ordering::Relaxed);

                    let port_forwards = self.port_forwards.clone();
                    let stats = self.stats.clone();
                    let config = self.config.clone();
                    let kubectl_manager = self.kubectl_manager.clone();
                    let clickhouse = self.clickhouse.clone();
                    let intercept_config = self.intercept_config.clone();

                    tokio::spawn(async move {
                        if let Err(e) = handle_client(
                            stream,
                            client_addr,
                            port_forwards,
                            stats.clone(),
                            config,
                            kubectl_manager,
                            clickhouse,
                            intercept_config,
                        )
                        .await
                        {
                            debug!("Client {} error: {}", client_addr, e);
                            stats.failed_connections.fetch_add(1, Ordering::Relaxed);
                        }
                        stats.active_connections.fetch_sub(1, Ordering::Relaxed);
                    });
                }
                Err(e) => {
                    error!("Failed to accept connection: {}", e);
                }
            }
        }
    }
}

/// Handle a single SOCKS5 client connection
async fn handle_client(
    mut stream: TcpStream,
    client_addr: SocketAddr,
    port_forwards: Arc<RwLock<HashMap<String, SocketAddr>>>,
    stats: Arc<SocksStats>,
    config: SocksConfig,
    kubectl_manager: Arc<RwLock<KubectlPortForwardManager>>,
    clickhouse: Option<Arc<RoxyClickHouse>>,
    intercept_config: InterceptConfig,
) -> Result<(), SocksError> {
    debug!("New SOCKS5 connection from {}", client_addr);

    // Step 1: Handle authentication negotiation
    let auth_method = negotiate_auth(&mut stream, &config).await?;
    debug!("Negotiated auth method: {:?}", auth_method);

    // Step 2: Perform authentication if required
    if auth_method == AuthMethod::UsernamePassword {
        authenticate(&mut stream, &config).await?;
    }

    // Step 3: Handle the request
    let (command, target_addr, target_port) = read_request(&mut stream).await?;
    debug!(
        "SOCKS5 request: {:?} to {}:{}",
        command, target_addr, target_port
    );

    // We only support CONNECT for now
    if command != Command::Connect {
        send_reply(&mut stream, Reply::CommandNotSupported, None).await?;
        return Err(SocksError::CommandNotSupported(command));
    }

    // Check if this is a Kubernetes service - use direct stream to avoid race condition
    if let TargetAddr::Domain(domain) = &target_addr {
        if let Some(k8s_service) = K8sService::from_dns_name(domain, target_port) {
            debug!(
                "Detected K8s service: {}.{} port {} - using direct stream",
                k8s_service.name, k8s_service.namespace, k8s_service.port
            );

            // Get a direct stream from Kubernetes (this waits for the connection to be ready)
            // We acquire the write lock only for the duration of getting the K8sStream,
            // then release it before running the tunnel to avoid blocking other connections.
            let mut k8s_stream = {
                let mut manager = kubectl_manager.write().await;
                manager.get_direct_stream(k8s_service).await?
                // Manager lock is dropped here
            };
            trace!("Released kubectl_manager write lock");

            // Take the stream - this must succeed since we just created it
            let target_stream = k8s_stream.take_stream().ok_or_else(|| {
                SocksError::ConnectionFailed("Failed to get K8s stream".to_string())
            })?;

            // Use localhost as the bound address for SOCKS reply
            let bound_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), target_port);

            info!(
                "SOCKS5 tunnel (K8s direct): {} -> {}:{}",
                client_addr, target_addr, target_port
            );

            // Send success reply and flush to ensure client receives it
            send_reply(&mut stream, Reply::Succeeded, Some(bound_addr)).await?;
            stream.flush().await?;

            // Start bidirectional copy with protocol interception using generic version
            let target_host = format!("{}:{}", target_addr, target_port);
            let intercept_stats = run_intercepting_tunnel_generic(
                stream,
                target_stream,
                client_addr,
                bound_addr,
                target_port,
                target_host,
                clickhouse.clone(),
                intercept_config.clone(),
            )
            .await;

            // Update stats
            update_stats(&stats, &intercept_stats);

            debug!(
                "K8s tunnel closed: {} -> {}:{} (sent: {}, received: {}, protocol: {:?})",
                client_addr,
                target_addr,
                target_port,
                intercept_stats.bytes_sent,
                intercept_stats.bytes_received,
                intercept_stats.protocol
            );

            // Clean up the K8s stream
            if let Err(e) = k8s_stream.join().await {
                debug!("K8s stream join error (may be expected): {}", e);
            }

            return Ok(());
        }
    }

    // Step 4: Resolve the target address (for non-K8s targets)
    let resolved_addr =
        resolve_target_with_forwards(&target_addr, target_port, &port_forwards, &kubectl_manager)
            .await;

    match resolved_addr {
        Ok(addr) => {
            // Step 5: Connect to target
            match TcpStream::connect(addr).await {
                Ok(target_stream) => {
                    info!(
                        "SOCKS5 tunnel: {} -> {}:{} (resolved: {})",
                        client_addr, target_addr, target_port, addr
                    );

                    // Send success reply
                    send_reply(&mut stream, Reply::Succeeded, Some(addr)).await?;

                    // Step 6: Start bidirectional copy with protocol interception
                    let target_host = format!("{}:{}", target_addr, target_port);
                    let intercept_stats = run_intercepting_tunnel(
                        stream,
                        target_stream,
                        client_addr,
                        addr,
                        target_port,
                        target_host,
                        clickhouse,
                        intercept_config,
                    )
                    .await;

                    // Update stats from interception
                    update_stats(&stats, &intercept_stats);

                    debug!(
                        "Tunnel closed: {} -> {}:{} (sent: {}, received: {}, protocol: {:?})",
                        client_addr,
                        target_addr,
                        target_port,
                        intercept_stats.bytes_sent,
                        intercept_stats.bytes_received,
                        intercept_stats.protocol
                    );
                }
                Err(e) => {
                    warn!(
                        "Failed to connect to {}:{} ({}): {}",
                        target_addr, target_port, addr, e
                    );

                    let reply = match e.kind() {
                        io::ErrorKind::ConnectionRefused => Reply::ConnectionRefused,
                        io::ErrorKind::TimedOut => Reply::TtlExpired,
                        _ => Reply::HostUnreachable,
                    };
                    send_reply(&mut stream, reply, None).await?;
                    return Err(SocksError::ConnectionFailed(e.to_string()));
                }
            }
        }
        Err(e) => {
            warn!("Failed to resolve {}:{}: {}", target_addr, target_port, e);
            send_reply(&mut stream, Reply::HostUnreachable, None).await?;
            return Err(e);
        }
    }

    Ok(())
}

/// Negotiate authentication method with the client
async fn negotiate_auth(
    stream: &mut TcpStream,
    config: &SocksConfig,
) -> Result<AuthMethod, SocksError> {
    // Read version and number of methods
    let mut header = [0u8; 2];
    stream.read_exact(&mut header).await?;

    let version = header[0];
    let nmethods = header[1] as usize;

    if version != SOCKS_VERSION {
        return Err(SocksError::InvalidVersion(version));
    }

    // Read methods
    let mut methods = vec![0u8; nmethods];
    stream.read_exact(&mut methods).await?;

    // Select authentication method
    let selected = if config.require_auth {
        if methods.contains(&(AuthMethod::UsernamePassword as u8)) {
            AuthMethod::UsernamePassword
        } else {
            AuthMethod::NoAcceptable
        }
    } else if methods.contains(&(AuthMethod::NoAuth as u8)) {
        AuthMethod::NoAuth
    } else if methods.contains(&(AuthMethod::UsernamePassword as u8)) {
        AuthMethod::UsernamePassword
    } else {
        AuthMethod::NoAcceptable
    };

    // Send response
    let response = [SOCKS_VERSION, selected as u8];
    stream.write_all(&response).await?;

    if selected == AuthMethod::NoAcceptable {
        return Err(SocksError::NoAcceptableAuth);
    }

    Ok(selected)
}

/// Perform username/password authentication
async fn authenticate(stream: &mut TcpStream, config: &SocksConfig) -> Result<(), SocksError> {
    // Read auth version
    let mut version = [0u8; 1];
    stream.read_exact(&mut version).await?;

    if version[0] != 0x01 {
        // Username/password auth version
        stream.write_all(&[0x01, 0x01]).await?; // Version 1, failure
        return Err(SocksError::AuthFailed);
    }

    // Read username
    let mut ulen = [0u8; 1];
    stream.read_exact(&mut ulen).await?;
    let mut username = vec![0u8; ulen[0] as usize];
    stream.read_exact(&mut username).await?;

    // Read password
    let mut plen = [0u8; 1];
    stream.read_exact(&mut plen).await?;
    let mut password = vec![0u8; plen[0] as usize];
    stream.read_exact(&mut password).await?;

    let username = String::from_utf8_lossy(&username);
    let password = String::from_utf8_lossy(&password);

    // Verify credentials
    let valid = match (&config.username, &config.password) {
        (Some(expected_user), Some(expected_pass)) => {
            username == *expected_user && password == *expected_pass
        }
        _ => false,
    };

    if valid {
        stream.write_all(&[0x01, 0x00]).await?; // Version 1, success
        Ok(())
    } else {
        stream.write_all(&[0x01, 0x01]).await?; // Version 1, failure
        Err(SocksError::AuthFailed)
    }
}

/// Read SOCKS5 request
async fn read_request(stream: &mut TcpStream) -> Result<(Command, TargetAddr, u16), SocksError> {
    // Read request header: VER, CMD, RSV, ATYP
    let mut header = [0u8; 4];
    stream.read_exact(&mut header).await?;

    let version = header[0];
    let command = Command::try_from(header[1])?;
    let _reserved = header[2];
    let addr_type = AddressType::try_from(header[3])?;

    if version != SOCKS_VERSION {
        return Err(SocksError::InvalidVersion(version));
    }

    // Read target address based on type
    let target_addr = match addr_type {
        AddressType::IPv4 => {
            let mut addr = [0u8; 4];
            stream.read_exact(&mut addr).await?;
            TargetAddr::Ip(IpAddr::V4(Ipv4Addr::from(addr)))
        }
        AddressType::IPv6 => {
            let mut addr = [0u8; 16];
            stream.read_exact(&mut addr).await?;
            TargetAddr::Ip(IpAddr::V6(Ipv6Addr::from(addr)))
        }
        AddressType::DomainName => {
            let mut len = [0u8; 1];
            stream.read_exact(&mut len).await?;
            let mut domain = vec![0u8; len[0] as usize];
            stream.read_exact(&mut domain).await?;
            TargetAddr::Domain(String::from_utf8_lossy(&domain).to_string())
        }
    };

    // Read port (2 bytes, big endian)
    let mut port_bytes = [0u8; 2];
    stream.read_exact(&mut port_bytes).await?;
    let port = u16::from_be_bytes(port_bytes);

    Ok((command, target_addr, port))
}

/// Send SOCKS5 reply
async fn send_reply(
    stream: &mut TcpStream,
    reply: Reply,
    bound_addr: Option<SocketAddr>,
) -> Result<(), SocksError> {
    let mut response = Vec::with_capacity(22);

    // VER, REP, RSV
    response.push(SOCKS_VERSION);
    response.push(reply as u8);
    response.push(0x00);

    // BND.ADDR and BND.PORT
    match bound_addr {
        Some(SocketAddr::V4(addr)) => {
            response.push(AddressType::IPv4 as u8);
            response.extend_from_slice(&addr.ip().octets());
            response.extend_from_slice(&addr.port().to_be_bytes());
        }
        Some(SocketAddr::V6(addr)) => {
            response.push(AddressType::IPv6 as u8);
            response.extend_from_slice(&addr.ip().octets());
            response.extend_from_slice(&addr.port().to_be_bytes());
        }
        None => {
            // Use 0.0.0.0:0 as placeholder
            response.push(AddressType::IPv4 as u8);
            response.extend_from_slice(&[0, 0, 0, 0]);
            response.extend_from_slice(&[0, 0]);
        }
    }

    stream.write_all(&response).await?;
    Ok(())
}

/// Update stats from interception results
fn update_stats(stats: &Arc<SocksStats>, intercept_stats: &crate::protocol::InterceptStats) {
    stats
        .bytes_sent
        .fetch_add(intercept_stats.bytes_sent, Ordering::Relaxed);
    stats
        .bytes_received
        .fetch_add(intercept_stats.bytes_received, Ordering::Relaxed);

    // Track protocol-specific stats
    if let Some(protocol) = intercept_stats.protocol {
        match protocol {
            Protocol::PostgreSQL => {
                stats.postgres_connections.fetch_add(1, Ordering::Relaxed);
                stats
                    .database_queries
                    .fetch_add(intercept_stats.client_messages, Ordering::Relaxed);
            }
            Protocol::Kafka => {
                stats.kafka_connections.fetch_add(1, Ordering::Relaxed);
                stats
                    .kafka_messages
                    .fetch_add(intercept_stats.client_messages, Ordering::Relaxed);
            }
            Protocol::Unknown => {}
        }
    }
}

/// Resolve target address, checking port forwards first
/// Note: K8s services are now handled directly in handle_client using direct streams
async fn resolve_target_with_forwards(
    addr: &TargetAddr,
    port: u16,
    port_forwards: &Arc<RwLock<HashMap<String, SocketAddr>>>,
    _kubectl_manager: &Arc<RwLock<KubectlPortForwardManager>>,
) -> Result<SocketAddr, SocksError> {
    match addr {
        TargetAddr::Ip(ip) => Ok(SocketAddr::new(*ip, port)),
        TargetAddr::Domain(domain) => {
            let forwards = port_forwards.read().await;

            // Try exact match with port
            let domain_with_port = format!("{}:{}", domain, port);
            if let Some(local_addr) = forwards.get(&domain_with_port) {
                debug!("Port forward match: {} -> {}", domain_with_port, local_addr);
                return Ok(*local_addr);
            }

            // Try match without port
            if let Some(local_addr) = forwards.get(domain) {
                debug!("Port forward match: {} -> {}", domain, local_addr);
                return Ok(SocketAddr::new(local_addr.ip(), port));
            }

            // Check for wildcard matches (e.g., *.svc.cluster.local)
            for (pattern, local_addr) in forwards.iter() {
                if pattern.starts_with("*.") {
                    let suffix = &pattern[1..]; // Remove the '*'
                    if domain.ends_with(suffix) {
                        debug!(
                            "Wildcard port forward match: {} ({}) -> {}",
                            domain, pattern, local_addr
                        );
                        return Ok(SocketAddr::new(local_addr.ip(), port));
                    }
                }
            }

            drop(forwards);

            // K8s services are handled in handle_client directly, so if we get here
            // for a K8s service, something went wrong - but try DNS anyway
            debug!("No port forward for {}, attempting DNS resolution", domain);

            let addr_str = format!("{}:{}", domain, port);
            let resolved = tokio::net::lookup_host(addr_str.as_str())
                .await
                .ok()
                .and_then(|mut addrs| addrs.next());

            match resolved {
                Some(addr) => Ok(addr),
                None => {
                    warn!("DNS resolution failed for {}", domain);
                    Err(SocksError::DnsResolutionFailed(addr_str))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_try_from() {
        assert_eq!(Command::try_from(0x01).unwrap(), Command::Connect);
        assert_eq!(Command::try_from(0x02).unwrap(), Command::Bind);
        assert_eq!(Command::try_from(0x03).unwrap(), Command::UdpAssociate);
        assert!(Command::try_from(0x04).is_err());
    }

    #[test]
    fn test_address_type_try_from() {
        assert_eq!(AddressType::try_from(0x01).unwrap(), AddressType::IPv4);
        assert_eq!(
            AddressType::try_from(0x03).unwrap(),
            AddressType::DomainName
        );
        assert_eq!(AddressType::try_from(0x04).unwrap(), AddressType::IPv6);
        assert!(AddressType::try_from(0x02).is_err());
    }

    #[test]
    fn test_target_addr_display() {
        let ip = TargetAddr::Ip(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)));
        assert_eq!(ip.to_string(), "127.0.0.1");

        let domain = TargetAddr::Domain("example.com".to_string());
        assert_eq!(domain.to_string(), "example.com");
    }

    #[test]
    fn test_default_config() {
        let config = SocksConfig::default();
        assert_eq!(config.port, DEFAULT_SOCKS_PORT);
        assert_eq!(config.bind_addr, IpAddr::V4(Ipv4Addr::LOCALHOST));
        assert!(!config.require_auth);
    }

    #[tokio::test]
    async fn test_port_forward_add_remove() {
        let proxy = SocksProxy::with_defaults();

        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 5432);
        proxy
            .add_port_forward("postgres.database.svc.cluster.local".to_string(), addr)
            .await;

        let forwards = proxy.get_port_forwards().await;
        assert_eq!(
            forwards.get("postgres.database.svc.cluster.local"),
            Some(&addr)
        );

        proxy
            .remove_port_forward("postgres.database.svc.cluster.local")
            .await;
        let forwards = proxy.get_port_forwards().await;
        assert!(forwards.is_empty());
    }

    #[tokio::test]
    async fn test_resolve_target_with_port_forward() {
        let port_forwards = Arc::new(RwLock::new(HashMap::new()));
        let kubectl_manager = Arc::new(RwLock::new(KubectlPortForwardManager::new()));
        let local_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 5432);

        {
            let mut forwards = port_forwards.write().await;
            forwards.insert(
                "postgres.database.svc.cluster.local".to_string(),
                local_addr,
            );
        }

        let target = TargetAddr::Domain("postgres.database.svc.cluster.local".to_string());
        let resolved =
            resolve_target_with_forwards(&target, 5432, &port_forwards, &kubectl_manager)
                .await
                .unwrap();

        assert_eq!(resolved, local_addr);
    }

    #[tokio::test]
    async fn test_resolve_target_wildcard() {
        let port_forwards = Arc::new(RwLock::new(HashMap::new()));
        let kubectl_manager = Arc::new(RwLock::new(KubectlPortForwardManager::new()));
        let local_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 5432);

        {
            let mut forwards = port_forwards.write().await;
            forwards.insert("*.svc.cluster.local".to_string(), local_addr);
        }

        let target = TargetAddr::Domain("any-service.any-namespace.svc.cluster.local".to_string());
        let resolved =
            resolve_target_with_forwards(&target, 9999, &port_forwards, &kubectl_manager)
                .await
                .unwrap();

        // Should use wildcard match but preserve the requested port
        assert_eq!(resolved.ip(), local_addr.ip());
        assert_eq!(resolved.port(), 9999);
    }

    #[tokio::test]
    async fn test_resolve_ip_target() {
        let port_forwards = Arc::new(RwLock::new(HashMap::new()));
        let kubectl_manager = Arc::new(RwLock::new(KubectlPortForwardManager::new()));

        let target = TargetAddr::Ip(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)));
        let resolved =
            resolve_target_with_forwards(&target, 5432, &port_forwards, &kubectl_manager)
                .await
                .unwrap();

        assert_eq!(
            resolved,
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)), 5432)
        );
    }
}
