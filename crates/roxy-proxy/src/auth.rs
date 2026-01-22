//! JWT Authentication and Header Injection Module
//!
//! This module provides JWT introspection and header injection capabilities
//! for the roxy proxy. In Kubernetes environments, sidecars (like Istio or
//! Oathkeeper) typically perform JWT validation and inject headers like
//! `X-User-ID` before requests reach backend services.
//!
//! When traffic is routed to local backends via roxy, this authentication
//! layer is bypassed. This module restores that functionality by:
//!
//! 1. Extracting Bearer tokens from the `Authorization` header
//! 2. Validating JWTs using JWKS (JSON Web Key Set)
//! 3. Injecting configured claims as headers (e.g., `sub` → `X-User-ID`)
//!
//! ## Usage
//!
//! Configure auth via the control API:
//!
//! ```bash
//! curl -X POST http://localhost:8889/auth/configure \
//!   -H "Content-Type: application/json" \
//!   -d '{
//!     "jwks_url": "https://auth.example.com/.well-known/jwks.json",
//!     "issuer": "https://auth.example.com",
//!     "header_mappings": [
//!       { "claim": "sub", "header": "X-User-ID" },
//!       { "claim": "email", "header": "X-User-Email" }
//!     ]
//!   }'
//! ```

use jsonwebtoken::{
    decode, decode_header, jwk::JwkSet, DecodingKey, TokenData, Validation,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

/// How often to refresh JWKS keys (in seconds)
const JWKS_CACHE_TTL_SECS: u64 = 3600; // 1 hour

/// How long to wait before retrying JWKS fetch after a failure
const JWKS_RETRY_DELAY_SECS: u64 = 60; // 1 minute

/// Maximum time to wait for JWKS fetch
const JWKS_FETCH_TIMEOUT_SECS: u64 = 10;

/// Error types for auth operations
#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("No Authorization header present")]
    NoAuthHeader,

    #[error("Invalid Authorization header format")]
    InvalidAuthHeader,

    #[error("Token validation failed: {0}")]
    ValidationFailed(String),

    #[error("JWKS fetch failed: {0}")]
    JwksFetchFailed(String),

    #[error("Auth not configured")]
    NotConfigured,

    #[error("Key not found for kid: {0}")]
    KeyNotFound(String),
}

/// A mapping from JWT claim to HTTP header
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeaderMapping {
    /// The JWT claim name (e.g., "sub", "email")
    pub claim: String,
    /// The HTTP header to inject (e.g., "X-User-ID", "X-User-Email")
    pub header: String,
}

/// Auth configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    /// URL to fetch JWKS keys from
    pub jwks_url: String,
    /// Expected token issuer (iss claim) - required for validation
    pub issuer: String,
    /// Expected token audience (aud claim) - optional
    #[serde(default)]
    pub audience: Option<String>,
    /// Claim-to-header mappings
    #[serde(default)]
    pub header_mappings: Vec<HeaderMapping>,
    /// Whether auth injection is enabled
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            jwks_url: String::new(),
            issuer: String::new(),
            audience: None,
            header_mappings: Vec::new(),
            enabled: false,
        }
    }
}

/// Cached JWKS with timestamp
struct CachedJwks {
    jwks: JwkSet,
    fetched_at: Instant,
    /// Whether the last fetch was successful (for retry logic)
    fetch_succeeded: bool,
}

/// JWT claims - we use a HashMap for flexibility since different
/// identity providers include different claims
#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    /// Standard claims
    #[serde(default)]
    pub iss: Option<String>,
    #[serde(default)]
    pub sub: Option<String>,
    #[serde(default)]
    pub aud: Option<serde_json::Value>,
    #[serde(default)]
    pub exp: Option<u64>,
    #[serde(default)]
    pub iat: Option<u64>,
    #[serde(default)]
    pub nbf: Option<u64>,

    /// All other claims as a flat map
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

