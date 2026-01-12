# Testing Requirements Skill

This skill ensures comprehensive test coverage for all code changes in the Roxy codebase.

## When to Use

Activate this skill when:
- Writing new features
- Fixing bugs
- Refactoring existing code
- User asks "what tests should I write?"

## Test Coverage Requirements

### Minimum Coverage Targets

- **Unit Tests:** 80% line coverage
- **Integration Tests:** All public APIs must have integration tests
- **Protocol Parsers:** 100% coverage (security-critical)

## Test Structure

### 1. Unit Tests

Place unit tests in the same file as the code being tested:

```rust
// src/my_module.rs

pub fn calculate_total(items: &[Item]) -> Result<f64, Error> {
    // Implementation
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_calculate_total_empty() {
        let items = vec![];
        let result = calculate_total(&items);
        assert_eq!(result.unwrap(), 0.0);
    }
    
    #[test]
    fn test_calculate_total_single_item() {
        let items = vec![Item { price: 10.0 }];
        let result = calculate_total(&items);
        assert_eq!(result.unwrap(), 10.0);
    }
    
    #[test]
    fn test_calculate_total_multiple_items() {
        let items = vec![
            Item { price: 10.0 },
            Item { price: 20.0 },
            Item { price: 30.0 },
        ];
        let result = calculate_total(&items);
        assert_eq!(result.unwrap(), 60.0);
    }
    
    #[test]
    fn test_calculate_total_overflow() {
        let items = vec![Item { price: f64::MAX }];
        let result = calculate_total(&items);
        assert!(result.is_err());
    }
}
```

### 2. Integration Tests

Place integration tests in the `tests/` directory:

```rust
// tests/integration/proxy_tests.rs

use roxy_proxy::{ProxyServer, ProxyConfig};

#[tokio::test]
async fn test_http_proxy_forwards_request() {
    // Arrange: Start test server
    let test_server = test_helpers::start_echo_server().await;
    
    // Arrange: Start proxy
    let config = ProxyConfig {
        port: 0,  // Random available port
        ..Default::default()
    };
    let proxy = ProxyServer::new(config).await.unwrap();
    let proxy_port = proxy.port();
    
    // Act: Make request through proxy
    let client = reqwest::Client::builder()
        .proxy(reqwest::Proxy::http(format!("http://localhost:{}", proxy_port)).unwrap())
        .build()
        .unwrap();
    
    let response = client
        .get(&format!("{}/test", test_server.url()))
        .send()
        .await
        .unwrap();
    
    // Assert: Verify response
    assert_eq!(response.status(), 200);
    let body = response.text().await.unwrap();
    assert_eq!(body, "echo: /test");
    
    // Assert: Verify request was logged
    let requests = proxy.clickhouse().get_recent_requests(10).await.unwrap();
    assert!(!requests.is_empty());
    assert_eq!(requests[0].path, "/test");
}
```

### 3. Protocol Parser Tests

Protocol parsers require exhaustive testing:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    // Test valid messages
    #[test]
    fn test_parse_query_message() { /* ... */ }
    
    #[test]
    fn test_parse_bind_message() { /* ... */ }
    
    // Test incomplete messages
    #[test]
    fn test_parse_incomplete_message() {
        let data = vec![0x51, 0x00, 0x00];  // Truncated Query message
        let result = parse_message(&data, false);
        assert!(matches!(result, Err(ParseError::Incomplete { needed: _ })));
    }
    
    // Test invalid messages
    #[test]
    fn test_parse_invalid_length() {
        let data = vec![
            0x51,  // Query message
            0xFF, 0xFF, 0xFF, 0xFF,  // Huge invalid length
        ];
        let result = parse_message(&data, false);
        assert!(matches!(result, Err(ParseError::InvalidLength(_))));
    }
    
    // Test maximum size
    #[test]
    fn test_parse_max_size_message() {
        let max_size = MAX_MESSAGE_SIZE as usize;
        let mut data = vec![0x51];  // Query message
        data.extend_from_slice(&(max_size as u32).to_be_bytes());
        data.extend(vec![b'S'; max_size - 5]);  // Fill to max
        data.push(0);  // Null terminator
        
        let result = parse_message(&data, false);
        assert!(result.is_ok());
    }
    
    // Test boundary conditions
    #[test]
    fn test_parse_empty_string() { /* ... */ }
    
    #[test]
    fn test_parse_null_string() { /* ... */ }
    
    // Fuzz testing (use cargo-fuzz)
    #[test]
    fn test_parse_random_data() {
        for _ in 0..1000 {
            let data: Vec<u8> = (0..100).map(|_| rand::random()).collect();
            let _ = parse_message(&data, false);  // Should not panic
        }
    }
}
```

### 4. Error Path Testing

Every error path must be tested:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_connection_timeout() {
        let config = ProxyConfig {
            timeout: Duration::from_millis(1),
            ..Default::default()
        };
        let proxy = ProxyServer::new(config).await.unwrap();
        
        // Try to connect to a non-responsive server
        let result = proxy.forward_request("http://192.0.2.1:1234").await;
        assert!(matches!(result, Err(Error::Timeout)));
    }
    
    #[tokio::test]
    async fn test_invalid_hostname() {
        let proxy = ProxyServer::new(Default::default()).await.unwrap();
        let result = proxy.forward_request("http://invalid.host.local").await;
        assert!(matches!(result, Err(Error::DnsResolutionFailed(_))));
    }
    
    #[tokio::test]
    async fn test_connection_refused() {
        let proxy = ProxyServer::new(Default::default()).await.unwrap();
        let result = proxy.forward_request("http://localhost:1").await;  // Port 1 is unlikely to be listening
        assert!(matches!(result, Err(Error::ConnectionRefused)));
    }
}
```

