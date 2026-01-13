//! Full-Stack Kubernetes Integration Test
//!
//! This test validates the complete Roxy workflow:
//! 1. Deploys the fullstack-k8s-dev example to Kubernetes
//! 2. Builds and loads the NestJS Docker image
//! 3. Launches Roxy with system proxy enabled
//! 4. Simulates React Native app requests to localhost:3000 (NestJS)
//! 5. Verifies that requests are captured in ClickHouse
//! 6. Cleans up all resources
//!
//! Prerequisites:
//! - Local Kubernetes cluster (Docker Desktop, minikube, or kind)
//! - ClickHouse running on localhost:8123
//! - Gateway API CRDs installed
//!
//! Run with:
//!   cargo test --test integration_test -- --ignored --nocapture

use anyhow::{Context, Result};
use roxy_core::{ClickHouseConfig, HttpRequestRecord, RoxyClickHouse};
use std::process::{Child, Command, Stdio};
use std::time::Duration;
use tokio::time::sleep;

/// Helper to execute shell commands
fn exec_cmd(cmd: &str, args: &[&str]) -> Result<String> {
    let output = Command::new(cmd)
        .args(args)
        .output()
        .context(format!("Failed to execute: {} {:?}", cmd, args))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Command failed: {} {:?}\n{}", cmd, args, stderr);
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Helper to execute a command without waiting for it to finish
fn spawn_cmd(cmd: &str, args: &[&str]) -> Result<Child> {
    Command::new(cmd)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context(format!("Failed to spawn: {} {:?}", cmd, args))
}

/// Check if kubectl is available
fn check_kubectl() -> Result<()> {
    exec_cmd("kubectl", &["version", "--client"])
        .context("kubectl not found. Please install kubectl.")?;
    Ok(())
}

/// Check if Docker is available
fn check_docker() -> Result<()> {
    exec_cmd("docker", &["version", "--format", "{{.Client.Version}}"])
        .context("Docker not found. Please install Docker.")?;
    Ok(())
}

/// Check if ClickHouse is available
async fn check_clickhouse() -> Result<()> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()?;

    client
        .get("http://localhost:8123/ping")
        .send()
        .await
        .context("ClickHouse not available at localhost:8123")?;

    Ok(())
}

/// Build the NestJS Docker image
fn build_nestjs_image() -> Result<()> {
    println!("🐳 Building NestJS Docker image...");
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let nestjs_dir = format!("{}/nestjs-api", manifest_dir);

    exec_cmd(
        "docker",
        &["build", "-t", "orders-api:latest", &nestjs_dir],
    )
    .context("Failed to build orders-api Docker image")?;

    println!("✅ NestJS image built successfully");
    Ok(())
}

/// Deploy Kubernetes resources
/// kubectl apply is idempotent and will create or update resources
fn deploy_kubernetes() -> Result<()> {
    println!("☸️  Deploying Kubernetes resources...");

    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let k8s_dir = format!("{}/kubernetes", manifest_dir);

    // Use server-side apply with force to handle conflicts gracefully
    exec_cmd("kubectl", &["apply", "--server-side", "--force-conflicts", "-k", &k8s_dir])
        .context("Failed to deploy Kubernetes resources")?;

    println!("✅ Kubernetes resources deployed");
    Ok(())
}

