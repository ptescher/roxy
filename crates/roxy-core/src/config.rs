//! Configuration management for Roxy
//!
//! Stores configuration in OS-appropriate locations:
//! - macOS: ~/Library/Application Support/Roxy/config.toml
//! - Linux: ~/.config/roxy/config.toml
//! - Windows: %APPDATA%\Roxy\config.toml

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Roxy configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RoxyConfig {
    /// Datadog configuration
    #[serde(default)]
    pub datadog: DatadogConfig,

    /// ClickHouse configuration
    #[serde(default)]
    pub clickhouse: ClickHouseSettings,

    /// Proxy configuration
    #[serde(default)]
    pub proxy: ProxyConfig,

    /// TLS interception configuration
    #[serde(default)]
    pub tls: TlsSettings,
}

/// Datadog API configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatadogConfig {
    /// Datadog API key
    pub api_key: Option<String>,

    /// Datadog application key
    pub app_key: Option<String>,

    /// Datadog site (e.g., "datadoghq.com", "datadoghq.eu")
    #[serde(default = "default_dd_site")]
    pub site: String,
}

impl Default for DatadogConfig {
    fn default() -> Self {
        Self {
            api_key: None,
            app_key: None,
            site: default_dd_site(),
        }
    }
}

fn default_dd_site() -> String {
    "datadoghq.com".to_string()
}

/// ClickHouse configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClickHouseSettings {
    /// ClickHouse host
    #[serde(default = "default_ch_host")]
    pub host: String,

    /// ClickHouse port
    #[serde(default = "default_ch_port")]
    pub port: u16,

    /// ClickHouse database name
    #[serde(default = "default_ch_database")]
    pub database: String,

    /// ClickHouse username
    #[serde(default = "default_ch_username")]
    pub username: String,

    /// ClickHouse password
    pub password: Option<String>,
}

impl Default for ClickHouseSettings {
    fn default() -> Self {
        Self {
            host: default_ch_host(),
            port: default_ch_port(),
            database: default_ch_database(),
            username: default_ch_username(),
            password: None,
        }
    }
}

fn default_ch_host() -> String {
    "localhost".to_string()
}

fn default_ch_port() -> u16 {
    8123
}

fn default_ch_database() -> String {
    "roxy".to_string()
}

fn default_ch_username() -> String {
    "default".to_string()
}

/// Proxy configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyConfig {
    /// HTTP proxy port
    #[serde(default = "default_http_port")]
    pub http_port: u16,

    /// SOCKS5 proxy port
    #[serde(default = "default_socks_port")]
    pub socks_port: u16,

    /// Enable TLS interception
    #[serde(default = "default_true")]
    pub enable_tls_interception: bool,
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            http_port: default_http_port(),
            socks_port: default_socks_port(),
            enable_tls_interception: default_true(),
        }
    }
}

fn default_http_port() -> u16 {
    8080
}

fn default_socks_port() -> u16 {
    1080
}

fn default_true() -> bool {
    true
}

/// TLS interception configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TlsSettings {
    /// TLS interception mode: "Passthrough", "InterceptListed", or "InterceptAll"
    #[serde(default = "default_tls_mode")]
    pub mode: String,

    /// List of hosts to intercept TLS for (when mode is InterceptListed)
    /// or exclude from interception (when mode is InterceptAll)
    /// Supports exact matches and wildcard patterns like `*.example.com`
    #[serde(default)]
    pub intercept_hosts: Vec<String>,
}

impl Default for TlsSettings {
    fn default() -> Self {
        Self {
            mode: default_tls_mode(),
            intercept_hosts: Vec::new(),
        }
    }
}

fn default_tls_mode() -> String {
    "Passthrough".to_string()
}

impl RoxyConfig {
    /// Get the config file path for the current OS
    pub fn config_path() -> Result<PathBuf> {
        let config_dir = dirs::config_dir()
            .context("Could not determine config directory")?
            .join("roxy");

        fs::create_dir_all(&config_dir).context("Failed to create config directory")?;

        Ok(config_dir.join("config.toml"))
    }

    /// Load configuration from disk, creating default if it doesn't exist
    pub fn load() -> Result<Self> {
        let path = Self::config_path()?;

        if !path.exists() {
            // Create default config
            let config = Self::default();
            config.save()?;
            return Ok(config);
        }

        let contents = fs::read_to_string(&path).context("Failed to read config file")?;

        let config: Self = toml::from_str(&contents).context("Failed to parse config file")?;

        Ok(config)
    }

    /// Save configuration to disk
    pub fn save(&self) -> Result<()> {
        let path = Self::config_path()?;
        let contents = toml::to_string_pretty(self).context("Failed to serialize config")?;

        fs::write(&path, contents).context("Failed to write config file")?;

        Ok(())
    }

    /// Load config and merge with environment variables
    /// Environment variables take precedence over config file
    pub fn load_with_env() -> Result<Self> {
        let mut config = Self::load()?;

        // Override with environment variables if set
        if let Ok(api_key) = std::env::var("DD_API_KEY") {
            config.datadog.api_key = Some(api_key);
        }

        if let Ok(app_key) = std::env::var("DD_APP_KEY") {
            config.datadog.app_key = Some(app_key);
        }

        if let Ok(site) = std::env::var("DD_SITE") {
            config.datadog.site = site;
        }

        Ok(config)
    }

    /// Export Datadog credentials to environment variables
    pub fn export_to_env(&self) -> Result<()> {
        if let Some(ref api_key) = self.datadog.api_key {
            std::env::set_var("DD_API_KEY", api_key);
        }

        if let Some(ref app_key) = self.datadog.app_key {
            std::env::set_var("DD_APP_KEY", app_key);
        }

        std::env::set_var("DD_SITE", &self.datadog.site);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = RoxyConfig::default();
        assert_eq!(config.datadog.site, "datadoghq.com");
        assert_eq!(config.proxy.http_port, 8080);
        assert_eq!(config.proxy.socks_port, 1080);
        assert_eq!(config.tls.mode, "Passthrough");
        assert!(config.tls.intercept_hosts.is_empty());
    }

    #[test]
    fn test_config_serialization() {
        let config = RoxyConfig::default();
        let toml = toml::to_string_pretty(&config).unwrap();
        let parsed: RoxyConfig = toml::from_str(&toml).unwrap();
        assert_eq!(parsed.datadog.site, config.datadog.site);
        assert_eq!(parsed.tls.mode, config.tls.mode);
    }

    #[test]
    fn test_tls_config_with_hosts() {
        let toml_str = r#"
[tls]
mode = "InterceptListed"
intercept_hosts = ["*.example.org", "api.example.com"]
"#;
        let config: RoxyConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.tls.mode, "InterceptListed");
        assert_eq!(config.tls.intercept_hosts.len(), 2);
        assert!(config.tls.intercept_hosts.contains(&"*.example.org".to_string()));
    }
}
