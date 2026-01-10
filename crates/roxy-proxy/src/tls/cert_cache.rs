//! Certificate Cache for TLS Interception
//!
//! Caches generated certificates to avoid regenerating them for every connection
//! to the same host. Uses an LRU-style eviction policy.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Default cache size (number of certificates to cache)
pub const DEFAULT_CACHE_SIZE: usize = 1000;

/// Entry in the certificate cache
struct CacheEntry {
    /// The cached server configuration
    config: Arc<rustls::ServerConfig>,
    /// Last access time (for LRU eviction)
    last_access: std::time::Instant,
}

/// LRU cache for generated TLS certificates
///
/// Stores `rustls::ServerConfig` instances keyed by hostname, evicting
/// least-recently-used entries when the cache is full.
pub struct CertificateCache {
    /// Maximum number of entries to cache
    max_size: usize,
    /// The cache entries
    entries: RwLock<HashMap<String, CacheEntry>>,
}

impl CertificateCache {
    /// Create a new certificate cache with the given maximum size
    pub fn new(max_size: usize) -> Self {
        Self {
            max_size,
            entries: RwLock::new(HashMap::with_capacity(max_size)),
        }
    }

    /// Create a cache with the default size
    pub fn with_default_size() -> Self {
        Self::new(DEFAULT_CACHE_SIZE)
    }

    /// Get a cached certificate configuration for the given host
    ///
    /// Returns `None` if not cached. Updates the last access time on hit.
    pub async fn get(&self, host: &str) -> Option<Arc<rustls::ServerConfig>> {
        let mut entries = self.entries.write().await;

        if let Some(entry) = entries.get_mut(host) {
            entry.last_access = std::time::Instant::now();
            Some(entry.config.clone())
        } else {
            None
        }
    }

    /// Insert a certificate configuration into the cache
    ///
    /// If the cache is full, evicts the least recently used entry.
    pub async fn insert(&self, host: String, config: Arc<rustls::ServerConfig>) {
        let mut entries = self.entries.write().await;

        // Evict if necessary
        if entries.len() >= self.max_size && !entries.contains_key(&host) {
            self.evict_lru(&mut entries);
        }

        entries.insert(
            host,
            CacheEntry {
                config,
                last_access: std::time::Instant::now(),
            },
        );
    }

    /// Remove a certificate from the cache
    pub async fn remove(&self, host: &str) -> bool {
        let mut entries = self.entries.write().await;
        entries.remove(host).is_some()
    }

    /// Clear all cached certificates
    pub async fn clear(&self) {
        let mut entries = self.entries.write().await;
        entries.clear();
    }

    /// Get the number of cached certificates
    pub async fn len(&self) -> usize {
        let entries = self.entries.read().await;
        entries.len()
    }

    /// Check if the cache is empty
    pub async fn is_empty(&self) -> bool {
        let entries = self.entries.read().await;
        entries.is_empty()
    }

    /// Get the maximum cache size
    pub fn max_size(&self) -> usize {
        self.max_size
    }

    /// Evict the least recently used entry
    fn evict_lru(&self, entries: &mut HashMap<String, CacheEntry>) {
        if entries.is_empty() {
            return;
        }

        // Find the entry with the oldest last_access time
        let oldest_key = entries
            .iter()
            .min_by_key(|(_, entry)| entry.last_access)
            .map(|(key, _)| key.clone());

        if let Some(key) = oldest_key {
            entries.remove(&key);
            tracing::debug!(host = %key, "Evicted certificate from cache");
        }
    }
}

