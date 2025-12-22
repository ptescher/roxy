//! Download manager for fetching service binaries
//!
//! This module handles downloading ClickHouse and OpenTelemetry Collector
//! binaries on first run, caching them in ~/.roxy/bin/

use anyhow::{bail, Context, Result};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use tokio::process::Command;
use tracing::{debug, info, warn};

/// Supported platforms for binary downloads
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    MacOsAarch64,
    MacOsX86_64,
    LinuxAarch64,
    LinuxX86_64,
}

impl Platform {
    /// Detect the current platform
    pub fn current() -> Result<Self> {
        let os = std::env::consts::OS;
        let arch = std::env::consts::ARCH;

        match (os, arch) {
            ("macos", "aarch64") => Ok(Platform::MacOsAarch64),
            ("macos", "x86_64") => Ok(Platform::MacOsX86_64),
            ("linux", "aarch64") => Ok(Platform::LinuxAarch64),
            ("linux", "x86_64") => Ok(Platform::LinuxX86_64),
            _ => bail!("Unsupported platform: {}-{}", os, arch),
        }
    }

    fn clickhouse_url(&self) -> &'static str {
        // ClickHouse provides static binaries
        match self {
            Platform::MacOsAarch64 => "https://github.com/ClickHouse/ClickHouse/releases/download/v25.9.7.56-stable/clickhouse-macos-aarch64",
            Platform::MacOsX86_64 => "https://github.com/ClickHouse/ClickHouse/releases/download/v25.9.7.56-stable/clickhouse-macos",
            Platform::LinuxAarch64 => "https://github.com/ClickHouse/ClickHouse/releases/download/v25.9.7.56-stable/clickhouse-linux-aarch64",
            Platform::LinuxX86_64 => "https://github.com/ClickHouse/ClickHouse/releases/download/v25.9.7.56-stable/clickhouse-linux-amd64",
        }
    }

    fn otel_collector_url(&self) -> &'static str {
        // OpenTelemetry Collector contrib releases
        match self {
            Platform::MacOsAarch64 => "https://github.com/open-telemetry/opentelemetry-collector-releases/releases/download/v0.96.0/otelcol-contrib_0.96.0_darwin_arm64.tar.gz",
            Platform::MacOsX86_64 => "https://github.com/open-telemetry/opentelemetry-collector-releases/releases/download/v0.96.0/otelcol-contrib_0.96.0_darwin_amd64.tar.gz",
            Platform::LinuxAarch64 => "https://github.com/open-telemetry/opentelemetry-collector-releases/releases/download/v0.96.0/otelcol-contrib_0.96.0_linux_arm64.tar.gz",
            Platform::LinuxX86_64 => "https://github.com/open-telemetry/opentelemetry-collector-releases/releases/download/v0.96.0/otelcol-contrib_0.96.0_linux_amd64.tar.gz",
        }
    }
}

/// Service type that can be downloaded
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceType {
    ClickHouse,
    OtelCollector,
}

impl ServiceType {
    pub fn binary_name(&self) -> &'static str {
        match self {
            ServiceType::ClickHouse => "clickhouse",
            ServiceType::OtelCollector => "otelcol-contrib",
        }
    }

    fn download_url(&self, platform: Platform) -> &'static str {
        match self {
            ServiceType::ClickHouse => platform.clickhouse_url(),
            ServiceType::OtelCollector => platform.otel_collector_url(),
        }
    }

    fn is_archive(&self) -> bool {
        match self {
            ServiceType::ClickHouse => false,   // Direct binary
            ServiceType::OtelCollector => true, // tar.gz archive
        }
    }
}

/// Download manager configuration
#[derive(Debug, Clone)]
pub struct DownloadManager {
    /// Base directory for Roxy data (~/.roxy)
    pub base_dir: PathBuf,
    /// Binary directory
    pub bin_dir: PathBuf,
    /// Detected platform
    pub platform: Platform,
}

impl DownloadManager {
    /// Create a new download manager with default paths
    pub fn new() -> Result<Self> {
        let base_dir = dirs::home_dir()
            .context("Could not determine home directory")?
            .join(".roxy");

        let bin_dir = base_dir.join("bin");
        let platform = Platform::current()?;

        Ok(Self {
            base_dir,
            bin_dir,
            platform,
        })
    }

    /// Create a download manager with a custom base directory
    pub fn with_base_dir(base_dir: PathBuf) -> Result<Self> {
        let bin_dir = base_dir.join("bin");
        let platform = Platform::current()?;

        Ok(Self {
            base_dir,
            bin_dir,
            platform,
        })
    }

    /// Ensure all directories exist
    pub fn ensure_dirs(&self) -> Result<()> {
        fs::create_dir_all(&self.base_dir).context("Failed to create base directory")?;
        fs::create_dir_all(&self.bin_dir).context("Failed to create bin directory")?;

        // Also create data directories
        fs::create_dir_all(self.base_dir.join("data/clickhouse"))
            .context("Failed to create ClickHouse data directory")?;
        fs::create_dir_all(self.base_dir.join("data/otel"))
            .context("Failed to create OTel data directory")?;
        fs::create_dir_all(self.base_dir.join("config"))
            .context("Failed to create config directory")?;
        fs::create_dir_all(self.base_dir.join("logs"))
            .context("Failed to create logs directory")?;

        Ok(())
    }