/// Wait for deployments to be ready
async fn wait_for_deployments() -> Result<()> {
    println!("⏳ Waiting for deployments to be ready...");

    let deployments = vec![
        ("postgres", "database", 60),      // Database needs time to initialize
        ("kafka", "messaging", 90),        // Kafka has 30s initial delay + startup time
        ("config-service", "backend", 60), // Config service
        ("orders-api", "backend", 120),    // Orders API waits for postgres + kafka via init containers
    ];

    for (name, namespace, timeout_attempts) in deployments {
        println!("  Waiting for {}/{} (timeout: {}s)...", namespace, name, timeout_attempts * 2);

        let mut last_status = String::new();

        for attempt in 1..=timeout_attempts {
            // First check the deployment rollout status
            let rollout_result = Command::new("kubectl")
                .args(&[
                    "rollout",
                    "status",
                    &format!("deployment/{}", name),
                    "-n",
                    namespace,
                    "--timeout=5s",
                ])
                .output();

            if let Ok(output) = rollout_result {
                if output.status.success() {
                    println!("  ✅ {}/{} ready", namespace, name);
                    break;
                }
            }

            // Check pod status for more details
            let pod_result = Command::new("kubectl")
                .args(&[
                    "get",
                    "pods",
                    "-n",
                    namespace,
                    "-l",
                    &format!("app={}", name),
                    "-o",
                    "jsonpath={.items[0].status.conditions[?(@.type=='Ready')].status}",
                ])
                .output();

            if let Ok(output) = pod_result {
                let status = String::from_utf8_lossy(&output.stdout);
                if status != last_status {
                    // Get more detailed status
                    let phase_result = Command::new("kubectl")
                        .args(&[
                            "get",
                            "pods",
                            "-n",
                            namespace,
                            "-l",
                            &format!("app={}", name),
                            "-o",
                            "jsonpath={.items[0].status.phase} ({.items[0].status.containerStatuses[0].ready})",
                        ])
                        .output();

                    if let Ok(phase_output) = phase_result {
                        let phase_status = String::from_utf8_lossy(&phase_output.stdout);
                        println!("    Status: {} - {}", status.trim(), phase_status.trim());
                        last_status = status.to_string();
                    }
                }
            }

            if attempt == timeout_attempts {
                // Print detailed error information
                println!("  ❌ Timeout waiting for {}/{}", namespace, name);
                println!("  Deployment status:");
                let _ = Command::new("kubectl")
                    .args(&[
                        "get",
                        "deployment",
                        name,
                        "-n",
                        namespace,
                    ])
                    .status();

                println!("  Pod status:");
                let _ = Command::new("kubectl")
                    .args(&[
                        "describe",
                        "pods",
                        "-n",
                        namespace,
                        "-l",
                        &format!("app={}", name),
                    ])
                    .status();

                anyhow::bail!("Timeout waiting for {}/{} to be ready", namespace, name);
            }

            sleep(Duration::from_secs(2)).await;
        }
    }

    println!("✅ All deployments ready");
    Ok(())
}

/// Start port forward for orders-api
fn start_port_forward() -> Result<Child> {
    println!("🔌 Starting port forward for orders-api...");

    let child = spawn_cmd(
        "kubectl",
        &[
            "port-forward",
            "-n",
            "backend",
            "service/orders-api",
            "3000:3000",
        ],
    )
    .context("Failed to start port forward")?;

    println!("✅ Port forward started (PID: {})", child.id());
    Ok(child)
}

/// Start the Roxy proxy server
fn start_roxy() -> Result<Child> {
    println!("🚀 Starting Roxy proxy...");

    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let project_root = format!("{}/../../", manifest_dir);

    // Build Roxy from the workspace root
    println!("  Building Roxy UI binary...");
    let build_result = Command::new("cargo")
        .args(&["build", "--release", "-p", "roxy-ui"])
        .current_dir(&project_root)
        .output()
        .context("Failed to build Roxy")?;

    if !build_result.status.success() {
        let stderr = String::from_utf8_lossy(&build_result.stderr);
        anyhow::bail!("Failed to build Roxy:\n{}", stderr);
    }

    let child = spawn_cmd(
        &format!("{}/target/release/roxy", project_root),
        &["--enable-system-proxy"],
    )
    .context("Failed to start Roxy")?;

    println!("✅ Roxy started (PID: {})", child.id());
    Ok(child)
}

/// Create an HTTP client configured to use the Roxy proxy
fn create_http_client() -> Result<reqwest::Client> {
    let client = reqwest::Client::builder()
        .proxy(reqwest::Proxy::http("http://127.0.0.1:8080")?)
        .proxy(reqwest::Proxy::https("http://127.0.0.1:8080")?)
        .timeout(Duration::from_secs(30))
        .danger_accept_invalid_certs(true) // For TLS interception
        .build()?;

    Ok(client)
}

