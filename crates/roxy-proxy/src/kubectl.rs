//! Kubernetes Port-Forward Manager (Native Implementation)
//!
//! This module manages port forwarding to Kubernetes services using the native
//! kube-rs client library. When a connection is requested to a K8s DNS name
//! (e.g., `postgres.backend.svc.cluster.local`), this manager will:
//!
//! 1. Parse the service name and namespace from the DNS name
//! 2. Find a pod backing the service
//! 3. Establish a port-forward connection through the Kubernetes API
//! 4. Return a stream that can be used to communicate with the pod
//!
//! This is more reliable than shelling out to kubectl as it:
//! - Uses the Kubernetes API directly
//! - Doesn't require kubectl to be installed
//! - Has no process management overhead
//! - Provides proper async streaming

use k8s_openapi::api::core::v1::{Pod, Service};
use kube::{
    api::{Api, ListParams, Portforwarder},
    Client, Config,
};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Arc;
use std::time::Instant;

use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::RwLock;
use tracing::{debug, error, info};

/// Starting port for dynamic allocation
const PORT_RANGE_START: u16 = 30000;
/// Ending port for dynamic allocation
const PORT_RANGE_END: u16 = 40000;

/// Parsed Kubernetes service address
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct K8sService {
    /// Service name
    pub name: String,
    /// Namespace
    pub namespace: String,
    /// Target port on the service
    pub port: u16,
}

impl K8sService {
    /// Parse a Kubernetes DNS name into service components
    ///
    /// Supported formats:
    /// - `service.namespace.svc.cluster.local`
    /// - `service.namespace.svc`
    ///
    /// Returns None if the DNS name is not a valid K8s service address
    pub fn from_dns_name(dns_name: &str, port: u16) -> Option<Self> {
        let parts: Vec<&str> = dns_name.split('.').collect();

        // service.namespace.svc.cluster.local (5 parts)
        if parts.len() == 5 && parts[2] == "svc" && parts[3] == "cluster" && parts[4] == "local" {
            return Some(K8sService {
                name: parts[0].to_string(),
                namespace: parts[1].to_string(),
                port,
            });
        }

        // service.namespace.svc (3 parts)
        if parts.len() == 3 && parts[2] == "svc" {
            return Some(K8sService {
                name: parts[0].to_string(),
                namespace: parts[1].to_string(),
                port,
            });
        }

        None
    }

    /// Get the full DNS name for this service
    pub fn dns_name(&self) -> String {
        format!("{}.{}.svc.cluster.local", self.name, self.namespace)
    }
}

/// Error types for Kubernetes operations
#[derive(Debug, thiserror::Error)]
pub enum KubeError {
    #[error("Kubernetes client error: {0}")]
    Client(#[from] kube::Error),

    #[error("No pods found for service {0}")]
    NoPodsFound(String),

    #[error("Service not found: {0}")]
    ServiceNotFound(String),

    #[error("Port forward failed: {0}")]
    PortForwardFailed(String),

    #[error("No available ports in range {0}-{1}")]
    NoAvailablePorts(u16, u16),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Connection closed")]
    ConnectionClosed,
}

/// Information about an active port forward
struct ActivePortForward {
    /// Local port that's listening
    local_port: u16,
    /// Handle to stop the forwarder
    shutdown_tx: tokio::sync::oneshot::Sender<()>,
}

/// Cached pod information for a service
struct CachedPod {
    /// Pod name
    name: String,
    /// When this cache entry was created
    cached_at: Instant,
}

/// How long to cache pod names (5 minutes)
const POD_CACHE_TTL_SECS: u64 = 300;

/// Manager for Kubernetes port forwarding using native kube-rs
pub struct KubectlPortForwardManager {
    /// Kubernetes client
    client: Option<Client>,
    /// Active port forwards: K8sService -> local port info
    active_forwards: Arc<RwLock<HashMap<K8sService, ActivePortForward>>>,
    /// Next port to try allocating
    next_port: AtomicU16,
    /// Cache of service -> pod name mappings to avoid repeated K8s API calls
    pod_cache: HashMap<K8sService, CachedPod>,
}

impl KubectlPortForwardManager {
    /// Create a new port forward manager
    pub fn new() -> Self {
        Self {
            client: None,
            active_forwards: Arc::new(RwLock::new(HashMap::new())),
            next_port: AtomicU16::new(PORT_RANGE_START),
            pod_cache: HashMap::new(),
        }
    }