### 5. Concurrency Tests

Test concurrent access to shared state:

```rust
#[tokio::test]
async fn test_concurrent_connections() {
    let proxy = Arc::new(ProxyServer::new(Default::default()).await.unwrap());
    
    // Spawn 100 concurrent requests
    let mut handles = Vec::new();
    for i in 0..100 {
        let proxy = proxy.clone();
        let handle = tokio::spawn(async move {
            proxy.forward_request(&format!("http://localhost/{}", i)).await
        });
        handles.push(handle);
    }
    
    // Wait for all to complete
    let results = futures::future::join_all(handles).await;
    
    // Verify no panics or deadlocks
    assert_eq!(results.len(), 100);
}

#[tokio::test]
async fn test_connection_tracking_race_condition() {
    let connections = Arc::new(ActiveK8sConnections::new());
    let service = K8sService {
        name: "test".to_string(),
        namespace: "default".to_string(),
        port: 8080,
    };
    
    // Spawn 10 concurrent adds
    let mut handles = Vec::new();
    for _ in 0..10 {
        let connections = connections.clone();
        let service = service.clone();
        let handle = tokio::spawn(async move {
            connections.add_connection(&service).await;
        });
        handles.push(handle);
    }
    
    futures::future::join_all(handles).await;
    
    // Should have exactly 1 entry with count=10
    let list = connections.list().await;
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].connection_count, 10);
}
```

## Test Helpers

Create reusable test helpers in `tests/helpers/`:

```rust
// tests/helpers/mod.rs

pub mod servers;
pub mod fixtures;

// tests/helpers/servers.rs

use tokio::net::TcpListener;
use axum::{Router, routing::get};

pub struct TestServer {
    addr: SocketAddr,
    shutdown_tx: tokio::sync::oneshot::Sender<()>,
}

impl TestServer {
    pub async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        
        let app = Router::new()
            .route("/", get(|| async { "Hello, World!" }))
            .route("/echo/:msg", get(|axum::extract::Path(msg): axum::extract::Path<String>| async move {
                format!("echo: {}", msg)
            }));
        
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
        
        tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async { shutdown_rx.await.ok(); })
                .await
                .unwrap();
        });
        
        TestServer { addr, shutdown_tx }
    }
    
    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        let _ = self.shutdown_tx.send(());
    }
}
```

## Property-Based Testing

For complex algorithms, use property-based testing with `proptest`:

```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn test_parse_any_valid_length(length in 4u32..1024) {
        let mut data = vec![0x51];  // Query message type
        data.extend_from_slice(&length.to_be_bytes());
        data.extend(vec![b'A'; (length - 5) as usize]);
        data.push(0);  // Null terminator
        
        let result = parse_message(&data, false);
        prop_assert!(result.is_ok());
    }
    
    #[test]
    fn test_headers_to_json_any_headers(headers in prop::collection::vec(prop::string::string_regex("[a-zA-Z-]{1,20}").unwrap(), 0..20)) {
        let mut header_map = hyper::HeaderMap::new();
        for header in headers {
            header_map.insert(header.parse().unwrap(), "value".parse().unwrap());
        }
        
        let json = headers_to_json(&header_map);
        prop_assert!(serde_json::from_str::<serde_json::Value>(&json).is_ok());
    }
}
```

