//! Gateway Routing Module
//!
//! Routes HTTP traffic based on Kubernetes Gateway API HTTPRoute resources.
//! When a request comes in, this module matches it against HTTPRoute rules
//! and determines the backend service to forward to.

use kube::{
    api::{Api, DynamicObject, GroupVersionKind, ListParams},
    config::Config,
    discovery::ApiResource,
    Client,
};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, info};

/// How often to refresh HTTPRoute cache (in seconds)
const ROUTE_CACHE_TTL_SECS: u64 = 30;

/// Error types for gateway operations
#[derive(Debug, thiserror::Error)]
pub enum GatewayError {
    #[error("Kubernetes client error: {0}")]
    Client(#[from] kube::Error),

    #[error("No matching route found for {0}")]
    NoRouteFound(String),

    #[error("No backend found for route {0}")]
    NoBackendFound(String),

    #[error("Failed to parse HTTPRoute: {0}")]
    ParseError(String),
}

/// A backend service reference from an HTTPRoute
#[derive(Debug, Clone)]
pub struct BackendRef {
    /// Service name
    pub name: String,
    /// Namespace (defaults to the route's namespace)
    pub namespace: String,
    /// Port number
    pub port: u16,
    /// Optional weight for traffic splitting
    pub weight: u32,
}

impl BackendRef {
    /// Get the full DNS name for this backend
    pub fn dns_name(&self) -> String {
        format!("{}.{}.svc.cluster.local", self.name, self.namespace)
    }
}

/// Path match type from HTTPRoute
#[derive(Debug, Clone)]
pub enum PathMatch {
    /// Exact path match
    Exact(String),
    /// Prefix path match
    PathPrefix(String),
    /// Regex path match
    RegularExpression(String),
}

impl PathMatch {
    /// Check if a path matches this rule
    pub fn matches(&self, path: &str) -> bool {
        match self {
            PathMatch::Exact(expected) => path == expected,
            PathMatch::PathPrefix(prefix) => {
                // Exact match
                if path == prefix {
                    return true;
                }
                // Prefix match must be at a path segment boundary
                // /api should match /api/ and /api/foo but not /apiv2
                if path.starts_with(prefix) {
                    let remainder = &path[prefix.len()..];
                    // Must start with / or prefix must end with /
                    return remainder.starts_with('/') || prefix.ends_with('/');
                }
                // Handle case where prefix has trailing slash but path doesn't
                if prefix.ends_with('/') && path == &prefix[..prefix.len() - 1] {
                    return true;
                }
                false
            }
            PathMatch::RegularExpression(pattern) => regex::Regex::new(pattern)
                .map(|re| re.is_match(path))
                .unwrap_or(false),
        }
    }
}

/// Header match from HTTPRoute
#[derive(Debug, Clone)]
pub struct HeaderMatch {
    /// Header name
    pub name: String,
    /// Match type
    pub match_type: HeaderMatchType,
    /// Value to match
    pub value: String,
}

/// Header match type
#[derive(Debug, Clone)]
pub enum HeaderMatchType {
    Exact,
    RegularExpression,
}

impl HeaderMatch {
    /// Check if headers match this rule
    pub fn matches(&self, headers: &http::HeaderMap) -> bool {
        let header_value = match headers.get(&self.name) {
            Some(v) => v.to_str().unwrap_or(""),
            None => return false,
        };

        match self.match_type {
            HeaderMatchType::Exact => header_value == self.value,
            HeaderMatchType::RegularExpression => regex::Regex::new(&self.value)
                .map(|re| re.is_match(header_value))
                .unwrap_or(false),
        }
    }
}

/// A single match rule from HTTPRoute
#[derive(Debug, Clone)]
pub struct RouteMatch {
    /// Path match (optional)
    pub path: Option<PathMatch>,
    /// Header matches (all must match)
    pub headers: Vec<HeaderMatch>,
    /// HTTP method match (optional)
    pub method: Option<String>,
}

impl RouteMatch {
    /// Check if a request matches this rule
    pub fn matches(&self, path: &str, method: &str, headers: &http::HeaderMap) -> bool {
        // Check path
        if let Some(ref path_match) = self.path {
            if !path_match.matches(path) {
                return false;
            }
        }

        // Check method
        if let Some(ref expected_method) = self.method {
            if !method.eq_ignore_ascii_case(expected_method) {
                return false;
            }
        }

        // Check all headers
        for header_match in &self.headers {
            if !header_match.matches(headers) {
                return false;
            }
        }

        true
    }
}

/// A routing rule with matches and backends
#[derive(Debug, Clone)]
pub struct RouteRule {
    /// Match conditions (any match triggers the rule)
    pub matches: Vec<RouteMatch>,
    /// Backend services to route to
    pub backends: Vec<BackendRef>,
}

impl RouteRule {
    /// Check if a request matches this rule and return matching backends
    pub fn matches(
        &self,
        path: &str,
        method: &str,
        headers: &http::HeaderMap,
    ) -> Option<&[BackendRef]> {
        // If no matches specified, it matches everything
        if self.matches.is_empty() {
            return Some(&self.backends);
        }

        // Any match means the rule applies
        for m in &self.matches {
            if m.matches(path, method, headers) {
                return Some(&self.backends);
            }
        }

        None
    }

