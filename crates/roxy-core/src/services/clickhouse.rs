//! ClickHouse service management
//!
//! This module handles running ClickHouse as a managed subprocess.

use super::downloader::{DownloadManager, ServiceType};
use super::{config_dir, data_dir, logs_dir, ManagedService};
use anyhow::{Context, Result};
use std::path::PathBuf;
use std::process::Stdio;
use tokio::process::{Child, Command};
use tracing::{debug, error, info, warn};

/// Default ClickHouse HTTP port
pub const CLICKHOUSE_HTTP_PORT: u16 = 8123;

/// Default ClickHouse native TCP port
pub const CLICKHOUSE_TCP_PORT: u16 = 9000;

/// ClickHouse service configuration
#[derive(Debug, Clone)]
pub struct ClickHouseConfig {
    pub http_port: u16,
    pub tcp_port: u16,
    pub data_dir: PathBuf,
    pub log_dir: PathBuf,
    pub config_dir: PathBuf,
}

impl Default for ClickHouseConfig {
    fn default() -> Self {
        Self {
            http_port: CLICKHOUSE_HTTP_PORT,
            tcp_port: CLICKHOUSE_TCP_PORT,
            data_dir: data_dir().join("clickhouse"),
            log_dir: logs_dir(),
            config_dir: config_dir(),
        }
    }
}

/// ClickHouse managed service
pub struct ClickHouseService {
    config: ClickHouseConfig,
    downloader: DownloadManager,
    process: Option<Child>,
}

impl ClickHouseService {
    /// Create a new ClickHouse service with default configuration
    pub fn new() -> Self {
        Self {
            config: ClickHouseConfig::default(),
            downloader: DownloadManager::new().expect("Failed to create download manager"),
            process: None,
        }
    }

    /// Create a new ClickHouse service with custom configuration
    pub fn with_config(config: ClickHouseConfig) -> Result<Self> {
        Ok(Self {
            config,
            downloader: DownloadManager::new()?,
            process: None,
        })
    }

    /// Get the HTTP endpoint URL
    pub fn http_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.config.http_port)
    }

    /// Get the native TCP endpoint
    pub fn tcp_url(&self) -> String {
        format!("127.0.0.1:{}", self.config.tcp_port)
    }

    /// Write the ClickHouse configuration files
    pub async fn write_config(&self) -> Result<()> {
        let config_path = self.config.config_dir.join("clickhouse-config.xml");
        let users_path = self.config.config_dir.join("clickhouse-users.xml");

        tokio::fs::create_dir_all(&self.config.config_dir).await?;
        tokio::fs::create_dir_all(&self.config.data_dir).await?;
        tokio::fs::create_dir_all(&self.config.log_dir).await?;

        // Main configuration
        let config_content = format!(
            r#"<?xml version="1.0"?>
<clickhouse>
    <logger>
        <level>information</level>
        <log>{log_dir}/clickhouse.log</log>
        <errorlog>{log_dir}/clickhouse-error.log</errorlog>
        <size>100M</size>
        <count>3</count>
    </logger>

    <http_port>{http_port}</http_port>
    <tcp_port>{tcp_port}</tcp_port>

    <listen_host>127.0.0.1</listen_host>

    <path>{data_dir}/</path>
    <tmp_path>{data_dir}/tmp/</tmp_path>
    <user_files_path>{data_dir}/user_files/</user_files_path>
    <format_schema_path>{data_dir}/format_schemas/</format_schema_path>

    <mark_cache_size>5368709120</mark_cache_size>

    <max_concurrent_queries>100</max_concurrent_queries>
    <max_connections>4096</max_connections>

    <uncompressed_cache_size>8589934592</uncompressed_cache_size>

    <users_config>{users_path}</users_config>

    <default_profile>default</default_profile>
    <default_database>default</default_database>

    <mlock_executable>false</mlock_executable>

    <builtin_dictionaries_reload_interval>3600</builtin_dictionaries_reload_interval>

    <max_session_timeout>3600</max_session_timeout>
    <default_session_timeout>60</default_session_timeout>
</clickhouse>
"#,
            log_dir = self.config.log_dir.display(),
            http_port = self.config.http_port,
            tcp_port = self.config.tcp_port,
            data_dir = self.config.data_dir.display(),
            users_path = users_path.display(),
        );

        // Users configuration
        let users_content = r#"<?xml version="1.0"?>
<clickhouse>
    <profiles>
        <default>
            <max_memory_usage>10000000000</max_memory_usage>
            <use_uncompressed_cache>0</use_uncompressed_cache>
            <load_balancing>random</load_balancing>
        </default>
        <readonly>
            <readonly>1</readonly>
        </readonly>
    </profiles>

    <users>
        <default>
            <password></password>
            <networks>
                <ip>::1</ip>
                <ip>127.0.0.1</ip>
            </networks>
            <profile>default</profile>
            <quota>default</quota>
            <access_management>1</access_management>
        </default>
    </users>

    <quotas>
        <default>
            <interval>
                <duration>3600</duration>
                <queries>0</queries>
                <errors>0</errors>
                <result_rows>0</result_rows>
                <read_rows>0</read_rows>
                <execution_time>0</execution_time>
            </interval>
        </default>
    </quotas>
</clickhouse>
"#;

        tokio::fs::write(&config_path, config_content)
            .await
            .context("Failed to write ClickHouse config")?;

        tokio::fs::write(&users_path, users_content)
            .await
            .context("Failed to write ClickHouse users config")?;

        info!("Wrote ClickHouse configuration to {:?}", config_path);
        Ok(())
    }

    /// Get the path to the binary
    fn binary_path(&self) -> PathBuf {
        self.downloader.binary_path(ServiceType::ClickHouse)
    }

    /// Get the config file path
    fn config_path(&self) -> PathBuf {
        self.config.config_dir.join("clickhouse-config.xml")
    }
}

