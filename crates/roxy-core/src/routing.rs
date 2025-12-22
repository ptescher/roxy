//! Routing rules and Kubernetes integration for Roxy
//!
//! This module provides data models for request routing and Kubernetes
//! port forwarding, all designed to be managed through the GUI.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

// ============================================================================
// Routing Rules
// ============================================================================

/// A routing rule that determines how requests are handled
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingRule {
    /// Unique identifier for this rule
    pub id: Uuid,

    /// Human-readable name for the rule
    pub name: String,

    /// Optional description
    pub description: Option<String>,

    /// Whether the rule is currently enabled
    pub enabled: bool,

    /// Priority (lower number = higher priority)
    pub priority: i32,

    /// Conditions that must match for this rule to apply
    pub match_conditions: MatchConditions,

    /// Action to take when the rule matches
    pub action: RuleAction,

    /// When this rule was created
    pub created_at: chrono::DateTime<chrono::Utc>,

    /// When this rule was last modified
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl RoutingRule {
    /// Create a new routing rule with default values
    pub fn new(name: impl Into<String>) -> Self {
        let now = chrono::Utc::now();
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            description: None,
            enabled: true,
            priority: 100,
            match_conditions: MatchConditions::default(),
            action: RuleAction::Passthrough,
            created_at: now,
            updated_at: now,
        }
    }

    /// Create a rule to forward matching requests to a local server
    pub fn forward_to_local(
        name: impl Into<String>,
        host_pattern: impl Into<String>,
        path_pattern: impl Into<String>,
        local_target: LocalTarget,
    ) -> Self {
        let mut rule = Self::new(name);
        rule.match_conditions.host = Some(MatchPattern::Glob(host_pattern.into()));
        rule.match_conditions.path = Some(MatchPattern::Glob(path_pattern.into()));
        rule.action = RuleAction::ForwardToLocal(local_target);
        rule
    }

    /// Check if this rule matches a request
    pub fn matches(&self, request: &RequestInfo) -> bool {
        if !self.enabled {
            return false;
        }
        self.match_conditions.matches(request)
    }
}

/// Conditions that determine whether a rule matches a request
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MatchConditions {
    /// Match against the request host (e.g., "api.example.com")
    pub host: Option<MatchPattern>,

    /// Match against the request path (e.g., "/v1/users/*")
    pub path: Option<MatchPattern>,

    /// Match against the HTTP method (GET, POST, etc.)
    pub method: Option<Vec<HttpMethod>>,

    /// Match against specific headers
    pub headers: Option<HashMap<String, MatchPattern>>,

    /// Match against query parameters
    pub query_params: Option<HashMap<String, MatchPattern>>,

    /// Invert the match (NOT logic)
    pub negate: bool,
}

impl MatchConditions {
    /// Check if these conditions match a request
    pub fn matches(&self, request: &RequestInfo) -> bool {
        let result = self.matches_inner(request);
        if self.negate {
            !result
        } else {
            result
        }
    }

    fn matches_inner(&self, request: &RequestInfo) -> bool {
        // Host matching
        if let Some(ref pattern) = self.host {
            if !pattern.matches(&request.host) {
                return false;
            }
        }

        // Path matching
        if let Some(ref pattern) = self.path {
            if !pattern.matches(&request.path) {
                return false;
            }
        }

        // Method matching
        if let Some(ref methods) = self.method {
            if !methods.contains(&request.method) {
                return false;
            }
        }

        // Header matching
        if let Some(ref header_patterns) = self.headers {
            for (name, pattern) in header_patterns {
                match request.headers.get(name) {
                    Some(value) if pattern.matches(value) => {}
                    _ => return false,
                }
            }
        }

        // Query param matching
        if let Some(ref param_patterns) = self.query_params {
            for (name, pattern) in param_patterns {
                match request.query_params.get(name) {
                    Some(value) if pattern.matches(value) => {}
                    _ => return false,
                }
            }
        }

        true
    }
}

/// Pattern for matching strings
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum MatchPattern {
    /// Exact string match
    Exact(String),

    /// Glob pattern (supports * and ?)
    Glob(String),

    /// Regular expression
    Regex(String),

    /// Prefix match
    Prefix(String),

    /// Suffix match
    Suffix(String),

    /// Contains substring
    Contains(String),
}

