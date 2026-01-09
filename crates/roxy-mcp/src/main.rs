//! Roxy MCP Server - Standalone binary entry point
//!
//! This binary runs the MCP server with stdio transport for use with
//! Claude Desktop and other MCP clients.

use tracing_subscriber::{fmt, prelude::*, EnvFilter};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging to stderr (stdout is used for MCP communication)
    tracing_subscriber::registry()
        .with(fmt::layer().with_writer(std::io::stderr))
        .with(EnvFilter::from_default_env().add_directive("roxy_mcp=info".parse()?))
        .init();

    tracing::info!("Starting Roxy MCP server");

    roxy_mcp::run_stdio().await
}