impl Default for CertificateCache {
    fn default() -> Self {
        Self::with_default_size()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tls::CertificateAuthority;
    use std::sync::Once;

    static INIT: Once = Once::new();

    /// Install the ring crypto provider for tests
    fn init_crypto() {
        INIT.call_once(|| {
            let _ = rustls::crypto::ring::default_provider().install_default();
        });
    }

    // Helper to create a real ServerConfig for testing using our CA
    fn create_test_server_config(hostname: &str) -> Arc<rustls::ServerConfig> {
        init_crypto();
        let ca = CertificateAuthority::new().unwrap();
        let (cert, key) = ca.generate_cert(hostname).unwrap();

        Arc::new(
            rustls::ServerConfig::builder()
                .with_no_client_auth()
                .with_single_cert(vec![cert], key)
                .expect("Failed to create test server config"),
        )
    }

    #[tokio::test]
    async fn test_cache_insert_and_get() {
        let cache = CertificateCache::new(10);

        // Cache should be empty
        assert!(cache.is_empty().await);
        assert!(cache.get("example.com").await.is_none());

        // Insert an entry
        let config = create_test_server_config("example.com");
        cache
            .insert("example.com".to_string(), config.clone())
            .await;

        // Should be retrievable
        assert!(!cache.is_empty().await);
        assert_eq!(cache.len().await, 1);
        assert!(cache.get("example.com").await.is_some());
    }

    #[tokio::test]
    async fn test_cache_remove() {
        let cache = CertificateCache::new(10);
        let config = create_test_server_config("example.com");

        cache.insert("example.com".to_string(), config).await;
        assert_eq!(cache.len().await, 1);

        assert!(cache.remove("example.com").await);
        assert!(cache.is_empty().await);
        assert!(!cache.remove("example.com").await);
    }

    #[tokio::test]
    async fn test_cache_clear() {
        let cache = CertificateCache::new(10);

        cache
            .insert("a.com".to_string(), create_test_server_config("a.com"))
            .await;
        cache
            .insert("b.com".to_string(), create_test_server_config("b.com"))
            .await;
        cache
            .insert("c.com".to_string(), create_test_server_config("c.com"))
            .await;

        assert_eq!(cache.len().await, 3);

        cache.clear().await;
        assert!(cache.is_empty().await);
    }

    #[tokio::test]
    async fn test_cache_eviction() {
        let cache = CertificateCache::new(3);

        // Fill the cache
        cache
            .insert("a.com".to_string(), create_test_server_config("a.com"))
            .await;
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        cache
            .insert("b.com".to_string(), create_test_server_config("b.com"))
            .await;
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        cache
            .insert("c.com".to_string(), create_test_server_config("c.com"))
            .await;

        assert_eq!(cache.len().await, 3);

        // Adding a 4th entry should evict the oldest (a.com)
        cache
            .insert("d.com".to_string(), create_test_server_config("d.com"))
            .await;

        assert_eq!(cache.len().await, 3);
        assert!(cache.get("a.com").await.is_none());
        assert!(cache.get("b.com").await.is_some());
        assert!(cache.get("c.com").await.is_some());
        assert!(cache.get("d.com").await.is_some());
    }

    #[tokio::test]
    async fn test_cache_lru_update() {
        let cache = CertificateCache::new(3);

        // Fill the cache
        cache
            .insert("a.com".to_string(), create_test_server_config("a.com"))
            .await;
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        cache
            .insert("b.com".to_string(), create_test_server_config("b.com"))
            .await;
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        cache
            .insert("c.com".to_string(), create_test_server_config("c.com"))
            .await;

        // Access a.com to update its last_access time
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        cache.get("a.com").await;

        // Adding d.com should now evict b.com (oldest after a.com was accessed)
        cache
            .insert("d.com".to_string(), create_test_server_config("d.com"))
            .await;

        assert_eq!(cache.len().await, 3);
        assert!(cache.get("a.com").await.is_some());
        assert!(cache.get("b.com").await.is_none());
        assert!(cache.get("c.com").await.is_some());
        assert!(cache.get("d.com").await.is_some());
    }

    #[test]
    fn test_default_cache() {
        let cache = CertificateCache::default();
        assert_eq!(cache.max_size(), DEFAULT_CACHE_SIZE);
    }
}