impl MatchPattern {
    /// Check if this pattern matches the given string
    pub fn matches(&self, value: &str) -> bool {
        match self {
            MatchPattern::Exact(pattern) => value == pattern,
            MatchPattern::Glob(pattern) => glob_match(pattern, value),
            MatchPattern::Regex(pattern) => regex::Regex::new(pattern)
                .map(|re| re.is_match(value))
                .unwrap_or(false),
            MatchPattern::Prefix(prefix) => value.starts_with(prefix),
            MatchPattern::Suffix(suffix) => value.ends_with(suffix),
            MatchPattern::Contains(substr) => value.contains(substr),
        }
    }
}

/// Simple glob matching (supports * and ?)
fn glob_match(pattern: &str, value: &str) -> bool {
    let mut pattern_chars = pattern.chars().peekable();
    let mut value_chars = value.chars().peekable();

    while let Some(p) = pattern_chars.next() {
        match p {
            '*' => {
                // * matches zero or more characters
                if pattern_chars.peek().is_none() {
                    return true; // Trailing * matches everything
                }
                // Try matching the rest of the pattern at each position
                let remaining_pattern: String = pattern_chars.collect();
                let mut remaining_value: String = value_chars.collect();
                loop {
                    if glob_match(&remaining_pattern, &remaining_value) {
                        return true;
                    }
                    if remaining_value.is_empty() {
                        return false;
                    }
                    remaining_value = remaining_value[1..].to_string();
                }
            }
            '?' => {
                // ? matches exactly one character
                if value_chars.next().is_none() {
                    return false;
                }
            }
            c => {
                if value_chars.next() != Some(c) {
                    return false;
                }
            }
        }
    }

    value_chars.next().is_none()
}

/// HTTP methods
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Delete,
    Patch,
    Head,
    Options,
    Connect,
    Trace,
}

impl std::fmt::Display for HttpMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HttpMethod::Get => write!(f, "GET"),
            HttpMethod::Post => write!(f, "POST"),
            HttpMethod::Put => write!(f, "PUT"),
            HttpMethod::Delete => write!(f, "DELETE"),
            HttpMethod::Patch => write!(f, "PATCH"),
            HttpMethod::Head => write!(f, "HEAD"),
            HttpMethod::Options => write!(f, "OPTIONS"),
            HttpMethod::Connect => write!(f, "CONNECT"),
            HttpMethod::Trace => write!(f, "TRACE"),
        }
    }
}

/// Action to take when a rule matches
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum RuleAction {
    /// Let the request pass through normally
    Passthrough,

    /// Forward to a local server
    ForwardToLocal(LocalTarget),

    /// Forward to a different remote server
    ForwardToRemote(RemoteTarget),

    /// Block the request with an error response
    Block(BlockResponse),

    /// Modify the request before forwarding
    ModifyRequest(RequestModification),

    /// Delay the request (for testing)
    Delay { milliseconds: u64 },
}

/// Target for forwarding to a local server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalTarget {
    /// Local host (usually 127.0.0.1 or localhost)
    pub host: String,

    /// Local port
    pub port: u16,

    /// Whether to use HTTPS for the local connection
    pub use_tls: bool,

    /// Optional path prefix to strip from the request
    pub strip_path_prefix: Option<String>,

    /// Optional path prefix to add to the request
    pub add_path_prefix: Option<String>,

    /// Whether to preserve the original Host header
    pub preserve_host: bool,
}

impl LocalTarget {
    /// Create a new local target
    pub fn new(port: u16) -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port,
            use_tls: false,
            strip_path_prefix: None,
            add_path_prefix: None,
            preserve_host: false,
        }
    }

    /// Get the base URL for this target
    pub fn base_url(&self) -> String {
        let scheme = if self.use_tls { "https" } else { "http" };
        format!("{}://{}:{}", scheme, self.host, self.port)
    }
}

/// Target for forwarding to a different remote server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteTarget {
    /// Remote host
    pub host: String,

    /// Remote port (defaults to 80/443 based on scheme)
    pub port: Option<u16>,

    /// Whether to use HTTPS
    pub use_tls: bool,

    /// Optional path modifications
    pub strip_path_prefix: Option<String>,
    pub add_path_prefix: Option<String>,

    /// Whether to preserve the original Host header
    pub preserve_host: bool,
}

/// Response to return when blocking a request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockResponse {
    /// HTTP status code
    pub status_code: u16,

    /// Response body
    pub body: Option<String>,

    /// Content-Type header
    pub content_type: Option<String>,
}

