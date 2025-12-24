//! Proxy subprocess manager for the Roxy UI
//!
//! This module handles spawning, monitoring, and communicating with
//! the roxy-proxy backend process.

use anyhow::{bail, Context, Result};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{broadcast, watch, RwLock};
use tracing::{error, info, warn};

/// Default proxy port
pub const DEFAULT_PROXY_PORT: u16 = 8080;

/// Status of the proxy process
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProxyStatus {
    /// Proxy is not running
    Stopped,
    /// Proxy is starting up
    Starting,
    /// Proxy is running and healthy
    Running,
    /// Proxy is shutting down
    Stopping,
    /// Proxy crashed or exited unexpectedly
    Failed(String),
}

impl Default for ProxyStatus {
    fn default() -> Self {
        Self::Stopped
    }
}

/// Configuration for the proxy manager
#[derive(Debug, Clone)]
pub struct ProxyManagerConfig {
    /// Path to the roxy-proxy binary
    pub binary_path: Option<PathBuf>,
    /// Port for the proxy to listen on
    pub proxy_port: u16,
    /// Whether to configure system proxy on startup
    pub configure_system_proxy: bool,
    /// Whether the proxy should start its own backend services
    pub start_services: bool,
    /// Additional environment variables
    pub env_vars: Vec<(String, String)>,
    /// Restart on crash
    pub auto_restart: bool,
    /// Maximum restart attempts
    pub max_restart_attempts: u32,
}

impl Default for ProxyManagerConfig {
    fn default() -> Self {
        Self {
            binary_path: None,
            proxy_port: DEFAULT_PROXY_PORT,
            configure_system_proxy: false,
            start_services: true,
            env_vars: Vec::new(),
            auto_restart: true,
            max_restart_attempts: 3,
        }
    }
}

/// Log entry from the proxy process
#[derive(Debug, Clone)]
pub struct ProxyLogEntry {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub level: LogLevel,
    pub message: String,
}

/// Log level
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    fn from_str(s: &str) -> Self {
        match s.to_uppercase().as_str() {
            "TRACE" => Self::Trace,
            "DEBUG" => Self::Debug,
            "INFO" => Self::Info,
            "WARN" | "WARNING" => Self::Warn,
            "ERROR" => Self::Error,
            _ => Self::Info,
        }
    }
}

/// Manager for the roxy-proxy subprocess
pub struct ProxyManager {
    config: ProxyManagerConfig,
    process: Arc<RwLock<Option<Child>>>,
    status_tx: watch::Sender<ProxyStatus>,
    status_rx: watch::Receiver<ProxyStatus>,
    log_tx: broadcast::Sender<ProxyLogEntry>,
    restart_count: Arc<RwLock<u32>>,
}

impl ProxyManager {
    /// Create a new proxy manager with the given configuration
    pub fn new(config: ProxyManagerConfig) -> Self {
        let (status_tx, status_rx) = watch::channel(ProxyStatus::Stopped);
        let (log_tx, _) = broadcast::channel(1000); // Buffer last 1000 log entries

        Self {
            config,
            process: Arc::new(RwLock::new(None)),
            status_tx,
            status_rx,
            log_tx,
            restart_count: Arc::new(RwLock::new(0)),
        }
    }

    /// Create a new proxy manager with default configuration
    pub fn with_defaults() -> Self {
        Self::new(ProxyManagerConfig::default())
    }

    /// Get the current proxy status
    pub fn status(&self) -> ProxyStatus {
        self.status_rx.borrow().clone()
    }

    /// Subscribe to status changes
    pub fn subscribe_status(&self) -> watch::Receiver<ProxyStatus> {
        self.status_rx.clone()
    }

    /// Subscribe to log entries
    pub fn subscribe_logs(&self) -> broadcast::Receiver<ProxyLogEntry> {
        self.log_tx.subscribe()
    }