/// Simulate React Native app requests
async fn simulate_app_requests(client: &reqwest::Client) -> Result<()> {
    println!("📱 Simulating React Native app requests...");

    // Use the hostname configured in the HTTPRoute
    let base_url = "http://api.example.com";

    // 1. Check health/dependencies
    println!("  1️⃣  Checking dependencies...");
    let response = client
        .get(&format!("{}/api/orders/health/dependencies", base_url))
        .send()
        .await
        .context("Failed to check dependencies")?;
    println!("     Status: {}", response.status());

    // 2. Get order stats
    println!("  2️⃣  Getting order stats...");
    let response = client
        .get(&format!("{}/api/orders/stats/summary", base_url))
        .send()
        .await
        .context("Failed to get stats")?;
    println!("     Status: {}", response.status());

    // 3. Create a test order
    println!("  3️⃣  Creating test order...");
    let order_payload = serde_json::json!({
        "customerId": format!("test-{}", chrono::Utc::now().timestamp()),
        "customerEmail": format!("test-{}@example.com", chrono::Utc::now().timestamp()),
        "items": [
            {
                "productId": "prod-001",
                "productName": "Test Product",
                "quantity": 2,
                "unitPrice": 29.99
            },
            {
                "productId": "prod-002",
                "productName": "Another Product",
                "quantity": 1,
                "unitPrice": 49.99
            }
        ],
        "shippingAddress": {
            "street": "123 Test Street",
            "city": "San Francisco",
            "state": "CA",
            "postalCode": "94102",
            "country": "USA"
        },
        "notes": "Created from integration test"
    });

    let response = client
        .post(&format!("{}/api/orders", base_url))
        .json(&order_payload)
        .send()
        .await
        .context("Failed to create order")?;

    println!("     Status: {}", response.status());
    let order: serde_json::Value = response.json().await?;
    let order_id = order["id"].as_str().unwrap();
    println!("     Order ID: {}", order_id);

    // 4. Get orders list
    println!("  4️⃣  Getting orders list...");
    let response = client
        .get(&format!("{}/api/orders?limit=10", base_url))
        .send()
        .await
        .context("Failed to get orders")?;
    println!("     Status: {}", response.status());

    // 5. Get specific order
    println!("  5️⃣  Getting order by ID...");
    let response = client
        .get(&format!("{}/api/orders/{}", base_url, order_id))
        .send()
        .await
        .context("Failed to get order by ID")?;
    println!("     Status: {}", response.status());

    // 6. Confirm order
    println!("  6️⃣  Confirming order...");
    let response = client
        .post(&format!("{}/api/orders/{}/confirm", base_url, order_id))
        .send()
        .await
        .context("Failed to confirm order")?;
    println!("     Status: {}", response.status());

    println!("✅ All requests completed successfully");
    Ok(())
}

/// Verify that requests were captured in ClickHouse
async fn verify_captured_requests() -> Result<()> {
    println!("🔍 Verifying captured requests in ClickHouse...");

    let config = ClickHouseConfig {
        database: "roxy".to_string(),
        ..Default::default()
    };
    let clickhouse = RoxyClickHouse::new(config);

    // Wait a bit for ClickHouse to process the inserts
    sleep(Duration::from_secs(2)).await;

    // Query recent requests
    let requests = clickhouse
        .get_recent_requests(100)
        .await
        .context("Failed to query ClickHouse")?;

    println!("  Found {} recent requests", requests.len());

    // Filter for requests to api.example.com
    let app_requests: Vec<&HttpRequestRecord> = requests
        .iter()
        .filter(|r| r.host.contains("api.example.com"))
        .collect();

    println!("  Found {} requests to api.example.com", app_requests.len());

    // Verify we captured the expected requests
    let expected_paths = vec![
        "/api/orders/health/dependencies",
        "/api/orders/stats/summary",
        "/api/orders",
    ];

    for path in &expected_paths {
        let found = app_requests.iter().any(|r| r.path.contains(path));
        if found {
            println!("  ✅ Found request to {}", path);
        } else {
            anyhow::bail!("Missing expected request to {}", path);
        }
    }

    // Verify we have GET, POST requests
    let has_get = app_requests.iter().any(|r| r.method == "GET");
    let has_post = app_requests.iter().any(|r| r.method == "POST");

    if has_get {
        println!("  ✅ Found GET requests");
    } else {
        anyhow::bail!("No GET requests found");
    }

    if has_post {
        println!("  ✅ Found POST requests");
    } else {
        anyhow::bail!("No POST requests found");
    }

    // Verify response bodies were captured
    let has_bodies = app_requests
        .iter()
        .any(|r| !r.response_body.is_empty());
    if has_bodies {
        println!("  ✅ Response bodies captured");
    } else {
        println!("  ⚠️  Warning: No response bodies captured");
    }

    println!("✅ All verifications passed");
    Ok(())
}

