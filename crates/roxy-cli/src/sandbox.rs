use anyhow::{bail, Context, Result};
use clap::Args;
use std::net::TcpStream;
use std::path::PathBuf;
use std::time::Duration;

use crate::docker;

const DEFAULT_IMAGE_TAG: &str = "roxy-sandbox:latest";
const DEFAULT_PROXY_PORT: u16 = 8080;
const DEFAULT_SOCKS_PORT: u16 = 1080;

#[derive(Args, Clone, Debug)]
pub struct SandboxArgs {
    /// Docker image to use (default: build from sandbox/Dockerfile)
    #[arg(long)]
    pub image: Option<String>,

    /// Force rebuild the sandbox image
    #[arg(long)]
    pub build: bool,

    /// Enable first-class Claude Code support inside the container
    #[arg(long)]
    pub claude_code: bool,

    /// Additional volume mounts (-v host:container)
    #[arg(short = 'v', long = "volume")]
    pub volumes: Vec<String>,

    /// Additional environment variables (-e KEY=VALUE)
    #[arg(short = 'e', long = "env")]
    pub envs: Vec<String>,

    /// HTTP/HTTPS proxy port on the host
    #[arg(long, default_value_t = DEFAULT_PROXY_PORT)]
    pub proxy_port: u16,

    /// SOCKS5 proxy port on the host
    #[arg(long, default_value_t = DEFAULT_SOCKS_PORT)]
    pub socks_port: u16,

    /// Directory to mount as /workspace (default: current directory)
    #[arg(long)]
    pub workdir: Option<PathBuf>,

    /// Container name
    #[arg(long)]
    pub name: Option<String>,

    /// Command (and arguments) to run inside the container
    #[arg(last = true)]
    pub cmd: Vec<String>,
}

/// Top-level entry point called from main.
pub async fn run(args: SandboxArgs) -> Result<()> {
    let runner = SandboxRunner::new(args);
    runner.run().await
}

struct SandboxRunner {
    args: SandboxArgs,
}

impl SandboxRunner {
    fn new(args: SandboxArgs) -> Self {
        Self { args }
    }

    // ── Orchestration ──────────────────────────────────────────────

    async fn run(&self) -> Result<()> {
        docker::check_docker()?;
        self.ensure_proxy_running().await?;

        let image = self.ensure_image()?;
        let docker_args = self.build_docker_args(&image)?;

        tracing::info!("Starting sandbox container...");
        let status = docker::run_container(&docker_args)?;

        if !status.success() {
            let code = status.code().unwrap_or(1);
            // 130 = SIGINT (Ctrl+C), which is normal interactive exit
            if code != 130 {
                bail!("Container exited with code {code}");
            }
        }

        Ok(())
    }

    // ── Proxy health ───────────────────────────────────────────────

