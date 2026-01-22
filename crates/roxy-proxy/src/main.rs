//! Roxy Proxy - HTTP/HTTPS proxy with OpenTelemetry instrumentation
//!
//! This is the standalone binary entry point. For embedding the proxy
//! in other applications, use the library directly.

use anyhow::Result;
use clap::Parser;
use roxy_proxy::{run_proxy, tls::TlsConfig, ProxyConfig};
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// Roxy Proxy - HTTP/HTTPS proxy with selective TLS interception
#[derive(Parser, Debug)]
#[command(name = "roxy-proxy", version, about = "HTTP/HTTPS proxy with selective TLS interception")]
struct Args {
    /// Port to listen on
    #[arg(short, long, default_value = "8080")]
    port: u16,

    /// Domains to intercept TLS for (comma-separated, supports wildcards like *.example.com)
    /// Other domains will be forwarded without interception
    #[arg(long, value_delimiter = ',')]
    intercept_domains: Option<Vec<String>>,

    /// Enable system proxy (routes all macOS traffic through this proxy)
    #[arg(long)]
    system_proxy: bool,

    /// Disable backend services (ClickHouse, OTel)
    #[arg(long)]
    no_services: bool,

    /// Disable SOCKS5 proxy
    #[arg(long)]
    no_socks: bool,

    /// Disable MCP server
    #[arg(long)]
    no_mcp: bool,
}

/// Initialize logging with tracing-subscriber
fn init_logging() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,roxy_proxy=debug,roxy_core=debug,hudsucker=info"));

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_target(true))
        .init();
}

#[tokio::main]
async fn main() -> Result<()> {
    init_logging();

    let args = Args::parse();

    tracing::info!("Starting Roxy Proxy v{}", env!("CARGO_PKG_VERSION"));

    // Configure TLS interception
    let tls_config = if let Some(domains) = args.intercept_domains {
        tracing::info!(
            "TLS interception enabled for domains: {}",
            domains.join(", ")
        );
        TlsConfig::with_intercept_hosts(domains)
    } else {
        TlsConfig::default()
    };

    let config = ProxyConfig {
        port: args.port,
        configure_system_proxy: args.system_proxy,
        start_services: !args.no_services,
        enable_socks: !args.no_socks,
        enable_mcp: !args.no_mcp,
        tls: tls_config,
        ..Default::default()
    };

    if args.system_proxy {
        tracing::info!("System proxy will be configured on startup");
    }

    tracing::info!("Proxy listening on port {}", config.port);

    run_proxy(config).await
}