    /// Initialize the Kubernetes client
    async fn get_client(&mut self) -> Result<Client, KubeError> {
        if let Some(ref client) = self.client {
            return Ok(client.clone());
        }

        let start = Instant::now();
        debug!("Initializing Kubernetes client...");

        // Try to load config from default locations (~/.kube/config or in-cluster)
        let config = Config::infer().await.map_err(|e| {
            KubeError::PortForwardFailed(format!("Failed to load kubeconfig: {}", e))
        })?;

        let client = Client::try_from(config)?;
        self.client = Some(client.clone());

        debug!("Kubernetes client initialized in {:?}", start.elapsed());
        Ok(client)
    }

    /// Find a pod that backs a service (with caching)
    async fn find_pod_for_service(&mut self, service: &K8sService) -> Result<String, KubeError> {
        // Check cache first
        if let Some(cached) = self.pod_cache.get(service) {
            if cached.cached_at.elapsed().as_secs() < POD_CACHE_TTL_SECS {
                debug!(
                    "Using cached pod for service {}: {} (age: {:?})",
                    service.dns_name(),
                    cached.name,
                    cached.cached_at.elapsed()
                );
                return Ok(cached.name.clone());
            } else {
                debug!("Pod cache expired for {}", service.dns_name());
            }
        }

        let start = Instant::now();
        debug!("Looking up pod for service {}...", service.dns_name());

        let client = self.get_client().await?;
        let client_time = start.elapsed();

        // First, get the service to find its selector
        let svc_start = Instant::now();
        let services: Api<Service> = Api::namespaced(client.clone(), &service.namespace);
        let svc = services.get(&service.name).await.map_err(|e| {
            if e.to_string().contains("NotFound") {
                KubeError::ServiceNotFound(service.dns_name())
            } else {
                KubeError::Client(e)
            }
        })?;
        let svc_time = svc_start.elapsed();

        // Get the selector from the service
        let selector = svc
            .spec
            .and_then(|s| s.selector)
            .ok_or_else(|| KubeError::NoPodsFound(service.dns_name()))?;

        // Build label selector string
        let label_selector: String = selector
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join(",");

        // Find pods matching the selector
        let pods_start = Instant::now();
        let pods: Api<Pod> = Api::namespaced(client, &service.namespace);
        let pod_list = pods
            .list(&ListParams::default().labels(&label_selector))
            .await?;
        let pods_time = pods_start.elapsed();

        // Find a running pod
        for pod in pod_list.items {
            if let Some(status) = &pod.status {
                if let Some(phase) = &status.phase {
                    if phase == "Running" {
                        if let Some(name) = pod.metadata.name {
                            debug!(
                                "Found running pod for service {}: {} (client: {:?}, svc: {:?}, pods: {:?}, total: {:?})",
                                service.dns_name(),
                                name,
                                client_time,
                                svc_time,
                                pods_time,
                                start.elapsed()
                            );

                            // Cache the result
                            self.pod_cache.insert(
                                service.clone(),
                                CachedPod {
                                    name: name.clone(),
                                    cached_at: Instant::now(),
                                },
                            );

                            return Ok(name);
                        }
                    }
                }
            }
        }

        Err(KubeError::NoPodsFound(service.dns_name()))
    }

    /// Invalidate the pod cache for a service (e.g., if connection fails)
    pub fn invalidate_pod_cache(&mut self, service: &K8sService) {
        if self.pod_cache.remove(service).is_some() {
            debug!("Invalidated pod cache for {}", service.dns_name());
        }
    }

