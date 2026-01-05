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
    api::{Api, ListParams},
    Client, Config,
};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Arc;

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

/// Manager for Kubernetes port forwarding using native kube-rs
pub struct KubectlPortForwardManager {
    /// Kubernetes client
    client: Option<Client>,
    /// Active port forwards: K8sService -> local port info
    active_forwards: Arc<RwLock<HashMap<K8sService, ActivePortForward>>>,
    /// Next port to try allocating
    next_port: AtomicU16,
}

impl KubectlPortForwardManager {
    /// Create a new port forward manager
    pub fn new() -> Self {
        Self {
            client: None,
            active_forwards: Arc::new(RwLock::new(HashMap::new())),
            next_port: AtomicU16::new(PORT_RANGE_START),
        }
    }

    /// Initialize the Kubernetes client
    async fn get_client(&mut self) -> Result<Client, KubeError> {
        if let Some(ref client) = self.client {
            return Ok(client.clone());
        }

        // Try to load config from default locations (~/.kube/config or in-cluster)
        let config = Config::infer().await.map_err(|e| {
            KubeError::PortForwardFailed(format!("Failed to load kubeconfig: {}", e))
        })?;

        let client = Client::try_from(config)?;
        self.client = Some(client.clone());
        Ok(client)
    }

    /// Find a pod that backs a service
    async fn find_pod_for_service(&mut self, service: &K8sService) -> Result<String, KubeError> {
        let client = self.get_client().await?;

        // First, get the service to find its selector
        let services: Api<Service> = Api::namespaced(client.clone(), &service.namespace);
        let svc = services.get(&service.name).await.map_err(|e| {
            if e.to_string().contains("NotFound") {
                KubeError::ServiceNotFound(service.dns_name())
            } else {
                KubeError::Client(e)
            }
        })?;

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
        let pods: Api<Pod> = Api::namespaced(client, &service.namespace);
        let pod_list = pods
            .list(&ListParams::default().labels(&label_selector))
            .await?;

        // Find a running pod
        for pod in pod_list.items {
            if let Some(status) = &pod.status {
                if let Some(phase) = &status.phase {
                    if phase == "Running" {
                        if let Some(name) = pod.metadata.name {
                            debug!(
                                "Found running pod for service {}: {}",
                                service.dns_name(),
                                name
                            );
                            return Ok(name);
                        }
                    }
                }
            }
        }

        Err(KubeError::NoPodsFound(service.dns_name()))
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
}

impl Default for KubectlPortForwardManager {
    fn default() -> Self {
        Self::new()
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
                            if let Err(e) = handle_port_forward_connection(
                                client,
                                &namespace,
                                &pod_name,
                                target_port,
                                stream,
                            )
                            .await
                            {
                                debug!("Port forward connection to {} ended: {}", service_name, e);
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

    // Create the port forward
    let mut port_forwarder = pods.portforward(pod_name, &[target_port]).await?;

    // Get the stream for our port
    let mut upstream = port_forwarder
        .take_stream(target_port)
        .ok_or_else(|| KubeError::PortForwardFailed("Failed to get port forward stream".into()))?;

    // Bidirectional copy
    let (mut local_read, mut local_write) = local_stream.split();
    let (mut upstream_read, mut upstream_write) = tokio::io::split(&mut upstream);

    let client_to_server = async { tokio::io::copy(&mut local_read, &mut upstream_write).await };

    let server_to_client = async { tokio::io::copy(&mut upstream_read, &mut local_write).await };

    // Run both directions concurrently
    let result = tokio::select! {
        r = client_to_server => r,
        r = server_to_client => r,
    };

    // Join the port forwarder to ensure clean shutdown
    port_forwarder
        .join()
        .await
        .map_err(|e| KubeError::PortForwardFailed(e.to_string()))?;

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
}