    async fn ensure_proxy_running(&self) -> Result<()> {
        if self.probe_proxy() {
            tracing::info!("Proxy already running on port {}", self.args.proxy_port);
            return Ok(());
        }

        tracing::info!(
            "Proxy not detected on port {}. Starting...",
            self.args.proxy_port
        );
        self.start_proxy().await?;

        // Wait up to 15 seconds for the proxy to become reachable.
        let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
        while tokio::time::Instant::now() < deadline {
            if self.probe_proxy() {
                tracing::info!("Proxy is ready.");
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }

        bail!(
            "Proxy did not become reachable on port {} within 15 seconds",
            self.args.proxy_port
        );
    }

    fn probe_proxy(&self) -> bool {
        TcpStream::connect_timeout(
            &format!("127.0.0.1:{}", self.args.proxy_port)
                .parse()
                .unwrap(),
            Duration::from_millis(500),
        )
        .is_ok()
    }

    async fn start_proxy(&self) -> Result<()> {
        use roxy_core::proxy_manager::{ProxyManager, ProxyManagerConfig};

        let config = ProxyManagerConfig {
            proxy_port: self.args.proxy_port,
            configure_system_proxy: false,
            auto_restart: false,
            ..Default::default()
        };

        // Start the proxy in the background. We intentionally leak the
        // manager here so the subprocess outlives the sandbox command —
        // the proxy should keep running while the container is alive.
        let manager = Box::leak(Box::new(ProxyManager::new(config)));
        manager.start().await.context("failed to start proxy")?;

        Ok(())
    }

    // ── Docker image ───────────────────────────────────────────────

    fn ensure_image(&self) -> Result<String> {
        if let Some(ref image) = self.args.image {
            if !self.args.build {
                return Ok(image.clone());
            }
        }

        let tag = self
            .args
            .image
            .clone()
            .unwrap_or_else(|| DEFAULT_IMAGE_TAG.to_string());

        let needs_build = self.args.build || !docker::image_exists(&tag).unwrap_or(false);

        if needs_build {
            let project_root = self.find_project_root()?;
            let dockerfile = project_root.join("sandbox").join("Dockerfile");
            let context = project_root.join("sandbox");

            if !dockerfile.exists() {
                bail!(
                    "sandbox/Dockerfile not found at {}. Run from the roxy project root.",
                    dockerfile.display()
                );
            }

            docker::build_image(&context, &dockerfile, &tag)?;
        }

        Ok(tag)
    }

    // ── Docker run arguments ───────────────────────────────────────

    fn build_docker_args(&self, image: &str) -> Result<Vec<String>> {
        let mut args: Vec<String> = Vec::new();

        // Interactive + auto-remove
        args.extend(["-it".into(), "--rm".into()]);

        // Container name
        if let Some(ref name) = self.args.name {
            args.extend(["--name".into(), name.clone()]);
        }

        // Workspace mount
        let workdir = self.resolve_workdir()?;
        args.extend([
            "-v".into(),
            format!("{}:/workspace", workdir.display()),
            "-w".into(),
            "/workspace".into(),
        ]);

        // Kube config (read-only)
        let home = home_dir()?;
        let kubeconfig = home.join(".kube");
        if kubeconfig.exists() {
            args.extend([
                "-v".into(),
                format!("{}:/root/.kube:ro", kubeconfig.display()),
            ]);
        }

        // CA certificate (read-only)
        let ca_cert = home.join(".roxy").join("ca.crt");
        if ca_cert.exists() {
            args.extend([
                "-v".into(),
                format!(
                    "{}:/usr/local/share/ca-certificates/roxy-ca.crt:ro",
                    ca_cert.display()
                ),
            ]);
        }

        // Platform-specific networking
        let proxy_host = self.proxy_host(&mut args);

        // Proxy environment variables
        let http_proxy = format!("http://{}:{}", proxy_host, self.args.proxy_port);
        let socks_proxy = format!("socks5://{}:{}", proxy_host, self.args.socks_port);
        let no_proxy = "localhost,127.0.0.1,::1";

        for key in &["HTTP_PROXY", "http_proxy"] {
            args.extend(["-e".into(), format!("{key}={http_proxy}")]);
        }
        for key in &["HTTPS_PROXY", "https_proxy"] {
            args.extend(["-e".into(), format!("{key}={http_proxy}")]);
        }
        for key in &["ALL_PROXY", "all_proxy"] {
            args.extend(["-e".into(), format!("{key}={socks_proxy}")]);
        }
        for key in &["NO_PROXY", "no_proxy"] {
            args.extend(["-e".into(), format!("{key}={no_proxy}")]);
        }

        // CA trust env vars so common runtimes trust the Roxy CA
        let ca_path = "/etc/ssl/certs/roxy-ca.pem";
        args.extend(["-e".into(), format!("NODE_EXTRA_CA_CERTS={ca_path}")]);
        args.extend(["-e".into(), format!("SSL_CERT_FILE={ca_path}")]);
        args.extend(["-e".into(), format!("REQUESTS_CA_BUNDLE={ca_path}")]);
        args.extend(["-e".into(), format!("CURL_CA_BUNDLE={ca_path}")]);

        // Claude Code support
        if self.args.claude_code {
            args.extend(["-e".into(), "ROXY_CLAUDE_CODE=true".into()]);

            let claude_dir = home.join(".claude");
            if claude_dir.exists() {
                args.extend([
                    "-v".into(),
                    format!("{}:/root/.claude", claude_dir.display()),
                ]);
            }

            // Forward API key if set
            if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
                args.extend(["-e".into(), format!("ANTHROPIC_API_KEY={key}")]);
            }
        }

        // User-supplied volumes
        for vol in &self.args.volumes {
            args.extend(["-v".into(), vol.clone()]);
        }

        // User-supplied env vars
        for env in &self.args.envs {
            args.extend(["-e".into(), env.clone()]);
        }

        // Image
        args.push(image.to_string());

        // Command
        if !self.args.cmd.is_empty() {
            args.extend(self.args.cmd.clone());
        }

        Ok(args)
    }

    /// Determine the proxy host and, on Linux, add `--network=host`.
    /// Returns the hostname/IP the container should use to reach the proxy.
    fn proxy_host(&self, args: &mut Vec<String>) -> String {
        if cfg!(target_os = "linux") {
            args.extend(["--network=host".into()]);
            "127.0.0.1".into()
        } else {
            // macOS / Windows Docker Desktop
            "host.docker.internal".into()
        }
    }

    // ── Helpers ─────────────────────────────────────────────────────