/// Cleanup Kubernetes resources
fn cleanup_kubernetes() -> Result<()> {
    println!("🧹 Cleaning up Kubernetes resources...");
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let k8s_dir = format!("{}/kubernetes", manifest_dir);

    let _ = exec_cmd("kubectl", &["delete", "-k", &k8s_dir]);
    println!("✅ Kubernetes resources cleaned up");
    Ok(())
}

/// Main integration test
#[tokio::test]
#[ignore = "requires local k8s cluster and ClickHouse"]
async fn fullstack_k8s_integration_test() -> Result<()> {
    println!("🧪 Starting Full-Stack Kubernetes Integration Test\n");

    // Check prerequisites
    println!("🔍 Checking prerequisites...");
    check_kubectl()?;
    check_docker()?;
    check_clickhouse().await?;
    println!("✅ All prerequisites met\n");

    // Build Docker image
    build_nestjs_image()?;
    println!();

    // Deploy to Kubernetes
    deploy_kubernetes()?;
    println!();

    // Wait for deployments
    wait_for_deployments().await?;
    println!();

    // Start Roxy (will automatically route via HTTPRoute resources)
    // Roxy's GatewayRouter will detect the HTTPRoute for api.example.com
    // and route to orders-api.backend.svc.cluster.local:3000
    let mut roxy_process = start_roxy()?;
    println!();

    // Give Roxy time to start
    sleep(Duration::from_secs(3)).await;

    // Run the test
    let test_result = async {
        // Create HTTP client
        let client = create_http_client()?;

        // Simulate app requests
        simulate_app_requests(&client).await?;
        println!();

        // Verify captured requests
        verify_captured_requests().await?;
        println!();

        Ok::<(), anyhow::Error>(())
    }
    .await;

    // Cleanup
    println!("🧹 Cleaning up...");
    let _ = roxy_process.kill();
    // Skip cleanup to keep resources for debugging/next run
    // cleanup_kubernetes()?;
    println!();

    // Check test result
    test_result?;

    println!("🎉 Integration test completed successfully!");
    Ok(())
}

/// Shorter test that just verifies the deployment works
#[tokio::test]
#[ignore = "requires local k8s cluster"]
async fn test_k8s_deployment_only() -> Result<()> {
    println!("🧪 Testing Kubernetes Deployment\n");

    check_kubectl()?;
    check_docker()?;

    build_nestjs_image()?;
    deploy_kubernetes()?;
    wait_for_deployments().await?;

    // Verify services are accessible
    println!("🔍 Verifying services...");

    let namespaces = vec![
        ("backend", vec!["orders-api", "config-service"]),
        ("database", vec!["postgres"]),
        ("messaging", vec!["kafka"]),
    ];

    for (namespace, services) in namespaces {
        for service in services {
            let output = exec_cmd(
                "kubectl",
                &["get", "service", service, "-n", namespace, "-o", "name"],
            )?;
            if output.contains(service) {
                println!("  ✅ Service {}/{} exists", namespace, service);
            }
        }
    }

    // Skip cleanup to keep resources for debugging/next run
    // cleanup_kubernetes()?;

    println!("\n🎉 Deployment test passed!");
    Ok(())
}