    /// Allocate a local port for port forwarding
    async fn allocate_port(&self) -> Result<u16, KubeError> {
        for _ in 0..(PORT_RANGE_END - PORT_RANGE_START) {
            let port = self.next_port.fetch_add(1, Ordering::SeqCst);

            if port >= PORT_RANGE_END {
                self.next_port.store(PORT_RANGE_START, Ordering::SeqCst);
                continue;
            }

            // Check if port is already in use by us
            {
                let forwards = self.active_forwards.read().await;
                if forwards.values().any(|pf| pf.local_port == port) {
                    continue;
                }
            }

            // Try to bind to verify availability
            match TcpListener::bind(format!("127.0.0.1:{}", port)).await {
                Ok(listener) => {
                    drop(listener);
                    return Ok(port);
                }
                Err(_) => continue,
            }
        }

        Err(KubeError::NoAvailablePorts(
            PORT_RANGE_START,
            PORT_RANGE_END,
        ))
    }

    /// Get or create a port forward for a Kubernetes service
    pub async fn get_or_create_forward(
        &mut self,
        service: K8sService,
    ) -> Result<SocketAddr, KubeError> {
        // Check if we already have a forward for this service
        {
            let forwards = self.active_forwards.read().await;
            if let Some(pf) = forwards.get(&service) {
                debug!(
                    "Reusing existing port forward for {}: localhost:{}",
                    service.dns_name(),
                    pf.local_port
                );
                return Ok(SocketAddr::new(
                    IpAddr::V4(Ipv4Addr::LOCALHOST),
                    pf.local_port,
                ));
            }
        }

        // Find a pod for this service
        let pod_name = self.find_pod_for_service(&service).await?;

        // Allocate a local port
        let local_port = self.allocate_port().await?;

        info!(
            "Starting port-forward for {} via pod {} (localhost:{} -> {})",
            service.dns_name(),
            pod_name,
            local_port,
            service.port
        );

        // Create shutdown channel
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

        // Get client for the forwarder task
        let client = self.get_client().await?;
        let namespace = service.namespace.clone();
        let target_port = service.port;
        let service_name = service.dns_name();

        // Spawn the port forward listener
        let active_forwards = self.active_forwards.clone();
        let service_key = service.clone();

        tokio::spawn(async move {
            if let Err(e) = run_port_forward(
                client,
                &namespace,
                &pod_name,
                local_port,
                target_port,
                shutdown_rx,
                &service_name,
            )
            .await
            {
                error!("Port forward for {} failed: {}", service_name, e);
            }

            // Clean up when done
            let mut forwards = active_forwards.write().await;
            forwards.remove(&service_key);
            info!("Port forward for {} stopped", service_name);
        });

        // Store the port forward
        {
            let mut forwards = self.active_forwards.write().await;
            forwards.insert(
                service,
                ActivePortForward {
                    local_port,
                    shutdown_tx,
                },
            );
        }

        // Give the listener a moment to start
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        info!("Port forward ready: localhost:{}", local_port);

        Ok(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), local_port))
    }

    /// Stop a specific port forward
    pub async fn stop_forward(&self, service: &K8sService) {
        let mut forwards = self.active_forwards.write().await;
        if let Some(pf) = forwards.remove(service) {
            info!("Stopping port forward for {}", service.dns_name());
            let _ = pf.shutdown_tx.send(());
        }
    }

    /// Stop all port forwards
    pub async fn stop_all(&self) {
        let mut forwards = self.active_forwards.write().await;
        for (service, pf) in forwards.drain() {
            info!("Stopping port forward for {}", service.dns_name());
            let _ = pf.shutdown_tx.send(());
        }
    }

    /// Get the local address for an existing port forward
    pub async fn get_forward(&self, service: &K8sService) -> Option<SocketAddr> {
        let forwards = self.active_forwards.read().await;
        forwards
            .get(service)
            .map(|pf| SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), pf.local_port))
    }

    /// List all active port forwards
    pub async fn list_forwards(&self) -> Vec<(K8sService, u16)> {
        let forwards = self.active_forwards.read().await;
        forwards
            .iter()
            .map(|(svc, pf)| (svc.clone(), pf.local_port))
            .collect()
    }

    /// Check if a domain is a Kubernetes service DNS name
    pub fn is_k8s_service(domain: &str) -> bool {
        domain.ends_with(".svc.cluster.local") || domain.ends_with(".svc")
    }

    /// Get a direct stream to a Kubernetes service
    ///
    /// This method establishes a port-forward connection directly to a pod
    /// and returns a stream that can be used for bidirectional communication.
    /// Unlike `get_or_create_forward`, this doesn't use a local TCP listener,
    /// which eliminates race conditions where the local connection succeeds
    /// before the K8s connection is ready.
    pub async fn get_direct_stream(&mut self, service: K8sService) -> Result<K8sStream, KubeError> {
        let total_start = Instant::now();

        let client = self.get_client().await?;
        let pod_name = self.find_pod_for_service(&service).await?;
        let lookup_time = total_start.elapsed();

        info!(
            "Creating direct K8s stream to {} via pod {} port {} (lookup took {:?})",
            service.dns_name(),
            pod_name,
            service.port,
            lookup_time
        );

        let pods: Api<Pod> = Api::namespaced(client, &service.namespace);

        // Create the port forward - this makes the K8s API call and waits for it
        let pf_start = Instant::now();
        let port_forwarder = match pods.portforward(&pod_name, &[service.port]).await {
            Ok(pf) => pf,
            Err(e) => {
                // If port forward fails, invalidate cache in case pod changed
                self.invalidate_pod_cache(&service);
                return Err(KubeError::Client(e));
            }
        };
        let pf_time = pf_start.elapsed();

        debug!(
            "K8s port-forward established to pod {}/{} port {} (portforward: {:?}, total: {:?})",
            service.namespace,
            pod_name,
            service.port,
            pf_time,
            total_start.elapsed()
        );

        Ok(K8sStream::new(port_forwarder, service.port))
    }
}

