//! OpenTelemetry Collector service management
//!
//! This module handles running the OpenTelemetry Collector as a managed subprocess.

use super::downloader::{DownloadManager, ServiceType};
use super::{config_dir, logs_dir, ManagedService};
use anyhow::{Context, Result};
use std::path::PathBuf;
use std::process::Stdio;
use tokio::process::{Child, Command};
use tracing::{debug, error, info, warn};

/// Default OTLP gRPC port
pub const OTEL_GRPC_PORT: u16 = 4317;

/// Default OTLP HTTP port
pub const OTEL_HTTP_PORT: u16 = 4318;

/// Default health check port
pub const OTEL_HEALTH_PORT: u16 = 13133;

/// OpenTelemetry Collector service configuration
#[derive(Debug, Clone)]
pub struct OtelCollectorConfig {
    pub grpc_port: u16,
    pub http_port: u16,
    pub health_port: u16,
    pub clickhouse_endpoint: String,
    pub config_dir: PathBuf,
    pub log_dir: PathBuf,
}

impl Default for OtelCollectorConfig {
    fn default() -> Self {
        Self {
            grpc_port: OTEL_GRPC_PORT,
            http_port: OTEL_HTTP_PORT,
            health_port: OTEL_HEALTH_PORT,
            clickhouse_endpoint: "tcp://127.0.0.1:9000".to_string(),
            config_dir: config_dir(),
            log_dir: logs_dir(),
        }
    }
}

/// OpenTelemetry Collector managed service
pub struct OtelCollectorService {
    config: OtelCollectorConfig,
    downloader: DownloadManager,
    process: Option<Child>,
}

impl OtelCollectorService {
    /// Create a new OTel Collector service with default configuration
    pub fn new() -> Self {
        Self {
            config: OtelCollectorConfig::default(),
            downloader: DownloadManager::new().expect("Failed to create download manager"),
            process: None,
        }
    }

    /// Create a new OTel Collector service with custom configuration
    pub fn with_config(config: OtelCollectorConfig) -> Result<Self> {
        Ok(Self {
            config,
            downloader: DownloadManager::new()?,
            process: None,
        })
    }

    /// Get the OTLP gRPC endpoint URL
    pub fn grpc_endpoint(&self) -> String {
        format!("http://127.0.0.1:{}", self.config.grpc_port)
    }

    /// Get the OTLP HTTP endpoint URL
    pub fn http_endpoint(&self) -> String {
        format!("http://127.0.0.1:{}", self.config.http_port)
    }

    /// Get the health check endpoint URL
    pub fn health_endpoint(&self) -> String {
        format!("http://127.0.0.1:{}", self.config.health_port)
    }

    /// Write the OpenTelemetry Collector configuration file
    pub async fn write_config(&self) -> Result<()> {
        let config_path = self.config.config_dir.join("otel-collector.yaml");

        tokio::fs::create_dir_all(&self.config.config_dir).await?;
        tokio::fs::create_dir_all(&self.config.log_dir).await?;

        let config_content = format!(
            r#"# OpenTelemetry Collector configuration for Roxy
# Auto-generated - modifications may be overwritten

receivers:
  otlp:
    protocols:
      grpc:
        endpoint: "0.0.0.0:{grpc_port}"
      http:
        endpoint: "0.0.0.0:{http_port}"

processors:
  batch:
    timeout: 1s
    send_batch_size: 1024
    send_batch_max_size: 2048

  memory_limiter:
    check_interval: 1s
    limit_mib: 256
    spike_limit_mib: 64

  resource:
    attributes:
      - key: deployment.environment
        value: local
        action: upsert

exporters:
  # ClickHouse exporter for traces
  clickhouse:
    endpoint: "{clickhouse_endpoint}"
    database: roxy
    username: default
    password: ""
    ttl: 168h
    traces_table_name: otel_traces
    logs_table_name: otel_logs
    metrics_table_name: otel_metrics
    timeout: 10s
    retry_on_failure:
      enabled: true
      initial_interval: 5s
      max_interval: 30s
      max_elapsed_time: 300s

  # Debug exporter for development (logs to console)
  debug:
    verbosity: basic
    sampling_initial: 5
    sampling_thereafter: 200

extensions:
  health_check:
    endpoint: "0.0.0.0:{health_port}"

  zpages:
    endpoint: "0.0.0.0:55679"

service:
  extensions: [health_check, zpages]

  pipelines:
    traces:
      receivers: [otlp]
      processors: [memory_limiter, batch, resource]
      exporters: [clickhouse, debug]

    metrics:
      receivers: [otlp]
      processors: [memory_limiter, batch]
      exporters: [clickhouse]

    logs:
      receivers: [otlp]
      processors: [memory_limiter, batch]
      exporters: [clickhouse]

  telemetry:
    logs:
      level: info
    metrics:
      address: "0.0.0.0:8888"
"#,
            grpc_port = self.config.grpc_port,
            http_port = self.config.http_port,
            health_port = self.config.health_port,
            clickhouse_endpoint = self.config.clickhouse_endpoint,
        );

        tokio::fs::write(&config_path, config_content)
            .await
            .context("Failed to write OTel Collector config")?;

        info!(
            "Wrote OpenTelemetry Collector configuration to {:?}",
            config_path
        );
        Ok(())
    }

