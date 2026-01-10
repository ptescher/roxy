//! TLS Configuration for Roxy Proxy
//!
//! Defines configuration options for TLS interception, including which hosts
//! should have their TLS connections intercepted vs. tunneled through.

use std::collections::HashSet;

/// Default path for CA certificate (relative to data directory)
pub const DEFAULT_CA_CERT_PATH: &str = "~/.roxy/ca.crt";

/// Default path for CA private key (relative to data directory)
pub const DEFAULT_CA_KEY_PATH: &str = "~/.roxy/ca.key";

/// Default certificate cache size
pub const DEFAULT_CACHE_SIZE: usize = 1000;

/// TLS interception mode for HTTPS connections
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TlsInterceptionMode {
    /// Tunnel all HTTPS traffic through without interception (default)
    ///
    /// In this mode, CONNECT requests result in a raw TCP tunnel and
    /// the proxy cannot see the decrypted traffic.
    #[default]
    Passthrough,

    /// Intercept TLS for hosts in the intercept list only
    ///
    /// Hosts not in the list are tunneled through transparently.
    InterceptListed,

    /// Intercept all TLS traffic except hosts in an exclude list
    ///
    /// Use with caution - requires trusting the CA on all clients.
    InterceptAll,
}

/// Configuration for TLS handling in the proxy
#[derive(Debug, Clone)]
pub struct TlsConfig {
    /// Path to the CA certificate file
    pub ca_cert_path: String,

    /// Path to the CA private key file
    pub ca_key_path: String,

    /// Size of the certificate cache (number of host certificates to cache)
    pub cache_size: usize,

    /// TLS interception mode
    pub mode: TlsInterceptionMode,

    /// List of hosts to intercept TLS for (when mode is InterceptListed)
    /// or exclude from interception (when mode is InterceptAll)
    ///
    /// Supports exact matches and wildcard patterns like `*.example.com`
    pub intercept_hosts: Vec<String>,

    /// Whether TLS interception is enabled at all
    ///
    /// When false, all HTTPS connections are tunneled through regardless
    /// of other settings.
    pub enabled: bool,
}

impl Default for TlsConfig {
    fn default() -> Self {
        // Expand home directory
        let home = dirs::home_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "~".to_string());

        Self {
            ca_cert_path: DEFAULT_CA_CERT_PATH.replace('~', &home),
            ca_key_path: DEFAULT_CA_KEY_PATH.replace('~', &home),
            cache_size: DEFAULT_CACHE_SIZE,
            mode: TlsInterceptionMode::Passthrough,
            intercept_hosts: Vec::new(),
            enabled: false,
        }
    }
}

impl TlsConfig {
    /// Create a new TLS config with interception enabled for specific hosts
    pub fn with_intercept_hosts(hosts: Vec<String>) -> Self {
        Self {
            mode: TlsInterceptionMode::InterceptListed,
            intercept_hosts: hosts,
            enabled: true,
            ..Default::default()
        }
    }

    /// Create a config that intercepts all hosts
    pub fn intercept_all() -> Self {
        Self {
            mode: TlsInterceptionMode::InterceptAll,
            enabled: true,
            ..Default::default()
        }
    }

    /// Check if TLS should be intercepted for the given host
    pub fn should_intercept(&self, host: &str) -> bool {
        if !self.enabled {
            return false;
        }

        match self.mode {
            TlsInterceptionMode::Passthrough => false,
            TlsInterceptionMode::InterceptListed => self.host_matches_list(host),
            TlsInterceptionMode::InterceptAll => !self.host_matches_list(host),
        }
    }

    /// Check if a host matches any pattern in the intercept_hosts list
    fn host_matches_list(&self, host: &str) -> bool {
        let host_lower = host.to_lowercase();

        for pattern in &self.intercept_hosts {
            let pattern_lower = pattern.to_lowercase();

            if pattern_lower.starts_with("*.") {
                // Wildcard pattern - matches the domain and any subdomain
                let suffix = &pattern_lower[1..]; // Keep the dot
                if host_lower.ends_with(suffix) || host_lower == pattern_lower[2..] {
                    return true;
                }
            } else if host_lower == pattern_lower {
                // Exact match
                return true;
            }
        }

        false
    }

    /// Add a host to the interception list
    pub fn add_intercept_host(&mut self, host: String) {
        if !self.intercept_hosts.contains(&host) {
            self.intercept_hosts.push(host);
        }
    }