    fn resolve_workdir(&self) -> Result<PathBuf> {
        match &self.args.workdir {
            Some(p) => {
                let p = p.canonicalize().context("workdir does not exist")?;
                Ok(p)
            }
            None => std::env::current_dir().context("could not determine current directory"),
        }
    }

    fn find_project_root(&self) -> Result<PathBuf> {
        // Walk up from the current exe or cwd to find Cargo.toml with [workspace]
        let start = std::env::current_dir()?;
        let mut dir = start.as_path();
        loop {
            let candidate = dir.join("sandbox").join("Dockerfile");
            if candidate.exists() {
                return Ok(dir.to_path_buf());
            }
            match dir.parent() {
                Some(parent) => dir = parent,
                None => bail!(
                    "Could not find project root (sandbox/Dockerfile) from {}",
                    start.display()
                ),
            }
        }
    }
}

fn home_dir() -> Result<PathBuf> {
    dirs::home_dir().context("could not determine home directory")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_args() -> SandboxArgs {
        SandboxArgs {
            image: Some("test-image:latest".into()),
            build: false,
            claude_code: false,
            volumes: vec![],
            envs: vec![],
            proxy_port: 8080,
            socks_port: 1080,
            workdir: None,
            name: None,
            cmd: vec![],
        }
    }

    #[test]
    fn test_build_docker_args_basic() {
        let args = default_args();
        let runner = SandboxRunner::new(args);
        let docker_args = runner.build_docker_args("test-image:latest").unwrap();

        // Should start with -it --rm
        assert_eq!(&docker_args[0], "-it");
        assert_eq!(&docker_args[1], "--rm");

        // Should contain workspace volume
        assert!(docker_args.iter().any(|a| a.contains(":/workspace")));

        // Should contain proxy env vars
        assert!(docker_args.iter().any(|a| a.starts_with("HTTP_PROXY=")));
        assert!(docker_args.iter().any(|a| a.starts_with("HTTPS_PROXY=")));
        assert!(docker_args.iter().any(|a| a.starts_with("ALL_PROXY=")));
        assert!(docker_args.iter().any(|a| a.starts_with("NO_PROXY=")));

        // Should contain CA env vars
        assert!(docker_args
            .iter()
            .any(|a| a.starts_with("NODE_EXTRA_CA_CERTS=")));

        // Image should be last (before any cmd)
        assert_eq!(docker_args.last().unwrap(), "test-image:latest");
    }

    #[test]
    fn test_build_docker_args_with_name() {
        let mut args = default_args();
        args.name = Some("my-sandbox".into());
        let runner = SandboxRunner::new(args);
        let docker_args = runner.build_docker_args("img").unwrap();

        let name_idx = docker_args.iter().position(|a| a == "--name").unwrap();
        assert_eq!(docker_args[name_idx + 1], "my-sandbox");
    }

    #[test]
    fn test_build_docker_args_with_claude_code() {
        let mut args = default_args();
        args.claude_code = true;
        let runner = SandboxRunner::new(args);
        let docker_args = runner.build_docker_args("img").unwrap();

        assert!(docker_args.iter().any(|a| a == "ROXY_CLAUDE_CODE=true"));
    }

    #[test]
    fn test_build_docker_args_with_cmd() {
        let mut args = default_args();
        args.cmd = vec!["bash".into(), "-c".into(), "echo hello".into()];
        let runner = SandboxRunner::new(args);
        let docker_args = runner.build_docker_args("img").unwrap();

        // Image followed by command
        let img_idx = docker_args.iter().rposition(|a| a == "img").unwrap();
        assert_eq!(docker_args[img_idx + 1], "bash");
        assert_eq!(docker_args[img_idx + 2], "-c");
        assert_eq!(docker_args[img_idx + 3], "echo hello");
    }

    #[test]
    fn test_build_docker_args_extra_volumes_and_envs() {
        let mut args = default_args();
        args.volumes = vec!["/tmp/data:/data".into()];
        args.envs = vec!["MY_VAR=hello".into()];
        let runner = SandboxRunner::new(args);
        let docker_args = runner.build_docker_args("img").unwrap();

        assert!(docker_args.iter().any(|a| a == "/tmp/data:/data"));
        assert!(docker_args.iter().any(|a| a == "MY_VAR=hello"));
    }

    #[test]
    fn test_proxy_host_macos() {
        // On macOS (where tests run), should use host.docker.internal
        let args = default_args();
        let runner = SandboxRunner::new(args);
        let mut v = Vec::new();
        let host = runner.proxy_host(&mut v);

        if cfg!(target_os = "macos") {
            assert_eq!(host, "host.docker.internal");
            assert!(v.is_empty()); // no --network=host on mac
        }
    }
}
