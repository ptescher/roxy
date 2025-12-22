//! Integration tests for Roxy Proxy
//!
//! These tests verify the HTTP request capture and ClickHouse storage functionality.
//! Note: Some tests require ClickHouse to be running.

use roxy_core::{ClickHouseConfig, HttpRequestRecord, RoxyClickHouse};
use std::time::Duration;
use uuid::Uuid;

/// Create a test HTTP request record
fn create_test_request(method: &str, url: &str, status: u16) -> HttpRequestRecord {
    HttpRequestRecord {
        id: Uuid::new_v4().to_string(),
        trace_id: format!("{:032x}", rand::random::<u128>()),
        span_id: format!("{:016x}", rand::random::<u64>()),
        timestamp: chrono::Utc::now().timestamp_millis(),
        method: method.to_string(),
        url: url.to_string(),
        host: url
            .parse::<url::Url>()
            .ok()
            .and_then(|u| u.host_str().map(String::from))
            .unwrap_or_else(|| "example.com".to_string()),
        path: url
            .parse::<url::Url>()
            .ok()
            .map(|u| u.path().to_string())
            .unwrap_or_else(|| "/".to_string()),
        query: "".to_string(),
        request_headers: r#"{"Content-Type": "application/json", "Accept": "*/*"}"#.to_string(),
        request_body: r#"{"test": true}"#.to_string(),
        request_body_size: 14,
        response_status: status,
        response_headers: r#"{"Content-Type": "application/json"}"#.to_string(),
        response_body: r#"{"success": true}"#.to_string(),
        response_body_size: 17,
        duration_ms: 45.5,
        error: "".to_string(),
        client_ip: "127.0.0.1".to_string(),
        server_ip: "93.184.216.34".to_string(),
        protocol: "HTTP/1.1".to_string(),
        tls_version: "TLSv1.3".to_string(),
    }
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn test_create_request_record() {
        let request = create_test_request("GET", "https://example.com/api/test", 200);

        assert_eq!(request.method, "GET");
        assert_eq!(request.url, "https://example.com/api/test");
        assert_eq!(request.host, "example.com");
        assert_eq!(request.path, "/api/test");
        assert_eq!(request.response_status, 200);
        assert!(request.duration_ms > 0.0);
    }

    #[test]
    fn test_request_record_methods() {
        let get_request = create_test_request("GET", "https://api.example.com/users", 200);
        assert_eq!(get_request.method, "GET");

        let post_request = create_test_request("POST", "https://api.example.com/users", 201);
        assert_eq!(post_request.method, "POST");

        let delete_request = create_test_request("DELETE", "https://api.example.com/users/1", 204);
        assert_eq!(delete_request.method, "DELETE");
    }

    #[test]
    fn test_request_record_status_codes() {
        let ok = create_test_request("GET", "https://example.com", 200);
        assert!(ok.response_status >= 200 && ok.response_status < 300);

        let redirect = create_test_request("GET", "https://example.com", 301);
        assert!(redirect.response_status >= 300 && redirect.response_status < 400);

        let not_found = create_test_request("GET", "https://example.com", 404);
        assert!(not_found.response_status >= 400 && not_found.response_status < 500);

        let error = create_test_request("GET", "https://example.com", 500);
        assert!(error.response_status >= 500);
    }

    #[test]
    fn test_request_body_parsing() {
        let request = create_test_request("POST", "https://example.com/api", 200);

        // Verify JSON body can be parsed
        let body: serde_json::Value = serde_json::from_str(&request.request_body).unwrap();
        assert_eq!(body["test"], true);
    }

    #[test]
    fn test_request_headers_parsing() {
        let request = create_test_request("GET", "https://example.com", 200);

        // Verify JSON headers can be parsed
        let headers: serde_json::Value = serde_json::from_str(&request.request_headers).unwrap();
        assert_eq!(headers["Content-Type"], "application/json");
    }

    #[test]
    fn test_unique_ids() {
        let request1 = create_test_request("GET", "https://example.com", 200);
        let request2 = create_test_request("GET", "https://example.com", 200);

        // Each request should have unique IDs
        assert_ne!(request1.id, request2.id);
        assert_ne!(request1.trace_id, request2.trace_id);
        assert_ne!(request1.span_id, request2.span_id);
    }
}

#[cfg(test)]
mod clickhouse_tests {
    use super::*;

    /// Check if ClickHouse is available
    async fn clickhouse_available() -> bool {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap();

        client
            .get("http://localhost:8123/ping")
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }

    #[tokio::test]
    async fn test_clickhouse_config_default() {
        let config = ClickHouseConfig::default();
        assert_eq!(config.url, "http://localhost:8123");
        assert_eq!(config.database, "roxy");
        assert!(config.username.is_none());
        assert!(config.password.is_none());
    }

    #[tokio::test]
    async fn test_clickhouse_client_creation() {
        let config = ClickHouseConfig::default();
        let _client = RoxyClickHouse::new(config);
        // Just verify it doesn't panic
    }

