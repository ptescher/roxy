//! Auto-update system for Roxy
//!
//! This module handles checking for updates from GitHub releases and
//! downloading new versions of Roxy components.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

/// GitHub repository for Roxy releases
const GITHUB_REPO: &str = "ptescher/roxy";

/// Current version of Roxy (from Cargo.toml)
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Component that can be updated
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Component {
    /// The main Roxy UI application
    Ui,
    /// The proxy backend worker
    Proxy,
    /// ClickHouse database
    ClickHouse,
    /// OpenTelemetry Collector
    OtelCollector,
}

impl Component {
    /// Get the binary name for this component
    pub fn binary_name(&self) -> &'static str {
        match self {
            Component::Ui => "roxy",
            Component::Proxy => "roxy-proxy",
            Component::ClickHouse => "clickhouse",
            Component::OtelCollector => "otelcol-contrib",
        }
    }

    /// Check if this is a first-party Roxy component
    pub fn is_roxy_component(&self) -> bool {
        matches!(self, Component::Ui | Component::Proxy)
    }
}

/// Semantic version
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
    pub prerelease: Option<String>,
}

impl Version {
    /// Parse a version string like "1.2.3" or "v1.2.3" or "1.2.3-beta.1"
    pub fn parse(s: &str) -> Result<Self> {
        let s = s.trim().trim_start_matches('v');

        let (version_part, prerelease) = if let Some(idx) = s.find('-') {
            (&s[..idx], Some(s[idx + 1..].to_string()))
        } else {
            (s, None)
        };

        let parts: Vec<&str> = version_part.split('.').collect();
        if parts.len() < 2 || parts.len() > 3 {
            bail!("Invalid version format: {}", s);
        }

        let major = parts[0].parse().context("Invalid major version")?;
        let minor = parts[1].parse().context("Invalid minor version")?;
        let patch = parts
            .get(2)
            .map(|p| p.parse())
            .transpose()
            .context("Invalid patch version")?
            .unwrap_or(0);

        Ok(Self {
            major,
            minor,
            patch,
            prerelease,
        })
    }

    /// Get the current version of Roxy
    pub fn current() -> Self {
        Self::parse(CURRENT_VERSION).expect("Invalid CARGO_PKG_VERSION")
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if let Some(ref pre) = self.prerelease {
            write!(f, "-{}", pre)?;
        }
        Ok(())
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.major.cmp(&other.major) {
            Ordering::Equal => {}
            ord => return ord,
        }
        match self.minor.cmp(&other.minor) {
            Ordering::Equal => {}
            ord => return ord,
        }
        match self.patch.cmp(&other.patch) {
            Ordering::Equal => {}
            ord => return ord,
        }

        // Prerelease versions are less than release versions
        match (&self.prerelease, &other.prerelease) {
            (None, None) => Ordering::Equal,
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (Some(a), Some(b)) => a.cmp(b),
        }
    }
}

/// Information about an available update
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateInfo {
    /// Version of the update
    pub version: Version,
    /// Release notes / changelog
    pub release_notes: String,
    /// Download URL for the update
    pub download_url: String,
    /// Size of the download in bytes
    pub download_size: Option<u64>,
    /// SHA256 checksum of the download
    pub checksum: Option<String>,
    /// Whether this is a mandatory update
    pub mandatory: bool,
    /// Release date
    pub published_at: String,
}

/// GitHub release asset
#[derive(Debug, Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
    size: u64,
}

/// GitHub release response
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct GitHubRelease {
    tag_name: String,
    name: String,
    body: Option<String>,
    draft: bool,
    prerelease: bool,
    published_at: String,
    assets: Vec<GitHubAsset>,
}

/// Update channel
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UpdateChannel {
    /// Stable releases only
    #[default]
    Stable,
    /// Include beta/preview releases
    Beta,
    /// Include all prereleases
    Nightly,
}

