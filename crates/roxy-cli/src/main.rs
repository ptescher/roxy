mod docker;
mod sandbox;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "roxy", about = "Roxy — HTTP/HTTPS proxy for development")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the proxy directly
    Proxy {
        /// Proxy listen port
        #[arg(long, default_value_t = 8080)]
        port: u16,

        /// SOCKS5 proxy port
        #[arg(long, default_value_t = 1080)]
        socks_port: u16,

        /// Configure macOS system proxy on start
        #[arg(long)]
        system_proxy: bool,
    },

    /// Launch a Docker sandbox with all traffic routed through Roxy
    Sandbox(sandbox::SandboxArgs),
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Proxy {
            port,
            socks_port: _,
            system_proxy,
        } => {
            run_proxy(port, system_proxy).await?;
        }
        Commands::Sandbox(args) => {
            sandbox::run(args).await?;
        }
    }

    Ok(())
}

async fn run_proxy(port: u16, system_proxy: bool) -> anyhow::Result<()> {
    use roxy_core::proxy_manager::{ProxyManager, ProxyManagerConfig};

    let config = ProxyManagerConfig {
        proxy_port: port,
        configure_system_proxy: system_proxy,
        ..Default::default()
    };

    let manager = ProxyManager::new(config);
    manager.start().await?;

    tracing::info!("Proxy running on port {port}. Press Ctrl+C to stop.");

    tokio::signal::ctrl_c().await?;

    tracing::info!("Shutting down...");
    manager.stop().await?;

    Ok(())
}