    #[tokio::test]
    #[ignore = "requires ClickHouse to be running"]
    async fn test_clickhouse_schema_initialization() {
        if !clickhouse_available().await {
            eprintln!("Skipping test: ClickHouse not available");
            return;
        }

        let config = ClickHouseConfig {
            database: "roxy_test".to_string(),
            ..Default::default()
        };
        let client = RoxyClickHouse::new(config);

        let result = client.initialize_schema().await;
        assert!(result.is_ok(), "Failed to initialize schema: {:?}", result);
    }

    #[tokio::test]
    #[ignore = "requires ClickHouse to be running"]
    async fn test_clickhouse_insert_request() {
        if !clickhouse_available().await {
            eprintln!("Skipping test: ClickHouse not available");
            return;
        }

        let config = ClickHouseConfig {
            database: "roxy_test".to_string(),
            ..Default::default()
        };
        let client = RoxyClickHouse::new(config);

        // Ensure schema exists
        client.initialize_schema().await.unwrap();

        // Insert a request
        let request = create_test_request("GET", "https://test.example.com/api/v1/users", 200);
        let result = client.insert_http_request(&request).await;
        assert!(result.is_ok(), "Failed to insert request: {:?}", result);
    }

    #[tokio::test]
    #[ignore = "requires ClickHouse to be running"]
    async fn test_clickhouse_query_requests() {
        if !clickhouse_available().await {
            eprintln!("Skipping test: ClickHouse not available");
            return;
        }

        let config = ClickHouseConfig {
            database: "roxy_test".to_string(),
            ..Default::default()
        };
        let client = RoxyClickHouse::new(config);

        // Ensure schema exists
        client.initialize_schema().await.unwrap();

        // Insert a request with a unique host for testing
        let unique_host = format!("test-{}.example.com", Uuid::new_v4());
        let mut request = create_test_request("POST", &format!("https://{}/api", unique_host), 201);
        request.host = unique_host.clone();

        client.insert_http_request(&request).await.unwrap();

        // Wait for ClickHouse to process
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Query recent requests
        let requests = client.get_recent_requests(100).await.unwrap();
        assert!(!requests.is_empty(), "No requests found");

        // Verify our request is in the results
        let found = requests.iter().any(|r| r.id == request.id);
        assert!(found, "Inserted request not found in query results");
    }

    #[tokio::test]
    #[ignore = "requires ClickHouse to be running"]
    async fn test_clickhouse_query_by_host() {
        if !clickhouse_available().await {
            eprintln!("Skipping test: ClickHouse not available");
            return;
        }

        let config = ClickHouseConfig {
            database: "roxy_test".to_string(),
            ..Default::default()
        };
        let client = RoxyClickHouse::new(config);

        // Ensure schema exists
        client.initialize_schema().await.unwrap();

        // Insert requests with a unique host
        let unique_host = format!("host-{}.example.com", Uuid::new_v4());

        for i in 0..3 {
            let mut request = create_test_request(
                "GET",
                &format!("https://{}/api/item/{}", unique_host, i),
                200,
            );
            request.host = unique_host.clone();
            client.insert_http_request(&request).await.unwrap();
        }

        // Wait for ClickHouse to process
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Query by host
        let requests = client.get_requests_by_host(&unique_host, 10).await.unwrap();
        assert_eq!(requests.len(), 3, "Expected 3 requests for host");
    }

    #[tokio::test]
    #[ignore = "requires ClickHouse to be running"]
    async fn test_clickhouse_get_hosts() {
        if !clickhouse_available().await {
            eprintln!("Skipping test: ClickHouse not available");
            return;
        }

        let config = ClickHouseConfig {
            database: "roxy_test".to_string(),
            ..Default::default()
        };
        let client = RoxyClickHouse::new(config);

        // Ensure schema exists
        client.initialize_schema().await.unwrap();

        // Insert a request
        let request = create_test_request("GET", "https://hosts-test.example.com/api", 200);
        client.insert_http_request(&request).await.unwrap();

        // Wait for ClickHouse to process
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Query hosts
        let hosts = client.get_hosts().await.unwrap();
        assert!(!hosts.is_empty(), "No hosts found");

        // Verify host summary contains expected fields
        for host in &hosts {
            assert!(!host.host.is_empty());
            assert!(host.request_count > 0);
        }
    }

    #[tokio::test]
    #[ignore = "requires ClickHouse to be running"]
    async fn test_clickhouse_get_request_by_id() {
        if !clickhouse_available().await {
            eprintln!("Skipping test: ClickHouse not available");
            return;
        }

        let config = ClickHouseConfig {
            database: "roxy_test".to_string(),
            ..Default::default()
        };
        let client = RoxyClickHouse::new(config);

        // Ensure schema exists
        client.initialize_schema().await.unwrap();

        // Insert a request
        let request = create_test_request("PUT", "https://id-test.example.com/api/resource", 200);
        let request_id = request.id.clone();
        client.insert_http_request(&request).await.unwrap();

        // Wait for ClickHouse to process
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Query by ID
        let found = client.get_request_by_id(&request_id).await.unwrap();
        assert!(found.is_some(), "Request not found by ID");

        let found = found.unwrap();
        assert_eq!(found.id, request_id);
        assert_eq!(found.method, "PUT");
    }
}