/// Configuration for the updater
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdaterConfig {
    /// Whether automatic update checks are enabled
    pub enabled: bool,
    /// Update channel to use
    pub channel: UpdateChannel,
    /// How often to check for updates (in hours)
    pub check_interval_hours: u32,
    /// GitHub repository (owner/repo format)
    pub github_repo: String,
    /// Directory to store downloaded updates
    pub download_dir: PathBuf,
}

impl Default for UpdaterConfig {
    fn default() -> Self {
        let download_dir = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".roxy")
            .join("updates");

        Self {
            enabled: true,
            channel: UpdateChannel::Stable,
            check_interval_hours: 24,
            github_repo: GITHUB_REPO.to_string(),
            download_dir,
        }
    }
}

/// Updater client for checking and downloading updates
pub struct Updater {
    config: UpdaterConfig,
    http_client: reqwest::Client,
}

impl Updater {
    /// Create a new updater with the given configuration
    pub fn new(config: UpdaterConfig) -> Result<Self> {
        let http_client = reqwest::Client::builder()
            .user_agent(format!("roxy/{}", CURRENT_VERSION))
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .context("Failed to create HTTP client")?;

        Ok(Self {
            config,
            http_client,
        })
    }

    /// Create a new updater with default configuration
    pub fn with_defaults() -> Result<Self> {
        Self::new(UpdaterConfig::default())
    }

    /// Check for available updates
    pub async fn check_for_updates(&self) -> Result<Option<UpdateInfo>> {
        if !self.config.enabled {
            debug!("Update checks are disabled");
            return Ok(None);
        }

        let current_version = Version::current();
        info!(
            "Checking for updates (current version: {})",
            current_version
        );

        let releases = self.fetch_releases().await?;

        for release in releases {
            // Skip drafts
            if release.draft {
                continue;
            }

            // Filter by channel
            if release.prerelease && self.config.channel == UpdateChannel::Stable {
                continue;
            }

            let release_version = match Version::parse(&release.tag_name) {
                Ok(v) => v,
                Err(e) => {
                    debug!(
                        "Skipping release with invalid version: {} - {}",
                        release.tag_name, e
                    );
                    continue;
                }
            };

            // Check if this is a newer version
            if release_version > current_version {
                // Find the appropriate asset for this platform
                if let Some(asset) = self.find_platform_asset(&release.assets) {
                    info!(
                        "Update available: {} -> {}",
                        current_version, release_version
                    );

                    return Ok(Some(UpdateInfo {
                        version: release_version,
                        release_notes: release.body.unwrap_or_default(),
                        download_url: asset.browser_download_url.clone(),
                        download_size: Some(asset.size),
                        checksum: None, // Would need to fetch separately or include in release
                        mandatory: false,
                        published_at: release.published_at,
                    }));
                }
            }
        }

        info!("No updates available");
        Ok(None)
    }

    /// Fetch releases from GitHub
    async fn fetch_releases(&self) -> Result<Vec<GitHubRelease>> {
        let url = format!(
            "https://api.github.com/repos/{}/releases",
            self.config.github_repo
        );

        let response = self
            .http_client
            .get(&url)
            .header("Accept", "application/vnd.github.v3+json")
            .send()
            .await
            .context("Failed to fetch releases from GitHub")?;

        if !response.status().is_success() {
            bail!(
                "GitHub API returned error: {} {}",
                response.status(),
                response.text().await.unwrap_or_default()
            );
        }

        let releases: Vec<GitHubRelease> = response
            .json::<Vec<GitHubRelease>>()
            .await
            .context("Failed to parse GitHub releases")?;

        Ok(releases)
    }