impl Claims {
    /// Get a claim value by name
    pub fn get(&self, name: &str) -> Option<String> {
        match name {
            "iss" => self.iss.clone(),
            "sub" => self.sub.clone(),
            "aud" => self.aud.as_ref().map(|v| match v {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Array(arr) => arr
                    .iter()
                    .filter_map(|v| v.as_str())
                    .collect::<Vec<_>>()
                    .join(","),
                other => other.to_string(),
            }),
            "exp" => self.exp.map(|v| v.to_string()),
            "iat" => self.iat.map(|v| v.to_string()),
            "nbf" => self.nbf.map(|v| v.to_string()),
            other => self.extra.get(other).map(|v| match v {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::Bool(b) => b.to_string(),
                other => other.to_string(),
            }),
        }
    }
}

/// Result of JWT validation with extracted headers
#[derive(Debug)]
pub struct AuthResult {
    /// The validated claims
    pub claims: Claims,
    /// Headers to inject based on configured mappings
    pub headers_to_inject: Vec<(String, String)>,
}

/// JWT Authentication manager
///
/// Handles JWT validation using JWKS and header injection.
pub struct AuthManager {
    /// Current configuration
    config: Arc<RwLock<Option<AuthConfig>>>,
    /// Cached JWKS
    jwks_cache: Arc<RwLock<Option<CachedJwks>>>,
    /// HTTP client for JWKS fetching
    http_client: reqwest::Client,
}

