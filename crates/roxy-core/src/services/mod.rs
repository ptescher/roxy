//! Service management for Roxy's backend dependencies
//!
//! This module handles downloading, configuring, and running ClickHouse
//! and OpenTelemetry Collector as managed subprocesses.

pub mod clickhouse;
pub mod downloader;
pub mod otel_collector;

use anyhow::Result;
use std::path::PathBuf;

/// Base directory for Roxy data and binaries
pub fn roxy_home() -> PathBuf {
    dirs::home_dir()
        .expect("Could not find home directory")
        .join(".roxy")
}

/// Directory for downloaded binaries
pub fn bin_dir() -> PathBuf {
    roxy_home().join("bin")
}

/// Directory for service data
pub fn data_dir() -> PathBuf {
    roxy_home().join("data")
}

/// Directory for service logs
pub fn logs_dir() -> PathBuf {
    roxy_home().join("logs")
}

/// Directory for service configs
pub fn config_dir() -> PathBuf {
    roxy_home().join("config")
}

/// Ensure all required directories exist
pub async fn ensure_directories() -> Result<()> {
    tokio::fs::create_dir_all(bin_dir()).await?;
    tokio::fs::create_dir_all(data_dir()).await?;
    tokio::fs::create_dir_all(logs_dir()).await?;
    tokio::fs::create_dir_all(config_dir()).await?;
    Ok(())
}

/// A managed service that runs as a subprocess
#[async_trait::async_trait]
pub trait ManagedService: Send + Sync {
    /// Name of the service
    fn name(&self) -> &'static str;

    /// Check if the service binary is installed
    async fn is_installed(&self) -> bool;

    /// Download and install the service binary
    async fn install(&self) -> Result<()>;

    /// Start the service
    async fn start(&mut self) -> Result<()>;

    /// Stop the service gracefully
    async fn stop(&mut self) -> Result<()>;

    /// Check if the service is healthy
    async fn health_check(&self) -> Result<bool>;

    /// Wait for the service to become healthy
    async fn wait_until_healthy(&self, timeout_secs: u64) -> Result<()> {
        let start = std::time::Instant::now();
        let timeout = std::time::Duration::from_secs(timeout_secs);

        while start.elapsed() < timeout {
            if self.health_check().await.unwrap_or(false) {
                tracing::info!("{} is healthy", self.name());
                return Ok(());
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }

        anyhow::bail!(
            "{} failed to become healthy within {}s",
            self.name(),
            timeout_secs
        )
    }
}

/// Manager for all backend services
pub struct ServiceManager {
    pub clickhouse: clickhouse::ClickHouseService,
    pub otel_collector: otel_collector::OtelCollectorService,
}

impl ServiceManager {
    /// Create a new service manager
    pub fn new() -> Self {
        Self {
            clickhouse: clickhouse::ClickHouseService::new(),
            otel_collector: otel_collector::OtelCollectorService::new(),
        }
    }

    /// Ensure all directories and configurations are set up
    pub async fn setup(&self) -> Result<()> {
        ensure_directories().await?;
        self.clickhouse.write_config().await?;
        self.otel_collector.write_config().await?;
        Ok(())
    }

    /// Install all required services if not present
    pub async fn install_all(&self) -> Result<()> {
        if !self.clickhouse.is_installed().await {
            tracing::info!("Installing ClickHouse...");
            self.clickhouse.install().await?;
        }

        if !self.otel_collector.is_installed().await {
            tracing::info!("Installing OpenTelemetry Collector...");
            self.otel_collector.install().await?;
        }

        Ok(())
    }

    /// Start all services
    pub async fn start_all(&mut self) -> Result<()> {
        tracing::info!("Starting ClickHouse...");
        self.clickhouse.start().await?;
        self.clickhouse.wait_until_healthy(30).await?;

        tracing::info!("Starting OpenTelemetry Collector...");
        self.otel_collector.start().await?;
        self.otel_collector.wait_until_healthy(10).await?;

        Ok(())
    }

    /// Stop all services
    pub async fn stop_all(&mut self) -> Result<()> {
        tracing::info!("Stopping services...");

        // Stop in reverse order
        let _ = self.otel_collector.stop().await;
        let _ = self.clickhouse.stop().await;

        Ok(())
    }
}

impl Default for ServiceManager {
    fn default() -> Self {
        Self::new()
    }
}