    /// Select a backend (weighted random selection if multiple)
    pub fn select_backend(&self) -> Option<&BackendRef> {
        if self.backends.is_empty() {
            return None;
        }

        if self.backends.len() == 1 {
            return self.backends.first();
        }

        // Weighted random selection
        let total_weight: u32 = self.backends.iter().map(|b| b.weight).sum();
        if total_weight == 0 {
            return self.backends.first();
        }

        let mut random = rand::random::<u32>() % total_weight;
        for backend in &self.backends {
            if random < backend.weight {
                return Some(backend);
            }
            random -= backend.weight;
        }

        self.backends.first()
    }
}

/// A parsed HTTPRoute resource
#[derive(Debug, Clone)]
pub struct HttpRoute {
    /// Route name
    pub name: String,
    /// Namespace
    pub namespace: String,
    /// Hostnames this route matches
    pub hostnames: Vec<String>,
    /// Routing rules
    pub rules: Vec<RouteRule>,
}

impl HttpRoute {
    /// Check if this route matches a hostname
    pub fn matches_hostname(&self, hostname: &str) -> bool {
        // If no hostnames specified, matches all
        if self.hostnames.is_empty() {
            return true;
        }

        let hostname_lower = hostname.to_lowercase();

        for pattern in &self.hostnames {
            let pattern_lower = pattern.to_lowercase();

            // Exact match
            if hostname_lower == pattern_lower {
                return true;
            }

            // Wildcard match (*.example.com) - only matches single level
            // e.g., *.example.com matches api.example.com but NOT sub.api.example.com
            if let Some(suffix) = pattern_lower.strip_prefix("*.") {
                // Check if hostname ends with .suffix
                let expected_suffix = format!(".{}", suffix);
                if hostname_lower.ends_with(&expected_suffix) {
                    // Extract the prefix part (everything before .suffix)
                    let prefix_len = hostname_lower.len() - expected_suffix.len();
                    let prefix = &hostname_lower[..prefix_len];
                    // Prefix must not contain dots (single level only)
                    if !prefix.contains('.') && !prefix.is_empty() {
                        return true;
                    }
                }
            }
        }

        false
    }

    /// Find matching backends for a request
    pub fn find_backends(
        &self,
        path: &str,
        method: &str,
        headers: &http::HeaderMap,
    ) -> Option<&BackendRef> {
        for rule in &self.rules {
            if rule.matches(path, method, headers).is_some() {
                return rule.select_backend();
            }
        }
        None
    }
}

/// Cached routes with timestamp
struct CachedRoutes {
    routes: Vec<HttpRoute>,
    fetched_at: Instant,
}

/// Gateway router that matches requests to HTTPRoute backends
pub struct GatewayRouter {
    /// Kubernetes client
    client: Option<Client>,
    /// Cached routes by hostname
    route_cache: Arc<RwLock<Option<CachedRoutes>>>,
    /// Whether Gateway API CRDs are available
    gateway_api_available: Arc<RwLock<Option<bool>>>,
}

impl GatewayRouter {
    /// Create a new gateway router
    pub fn new() -> Self {
        Self {
            client: None,
            route_cache: Arc::new(RwLock::new(None)),
            gateway_api_available: Arc::new(RwLock::new(None)),
        }
    }

    /// Initialize the Kubernetes client
    async fn get_client(&mut self) -> Result<Client, GatewayError> {
        if let Some(ref client) = self.client {
            return Ok(client.clone());
        }

        debug!("Initializing Kubernetes client for gateway routing...");

        let config = Config::infer()
            .await
            .map_err(|e| GatewayError::ParseError(format!("Failed to load kubeconfig: {}", e)))?;

        let client = Client::try_from(config)?;
        self.client = Some(client.clone());

        Ok(client)
    }

