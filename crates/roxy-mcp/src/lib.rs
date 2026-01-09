//! Roxy MCP Server - Model Context Protocol server for AI agent integration
//!
//! This crate provides MCP server functionality for Roxy, exposing observability
//! data (HTTP requests, spans, database queries, Kafka messages) to AI agents.
//!
//! ## Features
//!
//! - `stdio` - Enable stdio transport for CLI usage (default)
//! - `sse` - Enable SSE transport for remote/embedded usage

mod tools;

pub use tools::RoxyMcpServer;

use anyhow::Result;
use rmcp::ServiceExt;

/// Run the MCP server with stdio transport (for CLI usage)
#[cfg(feature = "stdio")]
pub async fn run_stdio() -> Result<()> {
    use rmcp::transport::io::stdio;

    let server = RoxyMcpServer::new().await?;
    let transport = stdio();
    let running_server = server.serve(transport).await?;

    tracing::info!("MCP server running on stdio");
    running_server.waiting().await?;

    Ok(())
}

/// Configuration for the SSE MCP server
#[cfg(feature = "sse")]
#[derive(Debug, Clone)]
pub struct SseServerConfig {
    /// Address to bind the SSE server to
    pub bind_address: std::net::SocketAddr,
    /// Path for SSE endpoint (default: "/sse")
    pub sse_path: String,
    /// Path for POST endpoint (default: "/message")
    pub post_path: String,
}

#[cfg(feature = "sse")]
impl Default for SseServerConfig {
    fn default() -> Self {
        Self {
            bind_address: ([127, 0, 0, 1], 3001).into(),
            sse_path: "/sse".to_string(),
            post_path: "/message".to_string(),
        }
    }
}

/// Handle for a running SSE MCP server
#[cfg(feature = "sse")]
pub struct SseMcpServer {
    cancel_token: tokio_util::sync::CancellationToken,
    handle: tokio::task::JoinHandle<()>,
}

#[cfg(feature = "sse")]
impl SseMcpServer {
    /// Stop the SSE server
    pub async fn stop(self) {
        self.cancel_token.cancel();
        let _ = self.handle.await;
    }

    /// Check if the server is still running
    pub fn is_running(&self) -> bool {
        !self.handle.is_finished()
    }
}

/// Start the MCP server with SSE transport (for embedding in other servers)
///
/// Returns a handle that can be used to stop the server.
#[cfg(feature = "sse")]
pub async fn run_sse(config: SseServerConfig) -> Result<SseMcpServer> {
    use futures::StreamExt;
    use rmcp::transport::sse_server::{SseServer, SseServerConfig as RmcpSseConfig};

    let rmcp_config = RmcpSseConfig {
        bind: config.bind_address,
        sse_path: config.sse_path,
        post_path: config.post_path,
        ct: Default::default(),
    };

    let mut sse_server = SseServer::serve_with_config(rmcp_config).await?;
    let cancel_token = tokio_util::sync::CancellationToken::new();
    let ct = cancel_token.clone();

    tracing::info!(
        address = %config.bind_address,
        "MCP SSE server starting"
    );

    let handle = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = ct.cancelled() => {
                    tracing::info!("MCP SSE server shutting down");
                    break;
                }
                transport = sse_server.next() => {
                    match transport {
                        Some(transport) => {
                            // Spawn a new server instance for each connection
                            tokio::spawn(async move {
                                match RoxyMcpServer::new().await {
                                    Ok(server) => {
                                        if let Err(e) = server.serve(transport).await {
                                            tracing::error!(error = %e, "MCP session error");
                                        }
                                    }
                                    Err(e) => {
                                        tracing::error!(error = %e, "Failed to create MCP server");
                                    }
                                }
                            });
                        }
                        None => {
                            tracing::warn!("SSE server stream ended");
                            break;
                        }
                    }
                }
            }
        }
    });

    Ok(SseMcpServer {
        cancel_token,
        handle,
    })
}

/// Default port for the MCP SSE server
pub const DEFAULT_MCP_SSE_PORT: u16 = 3001;