    /// Get the path to a service binary
    pub fn binary_path(&self, service: ServiceType) -> PathBuf {
        self.bin_dir.join(service.binary_name())
    }

    /// Check if a service binary exists
    pub fn binary_exists(&self, service: ServiceType) -> bool {
        let path = self.binary_path(service);
        path.exists() && path.is_file()
    }

    /// Ensure a service binary is available, downloading if necessary
    pub async fn ensure_binary(&self, service: ServiceType) -> Result<PathBuf> {
        let binary_path = self.binary_path(service);

        if self.binary_exists(service) {
            debug!("{:?} binary already exists at {:?}", service, binary_path);
            return Ok(binary_path);
        }

        info!("Downloading {:?} binary...", service);
        self.download_binary(service).await?;

        Ok(binary_path)
    }

    /// Download a service binary
    async fn download_binary(&self, service: ServiceType) -> Result<()> {
        self.ensure_dirs()?;

        let url = service.download_url(self.platform);
        let binary_path = self.binary_path(service);

        info!("Downloading from: {}", url);

        // Use reqwest for async download
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(600)) // 10 min timeout for large files
            .build()
            .context("Failed to create HTTP client")?;

        let response = client
            .get(url)
            .send()
            .await
            .context("Failed to start download")?;

        if !response.status().is_success() {
            bail!("Download failed with status: {}", response.status());
        }

        let total_size = response.content_length();

        if service.is_archive() {
            // Download to temp file, then extract
            let temp_path = self
                .bin_dir
                .join(format!("{}.tar.gz", service.binary_name()));
            self.download_to_file(response, &temp_path, total_size)
                .await?;
            self.extract_tar_gz(&temp_path, &self.bin_dir, service)
                .await?;
            fs::remove_file(&temp_path).ok(); // Clean up archive
        } else {
            // Direct binary download
            self.download_to_file(response, &binary_path, total_size)
                .await?;
        }

        // Make binary executable
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&binary_path)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&binary_path, perms)?;
        }

        info!("Successfully downloaded {:?} to {:?}", service, binary_path);
        Ok(())
    }

    async fn download_to_file(
        &self,
        response: reqwest::Response,
        path: &Path,
        total_size: Option<u64>,
    ) -> Result<()> {
        let mut file = fs::File::create(path).context("Failed to create output file")?;

        let mut downloaded: u64 = 0;
        let mut stream = response.bytes_stream();

        use futures::StreamExt;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("Error reading download stream")?;
            file.write_all(&chunk).context("Error writing to file")?;

            downloaded += chunk.len() as u64;

            if let Some(total) = total_size {
                let percent = (downloaded as f64 / total as f64) * 100.0;
                if downloaded % (1024 * 1024) < chunk.len() as u64 {
                    info!(
                        "Download progress: {:.1}% ({} MB / {} MB)",
                        percent,
                        downloaded / (1024 * 1024),
                        total / (1024 * 1024)
                    );
                }
            }
        }

        file.flush()?;
        Ok(())
    }

    async fn extract_tar_gz(
        &self,
        archive_path: &Path,
        dest_dir: &Path,
        service: ServiceType,
    ) -> Result<()> {
        info!("Extracting archive...");

        // Use tar command for extraction (more reliable than pure Rust for complex archives)
        let output = Command::new("tar")
            .args(["-xzf", archive_path.to_str().unwrap()])
            .current_dir(dest_dir)
            .output()
            .await
            .context("Failed to run tar command")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("Failed to extract archive: {}", stderr);
        }

        // OTel collector extracts as otelcol-contrib, which is what we want
        let expected_binary = dest_dir.join(service.binary_name());
        if !expected_binary.exists() {
            // Try to find the binary
            warn!(
                "Expected binary not found at {:?}, searching...",
                expected_binary
            );
        }

        Ok(())
    }

    /// Get the data directory for a service
    pub fn data_dir(&self, service: ServiceType) -> PathBuf {
        match service {
            ServiceType::ClickHouse => self.base_dir.join("data/clickhouse"),
            ServiceType::OtelCollector => self.base_dir.join("data/otel"),
        }
    }

    /// Get the log directory
    pub fn log_dir(&self) -> PathBuf {
        self.base_dir.join("logs")
    }

    /// Get the config directory
    pub fn config_dir(&self) -> PathBuf {
        self.base_dir.join("config")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_detection() {
        // Should not panic on supported platforms
        let result = Platform::current();
        assert!(
            result.is_ok()
                || cfg!(not(any(
                    all(target_os = "macos", target_arch = "aarch64"),
                    all(target_os = "macos", target_arch = "x86_64"),
                    all(target_os = "linux", target_arch = "aarch64"),
                    all(target_os = "linux", target_arch = "x86_64"),
                )))
        );
    }

    #[test]
    fn test_binary_names() {
        assert_eq!(ServiceType::ClickHouse.binary_name(), "clickhouse");
        assert_eq!(ServiceType::OtelCollector.binary_name(), "otelcol-contrib");
    }
}
