//! Example: TLS Interception
//!
//! This example demonstrates how to configure roxy-proxy to intercept
//! TLS connections for specific hosts.
//!
//! # Running
//!
//! ```bash
//! cargo run --example tls_interception
//! ```
//!
//! # Testing
//!
//! 1. Trust the CA certificate:
//!    - macOS: Open Keychain Access, import ~/.roxy/ca.crt, set to "Always Trust"
//!    - Linux: Copy to /usr/local/share/ca-certificates/ and run update-ca-certificates
//!
//! 2. Configure your client to use the proxy:
//!    ```bash
//!    export HTTPS_PROXY=http://127.0.0.1:8080
//!    curl https://api.example.com/test
//!    ```
//!
//! 3. Only requests to hosts in the intercept list will be decrypted.
//!    Other HTTPS requests pass through as encrypted tunnels.

use anyhow::Result;
use roxy_proxy::{ProxyConfig, ProxyServer, TlsConfig, TlsInterceptionMode};
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,roxy_proxy=debug"));

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_target(true))
        .init();

    // Configure TLS interception for specific hosts
    let tls_config = TlsConfig {
        // Enable TLS interception
        enabled: true,

        // Only intercept listed hosts (other HTTPS traffic passes through)
        mode: TlsInterceptionMode::InterceptListed,

        // List of hosts to intercept
        // Supports exact matches and wildcards like "*.example.com"
        intercept_hosts: vec![
            "api.example.com".to_string(),
            "*.internal.company.com".to_string(),
        ],

        // CA certificate will be generated at these paths
        // Default: ~/.roxy/ca.crt and ~/.roxy/ca.key
        ..Default::default()
    };

    // Create proxy configuration
    let config = ProxyConfig {
        port: 8080,
        tls: tls_config,
        // Disable services for this example
        start_services: false,
        enable_socks: false,
        enable_mcp: false,
        ..Default::default()
    };

    // Create and run the proxy
    let mut server = ProxyServer::new(config).await?;

    // Print CA certificate path for user to trust
    if let Some(ca_path) = server.ca_cert_path().await {
        println!("\n=== TLS Interception Enabled ===");
        println!("CA Certificate: {}", ca_path);
        println!("\nTo trust the CA on macOS:");
        println!("  1. Open Keychain Access");
        println!("  2. File -> Import Items -> Select {}", ca_path);
        println!("  3. Double-click the certificate");
        println!("  4. Expand 'Trust' and set to 'Always Trust'");
        println!("\nIntercepting hosts:");
        for host in server.tls_intercept_hosts().await {
            println!("  - {}", host);
        }
        println!("================================\n");
    }

    // Dynamically add/remove hosts at runtime
    // server.add_tls_intercept_host("another.example.com".to_string()).await;
    // server.remove_tls_intercept_host("api.example.com").await;

    // Run the proxy (blocks until shutdown)
    server.run().await
}