#[cfg(test)]
mod body_capture_tests {
    use super::*;

    #[test]
    fn test_json_body_capture() {
        let body = r#"{"name": "John", "age": 30, "active": true}"#;
        let request = HttpRequestRecord {
            request_body: body.to_string(),
            request_body_size: body.len() as i64,
            ..create_test_request("POST", "https://example.com/api", 200)
        };

        // Verify body is valid JSON
        let parsed: serde_json::Value = serde_json::from_str(&request.request_body).unwrap();
        assert_eq!(parsed["name"], "John");
        assert_eq!(parsed["age"], 30);
    }

    #[test]
    fn test_binary_body_base64_encoding() {
        // Simulate base64-encoded binary content
        let binary_marker = "base64:SGVsbG8gV29ybGQ="; // "Hello World" in base64
        let request = HttpRequestRecord {
            response_body: binary_marker.to_string(),
            response_body_size: 11, // "Hello World" is 11 bytes
            ..create_test_request("GET", "https://example.com/image.png", 200)
        };

        assert!(request.response_body.starts_with("base64:"));
    }

    #[test]
    fn test_empty_body() {
        let request = HttpRequestRecord {
            request_body: "".to_string(),
            request_body_size: 0,
            ..create_test_request("GET", "https://example.com/api", 200)
        };

        assert!(request.request_body.is_empty());
        assert_eq!(request.request_body_size, 0);
    }

    #[test]
    fn test_large_body_size_tracking() {
        let large_body = "x".repeat(1024 * 1024); // 1MB
        let request = HttpRequestRecord {
            response_body: large_body.clone(),
            response_body_size: large_body.len() as i64,
            ..create_test_request("GET", "https://example.com/large", 200)
        };

        assert_eq!(request.response_body_size, 1024 * 1024);
    }
}

#[cfg(test)]
mod header_tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_headers_json_encoding() {
        let mut headers = HashMap::new();
        headers.insert("Content-Type", "application/json");
        headers.insert("Authorization", "Bearer token123");
        headers.insert("X-Custom-Header", "custom-value");

        let json = serde_json::to_string(&headers).unwrap();

        let request = HttpRequestRecord {
            request_headers: json.clone(),
            ..create_test_request("GET", "https://example.com", 200)
        };

        // Verify we can decode it back
        let decoded: HashMap<String, String> =
            serde_json::from_str(&request.request_headers).unwrap();
        assert_eq!(
            decoded.get("Content-Type"),
            Some(&"application/json".to_string())
        );
    }

    #[test]
    fn test_response_headers() {
        let headers = r#"{"Content-Type": "text/html", "Cache-Control": "no-cache"}"#;
        let request = HttpRequestRecord {
            response_headers: headers.to_string(),
            ..create_test_request("GET", "https://example.com", 200)
        };

        let decoded: serde_json::Value = serde_json::from_str(&request.response_headers).unwrap();
        assert_eq!(decoded["Content-Type"], "text/html");
        assert_eq!(decoded["Cache-Control"], "no-cache");
    }
}

#[cfg(test)]
mod timing_tests {
    use super::*;

    #[test]
    fn test_duration_tracking() {
        let request = HttpRequestRecord {
            duration_ms: 123.456,
            ..create_test_request("GET", "https://example.com", 200)
        };

        assert!((request.duration_ms - 123.456).abs() < 0.001);
    }

    #[test]
    fn test_timestamp_is_recent() {
        let request = create_test_request("GET", "https://example.com", 200);
        let now = chrono::Utc::now().timestamp_millis();

        // Timestamp should be within the last second
        assert!(request.timestamp <= now);
        assert!(request.timestamp > now - 1000);
    }

    #[test]
    fn test_zero_duration() {
        let request = HttpRequestRecord {
            duration_ms: 0.0,
            ..create_test_request("GET", "https://example.com", 200)
        };

        assert_eq!(request.duration_ms, 0.0);
    }
}

#[cfg(test)]
mod url_parsing_tests {
    use super::*;

    #[test]
    fn test_url_with_path() {
        let request = create_test_request("GET", "https://api.example.com/v1/users/123", 200);
        assert_eq!(request.path, "/v1/users/123");
    }

    #[test]
    fn test_url_with_query() {
        let request = HttpRequestRecord {
            query: "page=1&limit=10".to_string(),
            ..create_test_request("GET", "https://example.com/api?page=1&limit=10", 200)
        };
        assert_eq!(request.query, "page=1&limit=10");
    }

    #[test]
    fn test_url_with_port() {
        let request = create_test_request("GET", "https://localhost:3000/api", 200);
        assert!(request.url.contains(":3000"));
    }

    #[test]
    fn test_different_hosts() {
        let hosts = vec![
            "api.example.com",
            "www.test.org",
            "localhost",
            "192.168.1.1",
        ];

        for host in hosts {
            let url = format!("https://{}/api", host);
            let request = create_test_request("GET", &url, 200);
            assert_eq!(request.host, host);
        }
    }
}