## Performance Testing

Add benchmarks for hot paths:

```rust
// benches/proxy_throughput.rs

use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use roxy_proxy::*;

fn bench_parse_postgres_query(c: &mut Criterion) {
    let data = vec![
        b'Q',  // Query message
        0, 0, 0, 20,  // Length
        b'S', b'E', b'L', b'E', b'C', b'T', b' ', b'1', 0,
    ];
    
    c.bench_function("parse_postgres_query", |b| {
        b.iter(|| {
            black_box(parse_message(black_box(&data), false).unwrap())
        })
    });
}

fn bench_headers_to_json(c: &mut Criterion) {
    let mut headers = hyper::HeaderMap::new();
    for i in 0..20 {
        headers.insert(
            format!("x-header-{}", i).parse().unwrap(),
            format!("value-{}", i).parse().unwrap(),
        );
    }
    
    let mut group = c.benchmark_group("headers_to_json");
    group.throughput(Throughput::Elements(20));
    group.bench_function("20_headers", |b| {
        b.iter(|| black_box(headers_to_json(black_box(&headers))))
    });
    group.finish();
}

criterion_group!(benches, bench_parse_postgres_query, bench_headers_to_json);
criterion_main!(benches);
```

Run benchmarks with:
```bash
cargo bench
```

## Test Documentation

Every test must have a clear docstring explaining what it tests:

```rust
/// Test that parse_message correctly handles incomplete PostgreSQL Query messages.
///
/// This test verifies that when we receive a truncated Query message (missing
/// the query string), the parser returns `ParseError::Incomplete` with the
/// correct number of bytes needed to complete the message.
#[test]
fn test_parse_incomplete_query_message() {
    let data = vec![0x51, 0x00, 0x00, 0x00, 0x10];  // Query, length=16, but no query string
    let result = parse_message(&data, false);
    assert!(matches!(result, Err(ParseError::Incomplete { needed: 11 })));
}
```

## CI/CD Integration

Add to `.github/workflows/test.yml`:

```yaml
name: Tests

on: [push, pull_request]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v2
      - uses: actions-rs/toolchain@v1
        with:
          toolchain: stable
          override: true
      
      - name: Run tests
        run: cargo test --all --verbose
      
      - name: Run integration tests
        run: cargo test --test '*' --verbose
      
      - name: Install cargo-tarpaulin
        run: cargo install cargo-tarpaulin
      
      - name: Generate coverage
        run: cargo tarpaulin --out Xml --all
      
      - name: Upload coverage to Codecov
        uses: codecov/codecov-action@v2
        with:
          file: ./cobertura.xml
          fail_ci_if_error: true
      
      - name: Check coverage threshold
        run: |
          COVERAGE=$(cargo tarpaulin --out Json | jq '.coverage')
          if (( $(echo "$COVERAGE < 80.0" | bc -l) )); then
            echo "Coverage $COVERAGE% is below 80% threshold"
            exit 1
          fi
```

## Running Tests

```bash
# Run all tests
cargo test --all

# Run specific test
cargo test test_parse_query_message

# Run with output
cargo test -- --nocapture

# Run integration tests only
cargo test --test integration_tests

# Run benchmarks
cargo bench

# Generate coverage report
cargo tarpaulin --out Html
open tarpaulin-report.html
```

## Test Checklist

Before submitting a PR, verify:

- [ ] All new functions have unit tests
- [ ] All error paths are tested
- [ ] Edge cases are covered (empty, null, max size, etc.)
- [ ] Async functions use `#[tokio::test]`
- [ ] Tests have descriptive names and doc comments
- [ ] Integration tests cover all public APIs
- [ ] No tests are ignored without justification
- [ ] All tests pass locally
- [ ] Code coverage is above 80%

## Examples of Good Test Names

✅ Good:
- `test_parse_query_message_with_empty_string`
- `test_connection_timeout_after_1_second`
- `test_concurrent_requests_no_deadlock`
- `test_invalid_sql_returns_error`

❌ Bad:
- `test1`
- `test_stuff`
- `test_it_works`
- `test_function`
