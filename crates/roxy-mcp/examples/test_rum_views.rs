//! Example: Test Datadog RUM views query
//!
//! This example demonstrates how to query Datadog RUM views using the
//! Datadog client directly (without going through MCP).
//!
//! Usage:
//!   cargo run --example test_rum_views --release

use roxy_core::{ClickHouseConfig, RoxyClickHouse, RoxyConfig};
use roxy_mcp::DatadogClient;
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("roxy_mcp=debug".parse()?),
        )
        .init();

    println!("Testing Datadog RUM views query...\n");

    // Load config and export to environment
    let config = RoxyConfig::load_with_env()?;
    config.export_to_env()?;

    println!("✓ Configuration loaded");

    // Create ClickHouse client
    let ch_config = ClickHouseConfig::default();
    let clickhouse = Arc::new(RoxyClickHouse::new(ch_config));

    // Create Datadog client
    let datadog = match DatadogClient::new(clickhouse) {
        Ok(client) => {
            println!("✓ Datadog client initialized");
            client
        }
        Err(e) => {
            eprintln!("✗ Failed to initialize Datadog client: {}", e);
            eprintln!("\nMake sure DD_API_KEY and DD_APP_KEY are set in:");
            eprintln!("  ~/Library/Application Support/roxy/config.toml");
            return Err(e);
        }
    };

    // Query RUM views - last 24 hours, limit 10
    println!("\nQuerying RUM views (last 24 hours, limit 10)...");

    let params = roxy_mcp::datadog::QueryRumViewsParams {
        application_id: None,
        view_name: None,
        time_range_hours: 24,
        limit: 10,
    };

    match datadog.query_rum_views(params).await {
        Ok(result) => {
            println!("\n=== RUM Views Response ===\n");

            // The result contains a vector of Content items
            // Each item is either text or an image
            for content in result.content {
                // Print the debug representation which will show the structure
                println!("{:#?}", content);
            }

            println!("\n✓ Query completed successfully");
        }
        Err(e) => {
            eprintln!("\n✗ Query failed: {}", e);
            return Err(anyhow::anyhow!(e.to_string()));
        }
    }

    Ok(())
}