    /// Find the roxy-proxy binary
    fn find_binary(&self) -> Result<PathBuf> {
        // If explicitly configured, use that
        if let Some(ref path) = self.config.binary_path {
            if path.exists() {
                return Ok(path.clone());
            }
            bail!("Configured proxy binary not found: {:?}", path);
        }

        // Try to find alongside the current executable
        if let Ok(current_exe) = std::env::current_exe() {
            let sibling = current_exe.parent().unwrap().join("roxy-proxy");
            if sibling.exists() {
                return Ok(sibling);
            }

            // Try with .exe extension on Windows
            #[cfg(windows)]
            {
                let sibling_exe = current_exe.parent().unwrap().join("roxy-proxy.exe");
                if sibling_exe.exists() {
                    return Ok(sibling_exe);
                }
            }
        }

        // Try ~/.roxy/bin
        if let Some(home) = dirs::home_dir() {
            let roxy_bin = home.join(".roxy").join("bin").join("roxy-proxy");
            if roxy_bin.exists() {
                return Ok(roxy_bin);
            }
        }

        // Try PATH
        if let Ok(output) = std::process::Command::new("which")
            .arg("roxy-proxy")
            .output()
        {
            if output.status.success() {
                let path = String::from_utf8_lossy(&output.stdout);
                let path = PathBuf::from(path.trim());
                if path.exists() {
                    return Ok(path);
                }
            }
        }

        bail!("Could not find roxy-proxy binary. Please ensure it is installed.")
    }

    /// Start the proxy process
    pub async fn start(&self) -> Result<()> {
        // Check if already running
        {
            let process = self.process.read().await;
            if process.is_some() {
                warn!("Proxy is already running");
                return Ok(());
            }
        }

        let binary = self.find_binary()?;
        info!("Starting proxy from: {:?}", binary);

        self.status_tx.send(ProxyStatus::Starting).ok();

        // Build command
        let mut cmd = Command::new(&binary);

        // Add environment variables
        cmd.env("RUST_LOG", "info,roxy_proxy=debug,roxy_core=debug");
        cmd.env("ROXY_PROXY_PORT", self.config.proxy_port.to_string());
        cmd.env(
            "ROXY_CONFIGURE_SYSTEM_PROXY",
            if self.config.configure_system_proxy {
                "true"
            } else {
                "false"
            },
        );
        cmd.env(
            "ROXY_START_SERVICES",
            if self.config.start_services {
                "true"
            } else {
                "false"
            },
        );

        for (key, value) in &self.config.env_vars {
            cmd.env(key, value);
        }

        // Capture stdout and stderr
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        cmd.kill_on_drop(true);

        // Spawn the process
        let mut child = cmd.spawn().context("Failed to spawn proxy process")?;
        let pid = child.id();
        info!("Proxy started with PID: {:?}", pid);

        // Set up log streaming from stdout
        if let Some(stdout) = child.stdout.take() {
            let log_tx = self.log_tx.clone();
            tokio::spawn(async move {
                let reader = BufReader::new(stdout);
                let mut lines = reader.lines();

                while let Ok(Some(line)) = lines.next_line().await {
                    let entry = parse_log_line(&line);
                    let _ = log_tx.send(entry);
                }
            });
        }

        // Set up log streaming from stderr
        if let Some(stderr) = child.stderr.take() {
            let log_tx = self.log_tx.clone();
            tokio::spawn(async move {
                let reader = BufReader::new(stderr);
                let mut lines = reader.lines();

                while let Ok(Some(line)) = lines.next_line().await {
                    let entry = ProxyLogEntry {
                        timestamp: chrono::Utc::now(),
                        level: LogLevel::Error,
                        message: line,
                    };
                    let _ = log_tx.send(entry);
                }
            });
        }

        // Store the process
        {
            let mut process = self.process.write().await;
            *process = Some(child);
        }

        // Monitor the process in a background task
        let process = self.process.clone();
        let status_tx = self.status_tx.clone();
        let auto_restart = self.config.auto_restart;
        let max_restarts = self.config.max_restart_attempts;
        let restart_count = self.restart_count.clone();

        tokio::spawn(async move {
            loop {
                // Wait for process to exit
                let exit_status = {
                    let mut guard = process.write().await;
                    if let Some(ref mut child) = *guard {
                        child.wait().await
                    } else {
                        break;
                    }
                };

                match exit_status {
                    Ok(status) if status.success() => {
                        info!("Proxy exited successfully");
                        status_tx.send(ProxyStatus::Stopped).ok();
                        break;
                    }
                    Ok(status) => {
                        let code = status.code().unwrap_or(-1);
                        error!("Proxy exited with code: {}", code);

                        if auto_restart {
                            let count = {
                                let mut c = restart_count.write().await;
                                *c += 1;
                                *c
                            };

                            if count <= max_restarts {
                                warn!("Restarting proxy (attempt {}/{})", count, max_restarts);
                                status_tx.send(ProxyStatus::Starting).ok();
                                // TODO: Actually restart here
                                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                            } else {
                                status_tx
                                    .send(ProxyStatus::Failed(format!(
                                        "Crashed {} times, giving up",
                                        count
                                    )))
                                    .ok();
                                break;
                            }
                        } else {
                            status_tx
                                .send(ProxyStatus::Failed(format!("Exit code: {}", code)))
                                .ok();
                            break;
                        }
                    }
                    Err(e) => {
                        error!("Error waiting for proxy: {}", e);
                        status_tx.send(ProxyStatus::Failed(e.to_string())).ok();
                        break;
                    }
                }
            }

            // Clear the process handle
            let mut guard = process.write().await;
            *guard = None;
        });

        // Wait a moment and check if still running
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        let is_running = {
            let guard = self.process.read().await;
            guard.is_some()
        };

        if is_running {
            self.status_tx.send(ProxyStatus::Running).ok();
            // Reset restart count on successful start
            *self.restart_count.write().await = 0;
            Ok(())
        } else {
            bail!("Proxy failed to start")
        }
    }

