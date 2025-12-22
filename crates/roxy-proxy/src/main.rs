//! Roxy Proxy - HTTP/HTTPS proxy with OpenTelemetry instrumentation
//!
//! This is the standalone binary entry point. For embedding the proxy
//! in other applications, use the library directly.

use anyhow::Result;
use roxy_proxy::{run_proxy, ProxyConfig};
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

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

    tracing::info!("Starting Roxy Proxy v{}", env!("CARGO_PKG_VERSION"));

    let config = ProxyConfig::default();
    run_proxy(config).await
}