    /// Check if Gateway API CRDs are available in the cluster
    async fn check_gateway_api_available(&mut self) -> bool {
        // Check cache first
        {
            let available = self.gateway_api_available.read().await;
            if let Some(available) = *available {
                return available;
            }
        }

        let client = match self.get_client().await {
            Ok(c) => c,
            Err(_) => return false,
        };

        let gvk = GroupVersionKind::gvk("gateway.networking.k8s.io", "v1", "HTTPRoute");
        let api_resource = ApiResource::from_gvk(&gvk);
        let api: Api<DynamicObject> = Api::all_with(client, &api_resource);

        // Try to list with limit 1 to check if the API is available
        let available = api.list(&ListParams::default().limit(1)).await.is_ok();

        // Cache the result
        {
            let mut cached = self.gateway_api_available.write().await;
            *cached = Some(available);
        }

        if !available {
            debug!("Gateway API CRDs not found in cluster");
        }

        available
    }

    /// Fetch all HTTPRoute resources from Kubernetes
    async fn fetch_routes(&mut self) -> Result<Vec<HttpRoute>, GatewayError> {
        // Check cache first
        {
            let cache = self.route_cache.read().await;
            if let Some(ref cached) = *cache {
                if cached.fetched_at.elapsed() < Duration::from_secs(ROUTE_CACHE_TTL_SECS) {
                    debug!("Using cached HTTPRoutes ({} routes)", cached.routes.len());
                    return Ok(cached.routes.clone());
                }
            }
        }

        // Check if Gateway API is available
        if !self.check_gateway_api_available().await {
            return Ok(Vec::new());
        }

        let client = self.get_client().await?;

        debug!("Fetching HTTPRoute resources from Kubernetes...");

        let gvk = GroupVersionKind::gvk("gateway.networking.k8s.io", "v1", "HTTPRoute");
        let api_resource = ApiResource::from_gvk(&gvk);
        let api: Api<DynamicObject> = Api::all_with(client, &api_resource);

        let route_list = api.list(&ListParams::default()).await?;

        let routes: Vec<HttpRoute> = route_list
            .items
            .into_iter()
            .filter_map(|obj| self.parse_http_route(obj))
            .collect();

        info!("Loaded {} HTTPRoutes from Kubernetes", routes.len());

        // Update cache
        {
            let mut cache = self.route_cache.write().await;
            *cache = Some(CachedRoutes {
                routes: routes.clone(),
                fetched_at: Instant::now(),
            });
        }

        Ok(routes)
    }

    /// Parse a DynamicObject into an HttpRoute
    fn parse_http_route(&self, obj: DynamicObject) -> Option<HttpRoute> {
        let name = obj.metadata.name?;
        let namespace = obj
            .metadata
            .namespace
            .unwrap_or_else(|| "default".to_string());

        let spec = obj.data.get("spec")?.as_object()?;

        // Parse hostnames
        let hostnames: Vec<String> = spec
            .get("hostnames")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|h| h.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        // Parse rules
        let rules_array = spec.get("rules").and_then(|v| v.as_array())?;
        let mut rules = Vec::new();

        for rule_val in rules_array {
            let rule_obj = rule_val.as_object()?;

            // Parse matches
            let mut matches = Vec::new();
            if let Some(matches_array) = rule_obj.get("matches").and_then(|m| m.as_array()) {
                for match_val in matches_array {
                    if let Some(route_match) = self.parse_route_match(match_val) {
                        matches.push(route_match);
                    }
                }
            }

            // Parse backends
            let mut backends = Vec::new();
            if let Some(backends_array) = rule_obj.get("backendRefs").and_then(|b| b.as_array()) {
                for backend_val in backends_array {
                    if let Some(backend_obj) = backend_val.as_object() {
                        let svc_name = backend_obj
                            .get("name")
                            .and_then(|n| n.as_str())
                            .unwrap_or_default()
                            .to_string();

                        if svc_name.is_empty() {
                            continue;
                        }

                        let svc_namespace = backend_obj
                            .get("namespace")
                            .and_then(|n| n.as_str())
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| namespace.clone());

                        let port = backend_obj
                            .get("port")
                            .and_then(|p| p.as_u64())
                            .unwrap_or(80) as u16;

                        let weight = backend_obj
                            .get("weight")
                            .and_then(|w| w.as_u64())
                            .unwrap_or(1) as u32;

                        backends.push(BackendRef {
                            name: svc_name,
                            namespace: svc_namespace,
                            port,
                            weight,
                        });
                    }
                }
            }

            if !backends.is_empty() {
                rules.push(RouteRule { matches, backends });
            }
        }