    /// Find the appropriate asset for the current platform
    fn find_platform_asset<'a>(&self, assets: &'a [GitHubAsset]) -> Option<&'a GitHubAsset> {
        let os = std::env::consts::OS;
        let arch = std::env::consts::ARCH;

        // Map to common naming conventions
        let os_pattern = match os {
            "macos" => "darwin",
            "linux" => "linux",
            "windows" => "windows",
            _ => os,
        };

        let arch_pattern = match arch {
            "aarch64" => "arm64",
            "x86_64" => "amd64",
            _ => arch,
        };

        // Look for matching asset
        for asset in assets {
            let name_lower = asset.name.to_lowercase();

            // Check if it matches our platform
            let os_match =
                name_lower.contains(os_pattern) || (os == "macos" && name_lower.contains("macos"));
            let arch_match = name_lower.contains(arch_pattern)
                || (arch == "aarch64" && name_lower.contains("aarch64"))
                || (arch == "x86_64" && name_lower.contains("x86_64"));

            if os_match && arch_match {
                return Some(asset);
            }
        }

        // Fallback: try to find any asset that might work
        for asset in assets {
            let name_lower = asset.name.to_lowercase();
            if name_lower.contains(os_pattern) {
                warn!(
                    "Using potentially mismatched asset for {}-{}: {}",
                    os, arch, asset.name
                );
                return Some(asset);
            }
        }

        None
    }

    /// Download an update to a temporary location
    pub async fn download_update(&self, update: &UpdateInfo) -> Result<PathBuf> {
        info!(
            "Downloading update {} from {}",
            update.version, update.download_url
        );

        // Ensure download directory exists
        tokio::fs::create_dir_all(&self.config.download_dir)
            .await
            .context("Failed to create download directory")?;

        let file_name = update
            .download_url
            .rsplit('/')
            .next()
            .unwrap_or("roxy-update");

        let download_path = self.config.download_dir.join(file_name);

        let response = self
            .http_client
            .get(&update.download_url)
            .send()
            .await
            .context("Failed to start download")?;

        if !response.status().is_success() {
            bail!("Download failed with status: {}", response.status());
        }

        let total_size = response.content_length();
        let mut downloaded: u64 = 0;

        let mut file = tokio::fs::File::create(&download_path)
            .await
            .context("Failed to create download file")?;

        use futures::StreamExt;
        use tokio::io::AsyncWriteExt;

        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("Error reading download stream")?;
            file.write_all(&chunk)
                .await
                .context("Error writing to file")?;

            downloaded += chunk.len() as u64;

            if let Some(total) = total_size {
                let percent = (downloaded as f64 / total as f64) * 100.0;
                if downloaded % (1024 * 1024) < chunk.len() as u64 {
                    info!("Download progress: {:.1}%", percent);
                }
            }
        }

        file.flush().await?;

        info!("Download complete: {:?}", download_path);

        // Verify checksum if provided
        if let Some(ref expected_checksum) = update.checksum {
            let actual_checksum = self.compute_sha256(&download_path).await?;
            if &actual_checksum != expected_checksum {
                bail!(
                    "Checksum mismatch: expected {}, got {}",
                    expected_checksum,
                    actual_checksum
                );
            }
            info!("Checksum verified");
        }

        Ok(download_path)
    }

    /// Compute SHA256 checksum of a file
    async fn compute_sha256(&self, path: &Path) -> Result<String> {
        use sha2::{Digest, Sha256};

        let content = tokio::fs::read(path)
            .await
            .context("Failed to read file for checksum")?;

        let mut hasher = Sha256::new();
        hasher.update(&content);
        let result = hasher.finalize();

        Ok(format!("{:x}", result))
    }

    /// Apply a downloaded update
    ///
    /// This replaces the current binary with the downloaded one.
    /// For the UI, this typically requires a restart.
    /// For the proxy (when managed as a subprocess), we can restart it automatically.
    pub async fn apply_update(&self, download_path: &Path, component: Component) -> Result<()> {
        info!(
            "Applying update for {:?} from {:?}",
            component, download_path
        );

        // Determine the target path
        let target_path = if component.is_roxy_component() {
            // For Roxy components, get the current executable path
            std::env::current_exe().context("Failed to get current executable path")?
        } else {
            // For third-party components, use the bin directory
            dirs::home_dir()
                .context("Failed to get home directory")?
                .join(".roxy")
                .join("bin")
                .join(component.binary_name())
        };

        // Create backup of current binary
        let backup_path = target_path.with_extension("backup");
        if target_path.exists() {
            tokio::fs::copy(&target_path, &backup_path)
                .await
                .context("Failed to create backup")?;
        }

        // Extract if it's an archive, otherwise copy directly
        let file_name = download_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");

        if file_name.ends_with(".tar.gz") || file_name.ends_with(".tgz") {
            self.extract_tar_gz(download_path, target_path.parent().unwrap())
                .await?;
        } else if file_name.ends_with(".zip") {
            self.extract_zip(download_path, target_path.parent().unwrap())
                .await?;
        } else {
            // Direct binary copy
            tokio::fs::copy(download_path, &target_path)
                .await
                .context("Failed to copy update")?;
        }

        // Make executable on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = tokio::fs::metadata(&target_path).await?.permissions();
            perms.set_mode(0o755);
            tokio::fs::set_permissions(&target_path, perms).await?;
        }

        // Clean up download
        let _ = tokio::fs::remove_file(download_path).await;

        info!("Update applied successfully");
        Ok(())
    }

    async fn extract_tar_gz(&self, archive: &Path, dest: &Path) -> Result<()> {
        use tokio::process::Command;

        let output = Command::new("tar")
            .args(["-xzf", archive.to_str().unwrap()])
            .current_dir(dest)
            .output()
            .await
            .context("Failed to run tar")?;

        if !output.status.success() {
            bail!(
                "tar extraction failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        Ok(())
    }

    async fn extract_zip(&self, archive: &Path, dest: &Path) -> Result<()> {
        use tokio::process::Command;

        let output = Command::new("unzip")
            .args([
                "-o",
                archive.to_str().unwrap(),
                "-d",
                dest.to_str().unwrap(),
            ])
            .output()
            .await
            .context("Failed to run unzip")?;

        if !output.status.success() {
            bail!(
                "unzip extraction failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        Ok(())
    }
}

/// State for tracking update status
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UpdateState {
    /// Last time we checked for updates
    pub last_check: Option<String>,
    /// Available update (if any)
    pub available_update: Option<UpdateInfo>,
    /// Whether an update is currently downloading
    pub downloading: bool,
    /// Download progress (0-100)
    pub download_progress: u8,
    /// Error message from last operation
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_parse() {
        let v = Version::parse("1.2.3").unwrap();
        assert_eq!(v.major, 1);
        assert_eq!(v.minor, 2);
        assert_eq!(v.patch, 3);
        assert_eq!(v.prerelease, None);

        let v = Version::parse("v1.2.3").unwrap();
        assert_eq!(v.major, 1);

        let v = Version::parse("1.2.3-beta.1").unwrap();
        assert_eq!(v.prerelease, Some("beta.1".to_string()));

        let v = Version::parse("1.2").unwrap();
        assert_eq!(v.patch, 0);
    }

    #[test]
    fn test_version_comparison() {
        let v1 = Version::parse("1.0.0").unwrap();
        let v2 = Version::parse("1.0.1").unwrap();
        let v3 = Version::parse("1.1.0").unwrap();
        let v4 = Version::parse("2.0.0").unwrap();
        let v5 = Version::parse("1.0.0-beta").unwrap();

        assert!(v1 < v2);
        assert!(v2 < v3);
        assert!(v3 < v4);
        assert!(v5 < v1); // Prerelease is less than release
    }

    #[test]
    fn test_version_display() {
        let v = Version::parse("1.2.3").unwrap();
        assert_eq!(v.to_string(), "1.2.3");

        let v = Version::parse("1.2.3-beta.1").unwrap();
        assert_eq!(v.to_string(), "1.2.3-beta.1");
    }

    #[test]
    fn test_current_version() {
        let v = Version::current();
        // Just verify it parses without panicking
        let _ = v.major;
    }
}