impl Default for BlockResponse {
    fn default() -> Self {
        Self {
            status_code: 403,
            body: Some("Blocked by Roxy".to_string()),
            content_type: Some("text/plain".to_string()),
        }
    }
}

/// Modifications to apply to a request
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RequestModification {
    /// Headers to add or replace
    pub set_headers: HashMap<String, String>,

    /// Headers to remove
    pub remove_headers: Vec<String>,

    /// Query parameters to add or replace
    pub set_query_params: HashMap<String, String>,

    /// Query parameters to remove
    pub remove_query_params: Vec<String>,
}

/// Information about a request (used for matching)
#[derive(Debug, Clone)]
pub struct RequestInfo {
    pub host: String,
    pub path: String,
    pub method: HttpMethod,
    pub headers: HashMap<String, String>,
    pub query_params: HashMap<String, String>,
}

// ============================================================================
// Routing Rule Manager
// ============================================================================

/// Manages routing rules and matches requests
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RoutingRuleSet {
    /// All routing rules, ordered by priority
    pub rules: Vec<RoutingRule>,
}

impl RoutingRuleSet {
    /// Create a new empty rule set
    pub fn new() -> Self {
        Self { rules: Vec::new() }
    }

    /// Add a rule to the set
    pub fn add_rule(&mut self, rule: RoutingRule) {
        self.rules.push(rule);
        self.sort_by_priority();
    }

    /// Remove a rule by ID
    pub fn remove_rule(&mut self, id: Uuid) -> Option<RoutingRule> {
        if let Some(pos) = self.rules.iter().position(|r| r.id == id) {
            Some(self.rules.remove(pos))
        } else {
            None
        }
    }

    /// Get a rule by ID
    pub fn get_rule(&self, id: Uuid) -> Option<&RoutingRule> {
        self.rules.iter().find(|r| r.id == id)
    }

    /// Get a mutable reference to a rule by ID
    pub fn get_rule_mut(&mut self, id: Uuid) -> Option<&mut RoutingRule> {
        self.rules.iter_mut().find(|r| r.id == id)
    }

    /// Update a rule and re-sort by priority
    pub fn update_rule(&mut self, rule: RoutingRule) -> bool {
        if let Some(existing) = self.rules.iter_mut().find(|r| r.id == rule.id) {
            *existing = rule;
            self.sort_by_priority();
            true
        } else {
            false
        }
    }

    /// Find the first matching rule for a request
    pub fn find_match(&self, request: &RequestInfo) -> Option<&RoutingRule> {
        self.rules.iter().find(|rule| rule.matches(request))
    }

    /// Sort rules by priority (lower number = higher priority)
    fn sort_by_priority(&mut self) {
        self.rules.sort_by_key(|r| r.priority);
    }

    /// Get all enabled rules
    pub fn enabled_rules(&self) -> impl Iterator<Item = &RoutingRule> {
        self.rules.iter().filter(|r| r.enabled)
    }
}

// ============================================================================
// Kubernetes Integration
// ============================================================================

/// Configuration for connecting to a Kubernetes cluster
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KubernetesCluster {
    /// Unique identifier
    pub id: Uuid,

    /// Display name for this cluster
    pub name: String,

    /// Path to kubeconfig file (None = use default)
    pub kubeconfig_path: Option<String>,

    /// Context name within kubeconfig (None = use current context)
    pub context: Option<String>,

    /// Whether this cluster connection is active
    pub connected: bool,
}

impl KubernetesCluster {
    /// Create a new cluster configuration using defaults
    pub fn default_cluster() -> Self {
        Self {
            id: Uuid::new_v4(),
            name: "Default Cluster".to_string(),
            kubeconfig_path: None,
            context: None,
            connected: false,
        }
    }

    /// Create a new cluster with a specific kubeconfig
    pub fn with_kubeconfig(name: impl Into<String>, path: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            kubeconfig_path: Some(path.into()),
            context: None,
            connected: false,
        }
    }
}

/// A Kubernetes service that can be port-forwarded
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KubernetesService {
    /// Service name
    pub name: String,

    /// Namespace
    pub namespace: String,

    /// Service ports
    pub ports: Vec<ServicePort>,

    /// Service type (ClusterIP, NodePort, LoadBalancer)
    pub service_type: String,

    /// Cluster IP
    pub cluster_ip: Option<String>,
}