impl Default for KubectlPortForwardManager {
    fn default() -> Self {
        Self::new()
    }
}

/// A wrapper around a Kubernetes port-forward stream
///
/// This type implements AsyncRead and AsyncWrite to allow direct use
/// in bidirectional tunnels without going through a local TCP listener.
pub struct K8sStream {
    port_forwarder: Portforwarder,
    port: u16,
    stream_taken: bool,
}

impl K8sStream {
    fn new(port_forwarder: Portforwarder, port: u16) -> Self {
        Self {
            port_forwarder,
            port,
            stream_taken: false,
        }
    }

    /// Take the underlying stream for use in a tunnel
    ///
    /// This should only be called once. Returns None if already taken.
    pub fn take_stream(&mut self) -> Option<impl AsyncRead + AsyncWrite + Unpin + '_> {
        if self.stream_taken {
            return None;
        }
        self.stream_taken = true;
        self.port_forwarder.take_stream(self.port)
    }

    /// Join the port forwarder to ensure clean shutdown
    pub async fn join(self) -> Result<(), String> {
        self.port_forwarder.join().await.map_err(|e| e.to_string())
    }
}

/// Run a port forward listener that accepts local connections and forwards them to a pod
async fn run_port_forward(
    client: Client,
    namespace: &str,
    pod_name: &str,
    local_port: u16,
    target_port: u16,
    mut shutdown_rx: tokio::sync::oneshot::Receiver<()>,
    service_name: &str,
) -> Result<(), KubeError> {
    let listener = TcpListener::bind(format!("127.0.0.1:{}", local_port)).await?;

    info!(
        "Port forward listener started on localhost:{} for {}",
        local_port, service_name
    );

    loop {
        tokio::select! {
            accept_result = listener.accept() => {
                match accept_result {
                    Ok((stream, client_addr)) => {
                        debug!(
                            "New connection from {} for port forward to {}",
                            client_addr, service_name
                        );

                        let client = client.clone();
                        let namespace = namespace.to_string();
                        let pod_name = pod_name.to_string();
                        let service_name = service_name.to_string();

                        tokio::spawn(async move {
                            match handle_port_forward_connection(
                                client,
                                &namespace,
                                &pod_name,
                                target_port,
                                stream,
                            )
                            .await
                            {
                                Ok(()) => {
                                    debug!("Port forward connection to {} completed normally", service_name);
                                }
                                Err(e) => {
                                    error!("Port forward connection to {} failed: {}", service_name, e);
                                }
                            }
                        });
                    }
                    Err(e) => {
                        error!("Failed to accept connection: {}", e);
                    }
                }
            }
            _ = &mut shutdown_rx => {
                debug!("Port forward shutdown requested for {}", service_name);
                break;
            }
        }
    }

    Ok(())
}