    /// Get the path to the binary
    fn binary_path(&self) -> PathBuf {
        self.downloader.binary_path(ServiceType::OtelCollector)
    }

    /// Get the config file path
    fn config_path(&self) -> PathBuf {
        self.config.config_dir.join("otel-collector.yaml")
    }
}

impl Default for OtelCollectorService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl ManagedService for OtelCollectorService {
    fn name(&self) -> &'static str {
        "OpenTelemetry Collector"
    }

    async fn is_installed(&self) -> bool {
        self.downloader.binary_exists(ServiceType::OtelCollector)
    }

    async fn install(&self) -> Result<()> {
        self.downloader
            .ensure_binary(ServiceType::OtelCollector)
            .await?;
        self.write_config().await?;
        Ok(())
    }

    async fn start(&mut self) -> Result<()> {
        if self.process.is_some() {
            warn!("OpenTelemetry Collector is already running");
            return Ok(());
        }

        let binary = self.binary_path();
        let config = self.config_path();

        if !binary.exists() {
            anyhow::bail!(
                "OTel Collector binary not found at {:?}. Run install first.",
                binary
            );
        }

        if !config.exists() {
            self.write_config().await?;
        }

        info!("Starting OpenTelemetry Collector with config {:?}", config);

        let log_file = std::fs::File::create(self.config.log_dir.join("otel-collector-stdout.log"))
            .context("Failed to create stdout log file")?;

        let err_file = std::fs::File::create(self.config.log_dir.join("otel-collector-stderr.log"))
            .context("Failed to create stderr log file")?;

        let child = Command::new(&binary)
            .arg("--config")
            .arg(&config)
            .stdout(Stdio::from(log_file))
            .stderr(Stdio::from(err_file))
            .kill_on_drop(true)
            .spawn()
            .context("Failed to spawn OTel Collector process")?;

        info!("OpenTelemetry Collector started with PID: {:?}", child.id());
        self.process = Some(child);

        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        if let Some(mut child) = self.process.take() {
            info!("Stopping OpenTelemetry Collector...");

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
                    info!("OpenTelemetry Collector exited with status: {}", status);
                }
                Ok(Err(e)) => {
                    error!("Error waiting for OTel Collector: {}", e);
                }
                Err(_) => {
                    warn!("OTel Collector didn't exit gracefully, killing...");
                    let _ = child.kill().await;
                }
            }
        }

        Ok(())
    }

    async fn health_check(&self) -> Result<bool> {
        let url = self.health_endpoint();

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(2))
            .build()?;

        match client.get(&url).send().await {
            Ok(response) => {
                let healthy = response.status().is_success();
                debug!("OTel Collector health check: {}", healthy);
                Ok(healthy)
            }
            Err(e) => {
                debug!("OTel Collector health check failed: {}", e);
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
        let config = OtelCollectorConfig::default();
        assert_eq!(config.grpc_port, 4317);
        assert_eq!(config.http_port, 4318);
        assert_eq!(config.health_port, 13133);
    }

    #[test]
    fn test_endpoints() {
        let service = OtelCollectorService::new();
        assert_eq!(service.grpc_endpoint(), "http://127.0.0.1:4317");
        assert_eq!(service.http_endpoint(), "http://127.0.0.1:4318");
        assert_eq!(service.health_endpoint(), "http://127.0.0.1:13133");
    }
}