impl AuthManager {
    /// Create a new auth manager
    pub fn new() -> Self {
        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(JWKS_FETCH_TIMEOUT_SECS))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            config: Arc::new(RwLock::new(None)),
            jwks_cache: Arc::new(RwLock::new(None)),
            http_client,
        }
    }

    /// Configure the auth manager
    pub async fn configure(&self, config: AuthConfig) {
        info!(
            jwks_url = %config.jwks_url,
            issuer = %config.issuer,
            mappings = %config.header_mappings.len(),
            "Configuring JWT auth"
        );

        // Clear JWKS cache when configuration changes
        {
            let mut cache = self.jwks_cache.write().await;
            *cache = None;
        }

        {
            let mut cfg = self.config.write().await;
            *cfg = Some(config);
        }

        // Pre-fetch JWKS in background
        let manager = self.clone();
        tokio::spawn(async move {
            if let Err(e) = manager.refresh_jwks().await {
                warn!(error = %e, "Failed to pre-fetch JWKS");
            }
        });
    }

    /// Get current configuration
    pub async fn get_config(&self) -> Option<AuthConfig> {
        let config = self.config.read().await;
        config.clone()
    }

    /// Disable auth
    pub async fn disable(&self) {
        info!("Disabling JWT auth");
        let mut config = self.config.write().await;
        *config = None;

        let mut cache = self.jwks_cache.write().await;
        *cache = None;
    }

    /// Check if auth is enabled
    pub async fn is_enabled(&self) -> bool {
        let config = self.config.read().await;
        config.as_ref().map(|c| c.enabled).unwrap_or(false)
    }

    /// Fetch JWKS from the configured URL
    async fn fetch_jwks(&self) -> Result<JwkSet, AuthError> {
        let config = {
            let cfg = self.config.read().await;
            cfg.clone()
                .ok_or(AuthError::NotConfigured)?
        };

        debug!(url = %config.jwks_url, "Fetching JWKS");

        let response = self
            .http_client
            .get(&config.jwks_url)
            .send()
            .await
            .map_err(|e| AuthError::JwksFetchFailed(e.to_string()))?;

        if !response.status().is_success() {
            return Err(AuthError::JwksFetchFailed(format!(
                "HTTP {}: {}",
                response.status(),
                response.status().canonical_reason().unwrap_or("Unknown")
            )));
        }

        let jwks: JwkSet = response
            .json()
            .await
            .map_err(|e| AuthError::JwksFetchFailed(format!("Failed to parse JWKS: {}", e)))?;

        info!(keys = %jwks.keys.len(), "Fetched JWKS successfully");

        Ok(jwks)
    }

    /// Refresh JWKS cache
    async fn refresh_jwks(&self) -> Result<(), AuthError> {
        let jwks = self.fetch_jwks().await?;

        let mut cache = self.jwks_cache.write().await;
        *cache = Some(CachedJwks {
            jwks,
            fetched_at: Instant::now(),
            fetch_succeeded: true,
        });

        Ok(())
    }

    /// Get JWKS, using cache if available
    async fn get_jwks(&self) -> Result<JwkSet, AuthError> {
        // Check cache first
        {
            let cache = self.jwks_cache.read().await;
            if let Some(ref cached) = *cache {
                let elapsed = cached.fetched_at.elapsed();

                // Use cache if it's fresh
                if elapsed < Duration::from_secs(JWKS_CACHE_TTL_SECS) {
                    return Ok(cached.jwks.clone());
                }

                // If last fetch failed, use shorter retry delay
                if !cached.fetch_succeeded
                    && elapsed < Duration::from_secs(JWKS_RETRY_DELAY_SECS)
                {
                    // Return cached version even if stale
                    return Ok(cached.jwks.clone());
                }
            }
        }

        // Fetch new JWKS
        match self.fetch_jwks().await {
            Ok(jwks) => {
                let mut cache = self.jwks_cache.write().await;
                *cache = Some(CachedJwks {
                    jwks: jwks.clone(),
                    fetched_at: Instant::now(),
                    fetch_succeeded: true,
                });
                Ok(jwks)
            }
            Err(e) => {
                // Mark as failed but keep the stale cache
                {
                    let mut cache = self.jwks_cache.write().await;
                    if let Some(ref mut cached) = *cache {
                        cached.fetch_succeeded = false;
                        cached.fetched_at = Instant::now();
                        // Return stale cache on fetch failure
                        return Ok(cached.jwks.clone());
                    }
                }
                Err(e)
            }
        }
    }

    /// Extract Bearer token from Authorization header
    fn extract_bearer_token(headers: &http::HeaderMap) -> Result<&str, AuthError> {
        let auth_header = headers
            .get("authorization")
            .ok_or(AuthError::NoAuthHeader)?
            .to_str()
            .map_err(|_| AuthError::InvalidAuthHeader)?;

        // Check for Bearer prefix (case-insensitive)
        if auth_header.len() < 7 {
            return Err(AuthError::InvalidAuthHeader);
        }

        let prefix = &auth_header[..7];
        if !prefix.eq_ignore_ascii_case("bearer ") {
            return Err(AuthError::InvalidAuthHeader);
        }

        let token = auth_header[7..].trim();
        if token.is_empty() {
            return Err(AuthError::InvalidAuthHeader);
        }

        Ok(token)
    }

    /// Find the decoding key for a JWT based on its header
    fn find_key(&self, header: &jsonwebtoken::Header, jwks: &JwkSet) -> Result<DecodingKey, AuthError> {
        // If the token has a kid, look for that specific key
        if let Some(kid) = &header.kid {
            for jwk in &jwks.keys {
                if jwk.common.key_id.as_ref() == Some(kid) {
                    return DecodingKey::from_jwk(jwk)
                        .map_err(|e| AuthError::ValidationFailed(format!("Invalid JWK: {}", e)));
                }
            }
            return Err(AuthError::KeyNotFound(kid.clone()));
        }

        // No kid, try to find a key that matches the algorithm
        for jwk in &jwks.keys {
            if let Some(alg) = &jwk.common.key_algorithm {
                // Compare algorithm names
                let alg_str = format!("{:?}", alg);
                let header_alg = format!("{:?}", header.alg);
                if alg_str == header_alg {
                    return DecodingKey::from_jwk(jwk)
                        .map_err(|e| AuthError::ValidationFailed(format!("Invalid JWK: {}", e)));
                }
            }
        }

        // Try the first key as a fallback
        jwks.keys
            .first()
            .ok_or_else(|| AuthError::KeyNotFound("No keys in JWKS".to_string()))
            .and_then(|jwk| {
                DecodingKey::from_jwk(jwk)
                    .map_err(|e| AuthError::ValidationFailed(format!("Invalid JWK: {}", e)))
            })
    }

    /// Validate a JWT and return the claims
    async fn validate_token(&self, token: &str) -> Result<Claims, AuthError> {
        let config = {
            let cfg = self.config.read().await;
            cfg.clone().ok_or(AuthError::NotConfigured)?
        };

        // Decode header to find the key
        let header = decode_header(token)
            .map_err(|e| AuthError::ValidationFailed(format!("Invalid token header: {}", e)))?;

        // Get JWKS and find the appropriate key
        let jwks = self.get_jwks().await?;
        let decoding_key = self.find_key(&header, &jwks)?;

        // Set up validation
        let mut validation = Validation::new(header.alg);
        validation.set_issuer(&[&config.issuer]);

        if let Some(ref aud) = config.audience {
            validation.set_audience(&[aud]);
        } else {
            // Disable audience validation if not configured
            validation.validate_aud = false;
        }

        // Decode and validate
        let token_data: TokenData<Claims> = decode(token, &decoding_key, &validation)
            .map_err(|e| AuthError::ValidationFailed(format!("{}", e)))?;

        debug!(sub = ?token_data.claims.sub, "JWT validated successfully");

        Ok(token_data.claims)
    }

    /// Process a request and return headers to inject
    ///
    /// This is the main entry point for auth processing.
    /// Returns None if:
    /// - Auth is not configured/enabled
    /// - No Authorization header present
    /// - Token validation fails
    ///
    /// Returns Some(headers) if validation succeeds
    pub async fn process_request(&self, headers: &http::HeaderMap) -> Option<AuthResult> {
        // Check if enabled
        if !self.is_enabled().await {
            return None;
        }

        // Extract token
        let token = match Self::extract_bearer_token(headers) {
            Ok(t) => t,
            Err(e) => {
                debug!(error = %e, "No bearer token found");
                return None;
            }
        };

        // Validate token
        let claims = match self.validate_token(token).await {
            Ok(c) => c,
            Err(e) => {
                warn!(error = %e, "JWT validation failed");
                return None;
            }
        };

        // Get header mappings
        let config = {
            let cfg = self.config.read().await;
            cfg.clone()
        };

        let Some(config) = config else {
            return None;
        };

        // Build headers to inject
        let mut headers_to_inject = Vec::new();
        for mapping in &config.header_mappings {
            if let Some(value) = claims.get(&mapping.claim) {
                headers_to_inject.push((mapping.header.clone(), value));
            }
        }

        info!(
            sub = ?claims.sub,
            headers_injected = %headers_to_inject.len(),
            "JWT auth successful"
        );

        Some(AuthResult {
            claims,
            headers_to_inject,
        })
    }

    /// Inject auth headers into a request builder
    ///
    /// This is a convenience method for use with hyper request builders.
    pub fn inject_headers(
        &self,
        builder: http::request::Builder,
        auth_result: &AuthResult,
    ) -> http::request::Builder {
        let mut builder = builder;
        for (name, value) in &auth_result.headers_to_inject {
            builder = builder.header(name.as_str(), value.as_str());
        }
        builder
    }

    /// Inject auth headers into an existing HeaderMap
    pub fn inject_into_headers(&self, headers: &mut http::HeaderMap, auth_result: &AuthResult) {
        for (name, value) in &auth_result.headers_to_inject {
            if let Ok(header_name) = http::header::HeaderName::try_from(name.as_str()) {
                if let Ok(header_value) = http::header::HeaderValue::from_str(value) {
                    headers.insert(header_name, header_value);
                }
            }
        }
    }
}