        if rules.is_empty() {
            return None;
        }

        Some(HttpRoute {
            name,
            namespace,
            hostnames,
            rules,
        })
    }

    /// Parse a match object from HTTPRoute
    fn parse_route_match(&self, match_val: &serde_json::Value) -> Option<RouteMatch> {
        let match_obj = match_val.as_object()?;

        // Parse path match
        let path = match_obj
            .get("path")
            .and_then(|p| p.as_object())
            .and_then(|path_obj| {
                let match_type = path_obj
                    .get("type")
                    .and_then(|t| t.as_str())
                    .unwrap_or("PathPrefix");

                let value = path_obj.get("value").and_then(|v| v.as_str())?;

                match match_type {
                    "Exact" => Some(PathMatch::Exact(value.to_string())),
                    "PathPrefix" => Some(PathMatch::PathPrefix(value.to_string())),
                    "RegularExpression" => Some(PathMatch::RegularExpression(value.to_string())),
                    _ => Some(PathMatch::PathPrefix(value.to_string())),
                }
            });

        // Parse header matches
        let headers: Vec<HeaderMatch> = match_obj
            .get("headers")
            .and_then(|h| h.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|hdr| {
                        let hdr_obj = hdr.as_object()?;
                        let name = hdr_obj.get("name")?.as_str()?.to_string();
                        let value = hdr_obj.get("value")?.as_str()?.to_string();
                        let match_type = hdr_obj
                            .get("type")
                            .and_then(|t| t.as_str())
                            .unwrap_or("Exact");

                        let match_type = match match_type {
                            "RegularExpression" => HeaderMatchType::RegularExpression,
                            _ => HeaderMatchType::Exact,
                        };

                        Some(HeaderMatch {
                            name,
                            match_type,
                            value,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        // Parse method match
        let method = match_obj
            .get("method")
            .and_then(|m| m.as_str())
            .map(|s| s.to_string());

        Some(RouteMatch {
            path,
            headers,
            method,
        })
    }

    /// Resolve a hostname to a backend service using HTTPRoute rules
    ///
    /// Returns the backend service DNS name and port if a match is found.
    pub async fn resolve(
        &mut self,
        hostname: &str,
        path: &str,
        method: &str,
        headers: &http::HeaderMap,
    ) -> Result<Option<(String, u16)>, GatewayError> {
        let routes = self.fetch_routes().await?;

        // Strip port from hostname if present
        let hostname_only = hostname.split(':').next().unwrap_or(hostname);

        debug!("Resolving gateway route for {}{}", hostname_only, path);

        // Find matching routes
        for route in &routes {
            if !route.matches_hostname(hostname_only) {
                continue;
            }

            debug!(
                "HTTPRoute {} matches hostname {}",
                route.name, hostname_only
            );

            if let Some(backend) = route.find_backends(path, method, headers) {
                info!(
                    "Matched HTTPRoute {}: {} -> {}:{} ({})",
                    route.name, path, backend.name, backend.port, backend.namespace
                );

                return Ok(Some((backend.dns_name(), backend.port)));
            }
        }

        debug!("No HTTPRoute match found for {}{}", hostname_only, path);
        Ok(None)
    }

    /// Invalidate the route cache (e.g., after a change notification)
    pub async fn invalidate_cache(&self) {
        let mut cache = self.route_cache.write().await;
        *cache = None;
        debug!("Gateway route cache invalidated");
    }

    /// Get cached routes without fetching
    pub async fn get_cached_routes(&self) -> Vec<HttpRoute> {
        let cache = self.route_cache.read().await;
        cache.as_ref().map(|c| c.routes.clone()).unwrap_or_default()
    }
}

impl Default for GatewayRouter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_match_exact() {
        let m = PathMatch::Exact("/orders".to_string());
        assert!(m.matches("/orders"));
        assert!(!m.matches("/orders/"));
        assert!(!m.matches("/orders/123"));
    }

    #[test]
    fn test_path_match_prefix() {
        let m = PathMatch::PathPrefix("/api".to_string());
        assert!(m.matches("/api"));
        assert!(m.matches("/api/"));
        assert!(m.matches("/api/orders"));
        assert!(!m.matches("/apiv2"));
    }

    #[test]
    fn test_hostname_exact_match() {
        let route = HttpRoute {
            name: "test".to_string(),
            namespace: "default".to_string(),
            hostnames: vec!["api.example.com".to_string()],
            rules: vec![],
        };

        assert!(route.matches_hostname("api.example.com"));
        assert!(route.matches_hostname("API.EXAMPLE.COM"));
        assert!(!route.matches_hostname("other.example.com"));
    }

    #[test]
    fn test_hostname_wildcard_match() {
        let route = HttpRoute {
            name: "test".to_string(),
            namespace: "default".to_string(),
            hostnames: vec!["*.example.com".to_string()],
            rules: vec![],
        };

        assert!(route.matches_hostname("api.example.com"));
        assert!(route.matches_hostname("orders.example.com"));
        assert!(!route.matches_hostname("example.com"));
        assert!(!route.matches_hostname("sub.api.example.com"));
    }

    #[test]
    fn test_empty_hostnames_matches_all() {
        let route = HttpRoute {
            name: "test".to_string(),
            namespace: "default".to_string(),
            hostnames: vec![],
            rules: vec![],
        };

        assert!(route.matches_hostname("anything.com"));
    }

    #[test]
    fn test_backend_dns_name() {
        let backend = BackendRef {
            name: "orders".to_string(),
            namespace: "production".to_string(),
            port: 8080,
            weight: 1,
        };

        assert_eq!(backend.dns_name(), "orders.production.svc.cluster.local");
    }

    #[test]
    fn test_route_match_method() {
        let route_match = RouteMatch {
            path: Some(PathMatch::PathPrefix("/".to_string())),
            headers: vec![],
            method: Some("POST".to_string()),
        };

        let headers = http::HeaderMap::new();
        assert!(route_match.matches("/orders", "POST", &headers));
        assert!(route_match.matches("/orders", "post", &headers));
        assert!(!route_match.matches("/orders", "GET", &headers));
    }

    #[test]
    fn test_full_route_matching() {
        // Create a route similar to what would come from an HTTPRoute resource
        let route = HttpRoute {
            name: "orders-route".to_string(),
            namespace: "production".to_string(),
            hostnames: vec!["api.example.com".to_string()],
            rules: vec![
                RouteRule {
                    matches: vec![RouteMatch {
                        path: Some(PathMatch::PathPrefix("/orders".to_string())),
                        headers: vec![],
                        method: None,
                    }],
                    backends: vec![BackendRef {
                        name: "orders-service".to_string(),
                        namespace: "production".to_string(),
                        port: 8080,
                        weight: 1,
                    }],
                },
                RouteRule {
                    matches: vec![RouteMatch {
                        path: Some(PathMatch::PathPrefix("/users".to_string())),
                        headers: vec![],
                        method: None,
                    }],
                    backends: vec![BackendRef {
                        name: "users-service".to_string(),
                        namespace: "production".to_string(),
                        port: 8080,
                        weight: 1,
                    }],
                },
            ],
        };

        let headers = http::HeaderMap::new();

        // Should match /orders path
        let backend = route.find_backends("/orders", "GET", &headers);
        assert!(backend.is_some());
        assert_eq!(backend.unwrap().name, "orders-service");

        // Should match /orders/123 path (prefix match)
        let backend = route.find_backends("/orders/123", "GET", &headers);
        assert!(backend.is_some());
        assert_eq!(backend.unwrap().name, "orders-service");

        // Should match /users path
        let backend = route.find_backends("/users", "GET", &headers);
        assert!(backend.is_some());
        assert_eq!(backend.unwrap().name, "users-service");

        // Should not match /products path
        let backend = route.find_backends("/products", "GET", &headers);
        assert!(backend.is_none());
    }

    #[test]
    fn test_route_with_no_matches_matches_all() {
        // A rule with no matches should match all requests
        let route = HttpRoute {
            name: "catch-all".to_string(),
            namespace: "default".to_string(),
            hostnames: vec![],
            rules: vec![RouteRule {
                matches: vec![], // No match conditions
                backends: vec![BackendRef {
                    name: "default-backend".to_string(),
                    namespace: "default".to_string(),
                    port: 80,
                    weight: 1,
                }],
            }],
        };

        let headers = http::HeaderMap::new();

        // Should match any path
        let backend = route.find_backends("/anything", "GET", &headers);
        assert!(backend.is_some());
        assert_eq!(backend.unwrap().name, "default-backend");
    }
}
