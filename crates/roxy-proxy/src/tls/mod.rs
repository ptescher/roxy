//! TLS Certificate Management for Roxy Proxy
//!
//! This module provides functionality for:
//! - Generating a root Certificate Authority (CA)
//! - Generating server certificates on demand for TLS interception
//! - Managing which hosts should have TLS intercepted
//!
//! ## TLS Interception Modes
//!
//! By default, HTTPS CONNECT requests are tunneled through without interception,
//! preserving end-to-end encryption. For hosts in the interception list, Roxy
//! will perform TLS termination and re-encryption, allowing inspection of the
//! traffic.

pub mod ca;
pub mod cert_cache;
pub mod config;

pub use ca::{CertificateAuthority, CertificateAuthorityError};
pub use cert_cache::CertificateCache;
pub use config::{TlsConfig, TlsInterceptionMode};

use std::sync::Arc;
use tokio::sync::RwLock;

/// TLS manager that coordinates CA and certificate generation
pub struct TlsManager {
    /// The Certificate Authority for signing certificates
    ca: Arc<CertificateAuthority>,
    /// Cache for generated certificates
    cert_cache: Arc<CertificateCache>,
    /// Configuration for TLS interception
    config: Arc<RwLock<TlsConfig>>,
}

impl TlsManager {
    /// Create a new TLS manager with the given configuration
    ///
    /// If no CA exists at the configured path, one will be generated.
    pub async fn new(config: TlsConfig) -> Result<Self, CertificateAuthorityError> {
        let ca = CertificateAuthority::load_or_create(&config.ca_cert_path, &config.ca_key_path)?;
        let cert_cache = CertificateCache::new(config.cache_size);

        Ok(Self {
            ca: Arc::new(ca),
            cert_cache: Arc::new(cert_cache),
            config: Arc::new(RwLock::new(config)),
        })
    }

    /// Check if TLS interception is enabled for a given host
    pub async fn should_intercept(&self, host: &str) -> bool {
        let config = self.config.read().await;
        config.should_intercept(host)
    }

    /// Get or generate a certificate for the given host
    pub async fn get_certificate(
        &self,
        host: &str,
    ) -> Result<Arc<rustls::ServerConfig>, CertificateAuthorityError> {
        // Check cache first
        if let Some(config) = self.cert_cache.get(host).await {
            return Ok(config);
        }

        // Generate new certificate
        let (cert, key) = self.ca.generate_cert(host)?;

        // Build rustls ServerConfig
        let server_config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![cert], key)
            .map_err(|e| CertificateAuthorityError::RustlsError(e.to_string()))?;

        let server_config = Arc::new(server_config);

        // Cache it
        self.cert_cache
            .insert(host.to_string(), server_config.clone())
            .await;

        Ok(server_config)
    }

    /// Get the CA certificate in PEM format for installation in trust stores
    pub fn ca_cert_pem(&self) -> &str {
        self.ca.cert_pem()
    }

    /// Get the path to the CA certificate file
    pub async fn ca_cert_path(&self) -> String {
        let config = self.config.read().await;
        config.ca_cert_path.clone()
    }

    /// Add a host to the interception list
    pub async fn add_intercept_host(&self, host: String) {
        let mut config = self.config.write().await;
        config.add_intercept_host(host);
    }

    /// Remove a host from the interception list
    pub async fn remove_intercept_host(&self, host: &str) {
        let mut config = self.config.write().await;
        config.remove_intercept_host(host);
    }

    /// Get the list of hosts configured for interception
    pub async fn intercept_hosts(&self) -> Vec<String> {
        let config = self.config.read().await;
        config.intercept_hosts.clone()
    }

    /// Get the underlying CA
    pub fn ca(&self) -> &Arc<CertificateAuthority> {
        &self.ca
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_tls_manager_creation() {
        let dir = tempdir().unwrap();
        let cert_path = dir.path().join("ca.crt").to_string_lossy().to_string();
        let key_path = dir.path().join("ca.key").to_string_lossy().to_string();

        let config = TlsConfig {
            ca_cert_path: cert_path,
            ca_key_path: key_path,
            ..Default::default()
        };

        let manager = TlsManager::new(config).await.unwrap();
        assert!(!manager.ca_cert_pem().is_empty());
    }

    #[tokio::test]
    async fn test_intercept_host_management() {
        let dir = tempdir().unwrap();
        let config = TlsConfig {
            ca_cert_path: dir.path().join("ca.crt").to_string_lossy().to_string(),
            ca_key_path: dir.path().join("ca.key").to_string_lossy().to_string(),
            enabled: true,
            mode: crate::tls::TlsInterceptionMode::InterceptListed,
            ..Default::default()
        };

        let manager = TlsManager::new(config).await.unwrap();

        // By default, no hosts are intercepted (empty list)
        assert!(!manager.should_intercept("example.com").await);

        // Add a host
        manager.add_intercept_host("example.com".to_string()).await;
        assert!(manager.should_intercept("example.com").await);

        // Remove the host
        manager.remove_intercept_host("example.com").await;
        assert!(!manager.should_intercept("example.com").await);
    }
}