impl KubernetesService {
    /// Get the internal DNS name for this service
    pub fn dns_name(&self) -> String {
        format!("{}.{}.svc.cluster.local", self.name, self.namespace)
    }
}

/// A port exposed by a Kubernetes service
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServicePort {
    /// Port name (optional)
    pub name: Option<String>,

    /// Port number
    pub port: u16,

    /// Target port (may be different from port)
    pub target_port: u16,

    /// Protocol (TCP/UDP)
    pub protocol: String,
}

/// Configuration for a port forward
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortForward {
    /// Unique identifier
    pub id: Uuid,

    /// Display name
    pub name: String,

    /// Cluster this port forward belongs to
    pub cluster_id: Uuid,

    /// Target type and reference
    pub target: PortForwardTarget,

    /// Local port to listen on
    pub local_port: u16,

    /// Remote port to forward to
    pub remote_port: u16,

    /// Local address to bind (usually 127.0.0.1)
    pub local_address: String,

    /// Whether this port forward is currently active
    pub active: bool,

    /// Whether to automatically start this forward on launch
    pub auto_start: bool,

    /// Optional routing rule to create for this forward
    /// (e.g., route postgres-primary.backend.svc.cluster.local:5432 to localhost:local_port)
    pub create_routing_rule: bool,
}

impl PortForward {
    /// Create a new port forward for a service
    pub fn for_service(
        name: impl Into<String>,
        cluster_id: Uuid,
        namespace: impl Into<String>,
        service_name: impl Into<String>,
        local_port: u16,
        remote_port: u16,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            cluster_id,
            target: PortForwardTarget::Service {
                namespace: namespace.into(),
                name: service_name.into(),
            },
            local_port,
            remote_port,
            local_address: "127.0.0.1".to_string(),
            active: false,
            auto_start: false,
            create_routing_rule: true,
        }
    }

    /// Get the local endpoint string
    pub fn local_endpoint(&self) -> String {
        format!("{}:{}", self.local_address, self.local_port)
    }

    /// Generate the kubectl port-forward command
    pub fn kubectl_command(&self) -> String {
        let target = match &self.target {
            PortForwardTarget::Service { namespace, name } => {
                format!("-n {} svc/{}", namespace, name)
            }
            PortForwardTarget::Pod { namespace, name } => {
                format!("-n {} pod/{}", namespace, name)
            }
            PortForwardTarget::Deployment { namespace, name } => {
                format!("-n {} deployment/{}", namespace, name)
            }
        };

        format!(
            "kubectl port-forward {} {}:{} --address={}",
            target, self.local_port, self.remote_port, self.local_address
        )
    }
}

/// Target for a port forward
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum PortForwardTarget {
    /// Forward to a Service
    Service { namespace: String, name: String },

    /// Forward to a specific Pod
    Pod { namespace: String, name: String },

    /// Forward to a Deployment (picks a pod)
    Deployment { namespace: String, name: String },
}

impl PortForwardTarget {
    /// Get the Kubernetes DNS name for this target
    pub fn dns_name(&self) -> String {
        match self {
            PortForwardTarget::Service { namespace, name } => {
                format!("{}.{}.svc.cluster.local", name, namespace)
            }
            PortForwardTarget::Pod { namespace, name } => {
                format!("{}.{}.pod.cluster.local", name, namespace)
            }
            PortForwardTarget::Deployment { namespace, name } => {
                // Deployments don't have direct DNS, use service name
                format!("{}.{}.svc.cluster.local", name, namespace)
            }
        }
    }
}

/// Manager for Kubernetes port forwards
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PortForwardManager {
    /// Configured clusters
    pub clusters: Vec<KubernetesCluster>,

    /// Configured port forwards
    pub port_forwards: Vec<PortForward>,
}

impl PortForwardManager {
    /// Create a new empty manager
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a cluster
    pub fn add_cluster(&mut self, cluster: KubernetesCluster) {
        self.clusters.push(cluster);
    }

    /// Add a port forward
    pub fn add_port_forward(&mut self, port_forward: PortForward) {
        self.port_forwards.push(port_forward);
    }

    /// Get all port forwards for a cluster
    pub fn port_forwards_for_cluster(&self, cluster_id: Uuid) -> Vec<&PortForward> {
        self.port_forwards
            .iter()
            .filter(|pf| pf.cluster_id == cluster_id)
            .collect()
    }