    /// Remove a host from the interception list
    pub fn remove_intercept_host(&mut self, host: &str) {
        self.intercept_hosts.retain(|h| h != host);
    }

    /// Set the interception hosts, replacing any existing list
    pub fn set_intercept_hosts(&mut self, hosts: Vec<String>) {
        self.intercept_hosts = hosts;
    }

    /// Get unique hosts from the interception list (no duplicates)
    pub fn unique_intercept_hosts(&self) -> HashSet<String> {
        self.intercept_hosts.iter().cloned().collect()
    }

    /// Enable TLS interception with the current settings
    pub fn enable(&mut self) {
        self.enabled = true;
        if self.mode == TlsInterceptionMode::Passthrough && !self.intercept_hosts.is_empty() {
            self.mode = TlsInterceptionMode::InterceptListed;
        }
    }

    /// Disable TLS interception (passthrough all)
    pub fn disable(&mut self) {
        self.enabled = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = TlsConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.mode, TlsInterceptionMode::Passthrough);
        assert!(config.intercept_hosts.is_empty());
    }

    #[test]
    fn test_passthrough_mode() {
        let config = TlsConfig::default();
        assert!(!config.should_intercept("example.com"));
        assert!(!config.should_intercept("anything.com"));
    }

    #[test]
    fn test_intercept_listed_mode() {
        let mut config = TlsConfig::with_intercept_hosts(vec![
            "example.com".to_string(),
            "*.api.example.com".to_string(),
        ]);

        // Should intercept listed hosts
        assert!(config.should_intercept("example.com"));
        assert!(config.should_intercept("EXAMPLE.COM")); // case insensitive

        // Should intercept wildcard matches
        assert!(config.should_intercept("api.example.com"));
        assert!(config.should_intercept("v1.api.example.com"));
        assert!(config.should_intercept("deep.v1.api.example.com"));

        // Should NOT intercept other hosts
        assert!(!config.should_intercept("other.com"));
        assert!(!config.should_intercept("sub.example.com")); // not in list

        // Test disabling
        config.disable();
        assert!(!config.should_intercept("example.com"));
    }

    #[test]
    fn test_intercept_all_mode() {
        let mut config = TlsConfig::intercept_all();
        config.intercept_hosts = vec!["internal.corp".to_string()];

        // Should intercept all hosts except excluded
        assert!(config.should_intercept("example.com"));
        assert!(config.should_intercept("anything.com"));

        // Should NOT intercept excluded hosts
        assert!(!config.should_intercept("internal.corp"));
    }

    #[test]
    fn test_add_remove_host() {
        let mut config = TlsConfig::default();
        config.mode = TlsInterceptionMode::InterceptListed;
        config.enabled = true;

        assert!(!config.should_intercept("example.com"));

        config.add_intercept_host("example.com".to_string());
        assert!(config.should_intercept("example.com"));

        // Adding duplicate should not create duplicate
        config.add_intercept_host("example.com".to_string());
        assert_eq!(config.intercept_hosts.len(), 1);

        config.remove_intercept_host("example.com");
        assert!(!config.should_intercept("example.com"));
    }

    #[test]
    fn test_wildcard_matching() {
        let config = TlsConfig::with_intercept_hosts(vec!["*.example.com".to_string()]);

        // Should match subdomains
        assert!(config.should_intercept("api.example.com"));
        assert!(config.should_intercept("www.example.com"));
        assert!(config.should_intercept("deep.sub.example.com"));

        // Should also match the base domain itself
        assert!(config.should_intercept("example.com"));

        // Should NOT match similar domains
        assert!(!config.should_intercept("notexample.com"));
        assert!(!config.should_intercept("example.com.evil.com"));
    }

    #[test]
    fn test_enable_with_hosts() {
        let mut config = TlsConfig::default();
        config.add_intercept_host("example.com".to_string());

        // Initially disabled
        assert!(!config.should_intercept("example.com"));

        // Enable should switch mode if hosts are configured
        config.enable();
        assert!(config.enabled);
        assert_eq!(config.mode, TlsInterceptionMode::InterceptListed);
        assert!(config.should_intercept("example.com"));
    }

    #[test]
    fn test_unique_hosts() {
        let mut config = TlsConfig::default();
        config.intercept_hosts = vec![
            "a.com".to_string(),
            "b.com".to_string(),
            "a.com".to_string(), // duplicate
        ];

        let unique = config.unique_intercept_hosts();
        assert_eq!(unique.len(), 2);
        assert!(unique.contains("a.com"));
        assert!(unique.contains("b.com"));
    }
}