    /// Stop the proxy process
    pub async fn stop(&self) -> Result<()> {
        self.status_tx.send(ProxyStatus::Stopping).ok();

        let mut process = self.process.write().await;

        if let Some(mut child) = process.take() {
            info!("Stopping proxy (PID: {:?})...", child.id());

            // Try graceful shutdown first
            #[cfg(unix)]
            {
                use nix::sys::signal::{self, Signal};
                use nix::unistd::Pid;

                if let Some(pid) = child.id() {
                    let _ = signal::kill(Pid::from_raw(pid as i32), Signal::SIGTERM);
                }
            }

            // Wait for graceful shutdown
            match tokio::time::timeout(std::time::Duration::from_secs(10), child.wait()).await {
                Ok(Ok(status)) => {
                    info!("Proxy stopped with status: {}", status);
                }
                Ok(Err(e)) => {
                    error!("Error waiting for proxy: {}", e);
                }
                Err(_) => {
                    warn!("Proxy didn't stop gracefully, killing...");
                    let _ = child.kill().await;
                }
            }
        }

        self.status_tx.send(ProxyStatus::Stopped).ok();
        Ok(())
    }

    /// Restart the proxy process
    pub async fn restart(&self) -> Result<()> {
        self.stop().await?;
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        self.start().await
    }

    /// Check if the proxy is running
    pub async fn is_running(&self) -> bool {
        let process = self.process.read().await;
        process.is_some()
    }

    /// Get the proxy port
    pub fn proxy_port(&self) -> u16 {
        self.config.proxy_port
    }

    /// Get the proxy URL
    pub fn proxy_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.config.proxy_port)
    }
}

impl Drop for ProxyManager {
    fn drop(&mut self) {
        // Try to stop the process synchronously on drop
        // This is best-effort since we can't use async in drop
        if let Ok(mut process) = self.process.try_write() {
            if let Some(mut child) = process.take() {
                let _ = child.start_kill();
            }
        }
    }
}

/// Parse a log line from the proxy
fn parse_log_line(line: &str) -> ProxyLogEntry {
    // Try to parse structured log format: "2024-01-01T00:00:00.000Z INFO message"
    let parts: Vec<&str> = line.splitn(3, ' ').collect();

    if parts.len() >= 3 {
        if let Ok(timestamp) = chrono::DateTime::parse_from_rfc3339(parts[0]) {
            return ProxyLogEntry {
                timestamp: timestamp.with_timezone(&chrono::Utc),
                level: LogLevel::from_str(parts[1]),
                message: parts[2..].join(" "),
            };
        }
    }

    // Fallback: treat the whole line as the message
    ProxyLogEntry {
        timestamp: chrono::Utc::now(),
        level: LogLevel::Info,
        message: line.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_level_from_str() {
        assert_eq!(LogLevel::from_str("INFO"), LogLevel::Info);
        assert_eq!(LogLevel::from_str("info"), LogLevel::Info);
        assert_eq!(LogLevel::from_str("ERROR"), LogLevel::Error);
        assert_eq!(LogLevel::from_str("WARN"), LogLevel::Warn);
        assert_eq!(LogLevel::from_str("WARNING"), LogLevel::Warn);
    }

    #[test]
    fn test_proxy_url() {
        let manager = ProxyManager::with_defaults();
        assert_eq!(manager.proxy_url(), "http://127.0.0.1:8080");
    }

    #[test]
    fn test_default_config() {
        let config = ProxyManagerConfig::default();
        assert_eq!(config.proxy_port, 8080);
        assert!(!config.configure_system_proxy);
        assert!(config.start_services);
        assert!(config.auto_restart);
    }
}