    /// Get all active port forwards
    pub fn active_port_forwards(&self) -> Vec<&PortForward> {
        self.port_forwards.iter().filter(|pf| pf.active).collect()
    }

    /// Generate routing rules for all active port forwards that have create_routing_rule set
    pub fn generate_routing_rules(&self) -> Vec<RoutingRule> {
        self.port_forwards
            .iter()
            .filter(|pf| pf.active && pf.create_routing_rule)
            .map(|pf| {
                let dns_name = pf.target.dns_name();
                let mut rule = RoutingRule::new(format!("K8s: {}", pf.name));
                rule.description = Some(format!(
                    "Auto-generated rule for port forward to {}",
                    dns_name
                ));
                rule.match_conditions.host = Some(MatchPattern::Exact(dns_name));
                rule.action = RuleAction::ForwardToLocal(LocalTarget {
                    host: pf.local_address.clone(),
                    port: pf.local_port,
                    use_tls: false,
                    strip_path_prefix: None,
                    add_path_prefix: None,
                    preserve_host: false,
                });
                rule
            })
            .collect()
    }
}

// ============================================================================
// Project/Workspace Configuration
// ============================================================================

/// A development project/workspace configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    /// Unique identifier
    pub id: Uuid,

    /// Project name
    pub name: String,

    /// Optional description
    pub description: Option<String>,

    /// Routing rules for this project
    pub routing_rules: RoutingRuleSet,

    /// Kubernetes configuration for this project
    pub kubernetes: PortForwardManager,

    /// When this project was created
    pub created_at: chrono::DateTime<chrono::Utc>,

    /// When this project was last modified
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl Project {
    /// Create a new empty project
    pub fn new(name: impl Into<String>) -> Self {
        let now = chrono::Utc::now();
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            description: None,
            routing_rules: RoutingRuleSet::new(),
            kubernetes: PortForwardManager::new(),
            created_at: now,
            updated_at: now,
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_glob_matching() {
        assert!(glob_match("*", "anything"));
        assert!(glob_match("*.example.com", "api.example.com"));
        assert!(glob_match("/v1/*", "/v1/users"));
        assert!(glob_match("/v1/*/items", "/v1/users/items"));
        assert!(!glob_match("/v1/*", "/v2/users"));
        assert!(glob_match("api.?.com", "api.x.com"));
        assert!(!glob_match("api.?.com", "api.xx.com"));
    }

    #[test]
    fn test_match_pattern() {
        let exact = MatchPattern::Exact("api.example.com".to_string());
        assert!(exact.matches("api.example.com"));
        assert!(!exact.matches("other.example.com"));

        let prefix = MatchPattern::Prefix("/v1/".to_string());
        assert!(prefix.matches("/v1/users"));
        assert!(!prefix.matches("/v2/users"));

        let contains = MatchPattern::Contains("example".to_string());
        assert!(contains.matches("api.example.com"));
        assert!(!contains.matches("api.other.com"));
    }

    #[test]
    fn test_routing_rule_matching() {
        let rule = RoutingRule::forward_to_local(
            "Forward to local",
            "api.example.com",
            "/v1/new-endpoint*",
            LocalTarget::new(3000),
        );

        let matching_request = RequestInfo {
            host: "api.example.com".to_string(),
            path: "/v1/new-endpoint/test".to_string(),
            method: HttpMethod::Get,
            headers: HashMap::new(),
            query_params: HashMap::new(),
        };

        let non_matching_request = RequestInfo {
            host: "api.example.com".to_string(),
            path: "/v1/old-endpoint".to_string(),
            method: HttpMethod::Get,
            headers: HashMap::new(),
            query_params: HashMap::new(),
        };

        assert!(rule.matches(&matching_request));
        assert!(!rule.matches(&non_matching_request));
    }

    #[test]
    fn test_port_forward_dns_name() {
        let pf = PortForward::for_service(
            "Postgres",
            Uuid::new_v4(),
            "backend",
            "postgres-primary",
            5432,
            5432,
        );

        assert_eq!(
            pf.target.dns_name(),
            "postgres-primary.backend.svc.cluster.local"
        );
    }

    #[test]
    fn test_local_target_url() {
        let target = LocalTarget::new(3000);
        assert_eq!(target.base_url(), "http://127.0.0.1:3000");

        let tls_target = LocalTarget {
            use_tls: true,
            ..LocalTarget::new(3001)
        };
        assert_eq!(tls_target.base_url(), "https://127.0.0.1:3001");
    }
}