impl Default for ClickHouseService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl ManagedService for ClickHouseService {
    fn name(&self) -> &'static str {
        "ClickHouse"
    }

    async fn is_installed(&self) -> bool {
        self.downloader.binary_exists(ServiceType::ClickHouse)
    }

    async fn install(&self) -> Result<()> {
        self.downloader
            .ensure_binary(ServiceType::ClickHouse)
            .await?;
        self.write_config().await?;
        Ok(())
    }

    async fn start(&mut self) -> Result<()> {
        if self.process.is_some() {
            warn!("ClickHouse is already running");
            return Ok(());
        }

        let binary = self.binary_path();
        let config = self.config_path();

        if !binary.exists() {
            anyhow::bail!(
                "ClickHouse binary not found at {:?}. Run install first.",
                binary
            );
        }

        if !config.exists() {
            self.write_config().await?;
        }

        info!("Starting ClickHouse with config {:?}", config);

        let log_file = std::fs::File::create(self.config.log_dir.join("clickhouse-stdout.log"))
            .context("Failed to create stdout log file")?;

        let err_file = std::fs::File::create(self.config.log_dir.join("clickhouse-stderr.log"))
            .context("Failed to create stderr log file")?;

        let child = Command::new(&binary)
            .arg("server")
            .arg("--config-file")
            .arg(&config)
            .stdout(Stdio::from(log_file))
            .stderr(Stdio::from(err_file))
            .kill_on_drop(true)
            .spawn()
            .context("Failed to spawn ClickHouse process")?;

        info!("ClickHouse started with PID: {:?}", child.id());
        self.process = Some(child);

        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        if let Some(mut child) = self.process.take() {
            info!("Stopping ClickHouse...");

            // Try graceful shutdown first
            #[cfg(unix)]
            {
                use nix::sys::signal::{self, Signal};
                use nix::unistd::Pid;

                if let Some(pid) = child.id() {
                    let _ = signal::kill(Pid::from_raw(pid as i32), Signal::SIGTERM);
                }
            }

            // Wait a bit for graceful shutdown
            match tokio::time::timeout(std::time::Duration::from_secs(5), child.wait()).await {
                Ok(Ok(status)) => {
                    info!("ClickHouse exited with status: {}", status);
                }
                Ok(Err(e)) => {
                    error!("Error waiting for ClickHouse: {}", e);
                }
                Err(_) => {
                    warn!("ClickHouse didn't exit gracefully, killing...");
                    let _ = child.kill().await;
                }
            }
        }

        Ok(())
    }

    async fn health_check(&self) -> Result<bool> {
        let url = format!("{}/ping", self.http_url());

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(2))
            .build()?;

        match client.get(&url).send().await {
            Ok(response) => {
                let healthy = response.status().is_success();
                debug!("ClickHouse health check: {}", healthy);
                Ok(healthy)
            }
            Err(e) => {
                debug!("ClickHouse health check failed: {}", e);
                Ok(false)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = ClickHouseConfig::default();
        assert_eq!(config.http_port, 8123);
        assert_eq!(config.tcp_port, 9000);
    }

    #[test]
    fn test_http_url() {
        let service = ClickHouseService::new();
        assert_eq!(service.http_url(), "http://127.0.0.1:8123");
    }
}