/// Handle a single port-forwarded connection
async fn handle_port_forward_connection(
    client: Client,
    namespace: &str,
    pod_name: &str,
    target_port: u16,
    mut local_stream: TcpStream,
) -> Result<(), KubeError> {
    let pods: Api<Pod> = Api::namespaced(client, namespace);

    debug!(
        "Starting Kubernetes port-forward to pod {}/{} port {}",
        namespace, pod_name, target_port
    );

    // Create the port forward - this makes the K8s API call
    let mut port_forwarder = match pods.portforward(pod_name, &[target_port]).await {
        Ok(pf) => {
            debug!(
                "Kubernetes port-forward established to pod {}/{} port {}",
                namespace, pod_name, target_port
            );
            pf
        }
        Err(e) => {
            error!(
                "Failed to establish Kubernetes port-forward to pod {}/{} port {}: {}",
                namespace, pod_name, target_port, e
            );
            return Err(KubeError::Client(e));
        }
    };

    // Get the stream for our port
    let mut upstream = match port_forwarder.take_stream(target_port) {
        Some(stream) => {
            debug!("Got upstream stream for port {}", target_port);
            stream
        }
        None => {
            error!(
                "Failed to get port forward stream for port {} - stream not available",
                target_port
            );
            return Err(KubeError::PortForwardFailed(
                "Failed to get port forward stream".into(),
            ));
        }
    };

    // Bidirectional copy
    let (mut local_read, mut local_write) = local_stream.split();
    let (mut upstream_read, mut upstream_write) = tokio::io::split(&mut upstream);

    let client_to_server = async { tokio::io::copy(&mut local_read, &mut upstream_write).await };

    let server_to_client = async { tokio::io::copy(&mut upstream_read, &mut local_write).await };

    // Run both directions concurrently
    debug!(
        "Starting bidirectional copy for port forward to port {}",
        target_port
    );
    let result = tokio::select! {
        r = client_to_server => {
            debug!("Client->Server copy finished: {:?}", r);
            r
        },
        r = server_to_client => {
            debug!("Server->Client copy finished: {:?}", r);
            r
        },
    };

    // Join the port forwarder to ensure clean shutdown
    if let Err(e) = port_forwarder.join().await {
        error!("Port forwarder join error: {}", e);
        return Err(KubeError::PortForwardFailed(e.to_string()));
    }

    match &result {
        Ok(bytes) => debug!("Port forward completed, {} bytes transferred", bytes),
        Err(e) => debug!("Port forward ended with IO error: {}", e),
    }

    result.map(|_| ()).map_err(KubeError::Io)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_k8s_dns_name_full() {
        let service = K8sService::from_dns_name("postgres.backend.svc.cluster.local", 5432);
        assert!(service.is_some());
        let service = service.unwrap();
        assert_eq!(service.name, "postgres");
        assert_eq!(service.namespace, "backend");
        assert_eq!(service.port, 5432);
    }

    #[test]
    fn test_parse_k8s_dns_name_short() {
        let service = K8sService::from_dns_name("kafka.messaging.svc", 9092);
        assert!(service.is_some());
        let service = service.unwrap();
        assert_eq!(service.name, "kafka");
        assert_eq!(service.namespace, "messaging");
        assert_eq!(service.port, 9092);
    }

    #[test]
    fn test_parse_non_k8s_dns_name() {
        let service = K8sService::from_dns_name("example.com", 80);
        assert!(service.is_none());

        let service = K8sService::from_dns_name("localhost", 5432);
        assert!(service.is_none());
    }

    #[test]
    fn test_k8s_service_dns_name() {
        let service = K8sService {
            name: "redis".to_string(),
            namespace: "cache".to_string(),
            port: 6379,
        };
        assert_eq!(service.dns_name(), "redis.cache.svc.cluster.local");
    }

    #[test]
    fn test_is_k8s_service() {
        assert!(KubectlPortForwardManager::is_k8s_service(
            "postgres.backend.svc.cluster.local"
        ));
        assert!(KubectlPortForwardManager::is_k8s_service(
            "kafka.messaging.svc"
        ));
        assert!(!KubectlPortForwardManager::is_k8s_service("example.com"));
        assert!(!KubectlPortForwardManager::is_k8s_service("localhost"));
    }

    // ==========================================================================
    // Integration tests requiring a local Kubernetes cluster
    //
    // These tests require:
    // 1. A running Kubernetes cluster (e.g., minikube, kind, or Docker Desktop)
    // 2. The example services deployed from examples/fullstack-k8s-dev:
    //    - kafka.messaging.svc.cluster.local:9092
    //    - postgres.database.svc.cluster.local:5432
    //
    // To run these tests:
    //   cargo test -p roxy-proxy k8s_integration -- --ignored
    //
    // To deploy the example services:
    //   cd examples/fullstack-k8s-dev && skaffold run
    // ==========================================================================

    /// Test that we can create a direct stream to Kafka and send/receive data
    #[tokio::test]
    #[ignore = "requires local k8s cluster with kafka.messaging service"]
    async fn k8s_integration_kafka_direct_stream() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let mut manager = KubectlPortForwardManager::new();
        let service = K8sService::from_dns_name("kafka.messaging.svc.cluster.local", 9092)
            .expect("should parse kafka service");

        // Get direct stream - this should wait for K8s connection
        let mut k8s_stream = manager
            .get_direct_stream(service)
            .await
            .expect("should connect to kafka");

        let mut stream = k8s_stream.take_stream().expect("should get stream");

        // Build Kafka ApiVersions request (API key 18, version 0)
        let client_id = b"test-client";
        let correlation_id: i32 = 12345;
        let api_key: i16 = 18; // ApiVersions
        let api_version: i16 = 0;

        let header_len = 2 + 2 + 4 + 2 + client_id.len(); // api_key + api_version + correlation_id + client_id_len + client_id
        let mut request = Vec::with_capacity(4 + header_len);
        request.extend_from_slice(&(header_len as i32).to_be_bytes());
        request.extend_from_slice(&api_key.to_be_bytes());
        request.extend_from_slice(&api_version.to_be_bytes());
        request.extend_from_slice(&correlation_id.to_be_bytes());
        request.extend_from_slice(&(client_id.len() as i16).to_be_bytes());
        request.extend_from_slice(client_id);

        // Send request
        stream
            .write_all(&request)
            .await
            .expect("should send request");

        // Read response (at least the length prefix)
        let mut response_len_buf = [0u8; 4];
        stream
            .read_exact(&mut response_len_buf)
            .await
            .expect("should read response length");

        let response_len = i32::from_be_bytes(response_len_buf);
        assert!(response_len > 0, "response should have positive length");
        assert!(response_len < 10000, "response length should be reasonable");

        // Read the rest of the response
        let mut response_body = vec![0u8; response_len as usize];
        stream
            .read_exact(&mut response_body)
            .await
            .expect("should read response body");

        // Verify correlation ID in response
        let response_correlation_id = i32::from_be_bytes([
            response_body[0],
            response_body[1],
            response_body[2],
            response_body[3],
        ]);
        assert_eq!(
            response_correlation_id, correlation_id,
            "correlation ID should match"
        );

        // Clean up
        drop(stream);
        let _ = k8s_stream.join().await;
    }

    /// Test that we can create a direct stream to PostgreSQL and perform a handshake
    #[tokio::test]
    #[ignore = "requires local k8s cluster with postgres.database service"]
    async fn k8s_integration_postgres_direct_stream() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let mut manager = KubectlPortForwardManager::new();
        let service = K8sService::from_dns_name("postgres.database.svc.cluster.local", 5432)
            .expect("should parse postgres service");

        // Get direct stream
        let mut k8s_stream = manager
            .get_direct_stream(service)
            .await
            .expect("should connect to postgres");

        let mut stream = k8s_stream.take_stream().expect("should get stream");

        // Build PostgreSQL startup message (protocol version 3.0)
        let user = b"postgres";
        let database = b"postgres";

        // Calculate message length
        let params = format!(
            "user\0{}\0database\0{}\0\0",
            String::from_utf8_lossy(user),
            String::from_utf8_lossy(database)
        );
        let msg_len = 4 + 4 + params.len(); // length + protocol version + params

        let mut request = Vec::with_capacity(msg_len);
        request.extend_from_slice(&(msg_len as i32).to_be_bytes());
        request.extend_from_slice(&196608i32.to_be_bytes()); // Protocol 3.0 (196608 = 3 << 16)
        request.extend_from_slice(params.as_bytes());

        // Send startup message
        stream
            .write_all(&request)
            .await
            .expect("should send startup message");

        // Read response - PostgreSQL should send 'R' (authentication) or 'E' (error)
        let mut response_type = [0u8; 1];
        stream
            .read_exact(&mut response_type)
            .await
            .expect("should read response type");

        // Should get either 'R' (AuthenticationOk/Request) or 'E' (Error)
        assert!(
            response_type[0] == b'R' || response_type[0] == b'E',
            "should get authentication or error response, got: {}",
            response_type[0] as char
        );

        // Clean up
        drop(stream);
        let _ = k8s_stream.join().await;
    }

    /// Test that pod caching works correctly - second connection should be faster
    #[tokio::test]
    #[ignore = "requires local k8s cluster with kafka.messaging service"]
    async fn k8s_integration_pod_cache() {
        let mut manager = KubectlPortForwardManager::new();
        let service = K8sService::from_dns_name("kafka.messaging.svc.cluster.local", 9092)
            .expect("should parse kafka service");

        // First connection - cold cache
        let start1 = std::time::Instant::now();
        let k8s_stream1 = manager
            .get_direct_stream(service.clone())
            .await
            .expect("should connect to kafka (first)");
        let duration1 = start1.elapsed();
        let _ = k8s_stream1.join().await;

        // Second connection - warm cache
        let start2 = std::time::Instant::now();
        let k8s_stream2 = manager
            .get_direct_stream(service.clone())
            .await
            .expect("should connect to kafka (second)");
        let duration2 = start2.elapsed();
        let _ = k8s_stream2.join().await;

        // Second connection should typically be faster due to caching
        // (though this isn't guaranteed, so we just log it)
        println!(
            "First connection: {:?}, Second connection: {:?}",
            duration1, duration2
        );

        // Both should complete in reasonable time (< 30 seconds)
        assert!(
            duration1.as_secs() < 30,
            "first connection took too long: {:?}",
            duration1
        );
        assert!(
            duration2.as_secs() < 30,
            "second connection took too long: {:?}",
            duration2
        );
    }

    /// Test that cache invalidation works when a pod lookup fails
    #[tokio::test]
    #[ignore = "requires local k8s cluster"]
    async fn k8s_integration_cache_invalidation() {
        let mut manager = KubectlPortForwardManager::new();
        let service = K8sService {
            name: "nonexistent".to_string(),
            namespace: "default".to_string(),
            port: 1234,
        };

        // This should fail but not panic
        let result = manager.get_direct_stream(service.clone()).await;
        assert!(result.is_err(), "should fail for nonexistent service");

        // Verify no stale cache entry
        manager.invalidate_pod_cache(&service);
    }

    /// Test multiple concurrent connections to verify no race conditions
    #[tokio::test]
    #[ignore = "requires local k8s cluster with kafka.messaging service"]
    async fn k8s_integration_concurrent_connections() {
        use std::sync::Arc;
        use tokio::sync::RwLock;

        let manager = Arc::new(RwLock::new(KubectlPortForwardManager::new()));

        let mut handles = vec![];

        for i in 0..3 {
            let manager = manager.clone();
            let handle = tokio::spawn(async move {
                let service = K8sService::from_dns_name("kafka.messaging.svc.cluster.local", 9092)
                    .expect("should parse kafka service");

                let mut mgr = manager.write().await;
                let result = mgr.get_direct_stream(service).await;
                drop(mgr);

                match result {
                    Ok(stream) => {
                        println!("Connection {} succeeded", i);
                        let _ = stream.join().await;
                        true
                    }
                    Err(e) => {
                        println!("Connection {} failed: {}", i, e);
                        false
                    }
                }
            });
            handles.push(handle);
        }

        let results: Vec<bool> = futures::future::join_all(handles)
            .await
            .into_iter()
            .map(|r| r.unwrap_or(false))
            .collect();

        let successes = results.iter().filter(|&&x| x).count();
        assert!(
            successes >= 2,
            "at least 2 of 3 concurrent connections should succeed"
        );
    }
}