impl Clone for AuthManager {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            jwks_cache: self.jwks_cache.clone(),
            http_client: self.http_client.clone(),
        }
    }
}

impl Default for AuthManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_bearer_token() {
        let mut headers = http::HeaderMap::new();

        // No header
        assert!(AuthManager::extract_bearer_token(&headers).is_err());

        // Invalid format
        headers.insert("authorization", "Basic abc123".parse().unwrap());
        assert!(AuthManager::extract_bearer_token(&headers).is_err());

        // Valid bearer token
        headers.insert("authorization", "Bearer abc123".parse().unwrap());
        assert_eq!(
            AuthManager::extract_bearer_token(&headers).unwrap(),
            "abc123"
        );

        // Case insensitive
        headers.insert("authorization", "bearer def456".parse().unwrap());
        assert_eq!(
            AuthManager::extract_bearer_token(&headers).unwrap(),
            "def456"
        );

        // With whitespace
        headers.insert("authorization", "Bearer   ghi789  ".parse().unwrap());
        assert_eq!(
            AuthManager::extract_bearer_token(&headers).unwrap(),
            "ghi789"
        );
    }

    #[test]
    fn test_claims_get() {
        let claims = Claims {
            iss: Some("https://auth.example.com".to_string()),
            sub: Some("user123".to_string()),
            aud: Some(serde_json::json!("my-api")),
            exp: Some(1234567890),
            iat: Some(1234567800),
            nbf: None,
            extra: {
                let mut map = HashMap::new();
                map.insert("email".to_string(), serde_json::json!("user@example.com"));
                map.insert("roles".to_string(), serde_json::json!(["admin", "user"]));
                map
            },
        };

        assert_eq!(claims.get("iss"), Some("https://auth.example.com".to_string()));
        assert_eq!(claims.get("sub"), Some("user123".to_string()));
        assert_eq!(claims.get("email"), Some("user@example.com".to_string()));
        assert_eq!(claims.get("nonexistent"), None);
    }

    #[test]
    fn test_header_mapping_serialization() {
        let mapping = HeaderMapping {
            claim: "sub".to_string(),
            header: "X-User-ID".to_string(),
        };

        let json = serde_json::to_string(&mapping).unwrap();
        assert!(json.contains("sub"));
        assert!(json.contains("X-User-ID"));

        let deserialized: HeaderMapping = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.claim, "sub");
        assert_eq!(deserialized.header, "X-User-ID");
    }

    #[test]
    fn test_auth_config_defaults() {
        let config: AuthConfig = serde_json::from_str(r#"{
            "jwks_url": "https://example.com/.well-known/jwks.json",
            "issuer": "https://example.com"
        }"#).unwrap();

        assert_eq!(config.jwks_url, "https://example.com/.well-known/jwks.json");
        assert_eq!(config.issuer, "https://example.com");
        assert!(config.audience.is_none());
        assert!(config.header_mappings.is_empty());
        assert!(config.enabled); // defaults to true
    }

    #[tokio::test]
    async fn test_auth_manager_disabled_by_default() {
        let manager = AuthManager::new();
        assert!(!manager.is_enabled().await);
        assert!(manager.get_config().await.is_none());
    }

    #[tokio::test]
    async fn test_auth_manager_configure_and_disable() {
        let manager = AuthManager::new();

        // Configure
        let config = AuthConfig {
            jwks_url: "https://example.com/.well-known/jwks.json".to_string(),
            issuer: "https://example.com".to_string(),
            audience: None,
            header_mappings: vec![HeaderMapping {
                claim: "sub".to_string(),
                header: "X-User-ID".to_string(),
            }],
            enabled: true,
        };

        manager.configure(config.clone()).await;
        assert!(manager.is_enabled().await);

        let retrieved = manager.get_config().await.unwrap();
        assert_eq!(retrieved.jwks_url, config.jwks_url);
        assert_eq!(retrieved.header_mappings.len(), 1);

        // Disable
        manager.disable().await;
        assert!(!manager.is_enabled().await);
        assert!(manager.get_config().await.is_none());
    }

    #[tokio::test]
    async fn test_process_request_disabled() {
        let manager = AuthManager::new();
        let headers = http::HeaderMap::new();
        assert!(manager.process_request(&headers).await.is_none());
    }

    #[test]
    fn test_inject_into_headers() {
        let manager = AuthManager::new();
        let mut headers = http::HeaderMap::new();

        let auth_result = AuthResult {
            claims: Claims {
                iss: Some("issuer".to_string()),
                sub: Some("user123".to_string()),
                aud: None,
                exp: None,
                iat: None,
                nbf: None,
                extra: HashMap::new(),
            },
            headers_to_inject: vec![
                ("X-User-ID".to_string(), "user123".to_string()),
                ("X-User-Email".to_string(), "user@example.com".to_string()),
            ],
        };

        manager.inject_into_headers(&mut headers, &auth_result);

        assert_eq!(
            headers.get("X-User-ID").unwrap().to_str().unwrap(),
            "user123"
        );
        assert_eq!(
            headers.get("X-User-Email").unwrap().to_str().unwrap(),
            "user@example.com"
        );
    }
}
