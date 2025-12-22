//! ClickHouse client and schema definitions for Roxy
//!
//! This module provides the database layer for storing and querying
//! HTTP traffic data captured by the proxy.

use anyhow::{Context, Result};
use clickhouse::{Client, Row};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Escape a string for safe use in ClickHouse SQL queries
fn escape_string(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\'', "\\'")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
        .replace('\0', "")
}

/// Configuration for connecting to ClickHouse
#[derive(Debug, Clone)]
pub struct ClickHouseConfig {
    pub url: String,
    pub database: String,
    pub username: Option<String>,
    pub password: Option<String>,
}

impl Default for ClickHouseConfig {
    fn default() -> Self {
        Self {
            url: "http://localhost:8123".to_string(),
            database: "roxy".to_string(),
            username: None,
            password: None,
        }
    }
}

/// ClickHouse client wrapper for Roxy
#[derive(Clone)]
pub struct RoxyClickHouse {
    client: Client,
    config: ClickHouseConfig,
}

impl RoxyClickHouse {
    /// Create a new ClickHouse client with the given configuration
    pub fn new(config: ClickHouseConfig) -> Self {
        let mut client = Client::default()
            .with_url(&config.url)
            .with_database(&config.database);

        if let Some(ref username) = config.username {
            client = client.with_user(username);
        }

        if let Some(ref password) = config.password {
            client = client.with_password(password);
        }

        Self { client, config }
    }

    /// Create a client with default localhost configuration
    pub fn localhost() -> Self {
        Self::new(ClickHouseConfig::default())
    }

    /// Initialize the database schema
    pub async fn initialize_schema(&self) -> Result<()> {
        // Create database if not exists
        self.client
            .query(&format!(
                "CREATE DATABASE IF NOT EXISTS {}",
                self.config.database
            ))
            .execute()
            .await
            .context("Failed to create database")?;

        // Create spans table for OpenTelemetry traces
        self.client
            .query(SPANS_TABLE_SCHEMA)
            .execute()
            .await
            .context("Failed to create spans table")?;

        // Create HTTP requests table
        self.client
            .query(HTTP_REQUESTS_TABLE_SCHEMA)
            .execute()
            .await
            .context("Failed to create http_requests table")?;

        // Create hosts summary materialized view
        self.client
            .query(HOSTS_SUMMARY_VIEW_SCHEMA)
            .execute()
            .await
            .context("Failed to create hosts_summary view")?;

        tracing::info!("ClickHouse schema initialized successfully");
        Ok(())
    }

    /// Insert a span record
    pub async fn insert_span(&self, span: &SpanRecord) -> Result<()> {
        let mut insert = self.client.insert("spans")?;
        insert.write(span).await?;
        insert.end().await?;
        Ok(())
    }

    /// Insert multiple span records
    pub async fn insert_spans(&self, spans: &[SpanRecord]) -> Result<()> {
        let mut insert = self.client.insert("spans")?;
        for span in spans {
            insert.write(span).await?;
        }
        insert.end().await?;
        Ok(())
    }

    /// Insert an HTTP request record using SQL INSERT for better compatibility
    pub async fn insert_http_request(&self, request: &HttpRequestRecord) -> Result<()> {
        // Use SQL INSERT with proper escaping to avoid binary format issues
        let query = format!(
            r"INSERT INTO http_requests (
                id, trace_id, span_id, timestamp, method, url, host, path, query,
                request_headers, request_body, request_body_size,
                response_status, response_headers, response_body, response_body_size,
                duration_ms, error, client_ip, server_ip, protocol, tls_version
            ) VALUES (
                '{id}', '{trace_id}', '{span_id}', {timestamp}, '{method}', '{url}',
                '{host}', '{path}', '{query}', '{request_headers}', '{request_body}',
                {request_body_size}, {response_status}, '{response_headers}',
                '{response_body}', {response_body_size}, {duration_ms}, '{error}',
                '{client_ip}', '{server_ip}', '{protocol}', '{tls_version}'
            )",
            id = request.id,
            trace_id = escape_string(&request.trace_id),
            span_id = escape_string(&request.span_id),
            timestamp = request.timestamp,
            method = escape_string(&request.method),
            url = escape_string(&request.url),
            host = escape_string(&request.host),
            path = escape_string(&request.path),
            query = escape_string(&request.query),
            request_headers = escape_string(&request.request_headers),
            request_body = escape_string(&request.request_body),
            request_body_size = request.request_body_size,
            response_status = request.response_status,
            response_headers = escape_string(&request.response_headers),
            response_body = escape_string(&request.response_body),
            response_body_size = request.response_body_size,
            duration_ms = request.duration_ms,
            error = escape_string(&request.error),
            client_ip = escape_string(&request.client_ip),
            server_ip = escape_string(&request.server_ip),
            protocol = escape_string(&request.protocol),
            tls_version = escape_string(&request.tls_version),
        );

        self.client
            .query(&query)
            .execute()
            .await
            .context("Failed to insert HTTP request")?;

        Ok(())
    }

    /// Query recent HTTP requests
    pub async fn get_recent_requests(&self, limit: u32) -> Result<Vec<HttpRequestRecord>> {
        let requests = self
            .client
            .query("SELECT * FROM http_requests ORDER BY timestamp DESC LIMIT ?")
            .bind(limit)
            .fetch_all::<HttpRequestRecord>()
            .await?;
        Ok(requests)
    }

    /// Query requests by host
    pub async fn get_requests_by_host(
        &self,
        host: &str,
        limit: u32,
    ) -> Result<Vec<HttpRequestRecord>> {
        let requests = self
            .client
            .query("SELECT * FROM http_requests WHERE host = ? ORDER BY timestamp DESC LIMIT ?")
            .bind(host)
            .bind(limit)
            .fetch_all::<HttpRequestRecord>()
            .await?;
        Ok(requests)
    }

    /// Get all unique hosts
    pub async fn get_hosts(&self) -> Result<Vec<HostSummary>> {
        let hosts = self
            .client
            .query(
                r"
                SELECT
                    host,
                    count() as request_count,
                    avg(duration_ms) as avg_duration_ms,
                    max(timestamp) as last_seen
                FROM http_requests
                GROUP BY host
                ORDER BY request_count DESC
                ",
            )
            .fetch_all::<HostSummary>()
            .await?;
        Ok(hosts)
    }

    /// Get spans for a trace
    pub async fn get_trace_spans(&self, trace_id: &str) -> Result<Vec<SpanRecord>> {
        let spans = self
            .client
            .query("SELECT * FROM spans WHERE trace_id = ? ORDER BY start_time ASC")
            .bind(trace_id)
            .fetch_all::<SpanRecord>()
            .await?;
        Ok(spans)
    }

    /// Get recent spans
    pub async fn get_recent_spans(&self, limit: u32) -> Result<Vec<SpanRecord>> {
        let spans = self
            .client
            .query("SELECT * FROM spans ORDER BY start_time DESC LIMIT ?")
            .bind(limit)
            .fetch_all::<SpanRecord>()
            .await?;
        Ok(spans)
    }

    /// Get request/response by ID
    pub async fn get_request_by_id(&self, id: &str) -> Result<Option<HttpRequestRecord>> {
        let requests = self
            .client
            .query("SELECT * FROM http_requests WHERE id = ?")
            .bind(id)
            .fetch_all::<HttpRequestRecord>()
            .await?;
        Ok(requests.into_iter().next())
    }
}

/// OpenTelemetry span record stored in ClickHouse
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Row)]
pub struct SpanRecord {
    pub trace_id: String,
    pub span_id: String,
    pub parent_span_id: String,
    pub name: String,
    pub service_name: String,
    pub kind: String,
    pub start_time: i64,
    pub end_time: i64,
    pub duration_ns: i64,
    pub status_code: String,
    pub status_message: String,
    pub attributes: String, // JSON encoded
    pub events: String,     // JSON encoded
    pub links: String,      // JSON encoded
}

/// HTTP request/response record stored in ClickHouse
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Row)]
pub struct HttpRequestRecord {
    /// Request ID - stored as String for ClickHouse compatibility
    pub id: String,
    pub trace_id: String,
    pub span_id: String,
    pub timestamp: i64,
    pub method: String,
    pub url: String,
    pub host: String,
    pub path: String,
    pub query: String,
    pub request_headers: String, // JSON encoded
    pub request_body: String,    // Base64 or plain text
    pub request_body_size: i64,
    pub response_status: u16,
    pub response_headers: String, // JSON encoded
    pub response_body: String,    // Base64 or plain text
    pub response_body_size: i64,
    pub duration_ms: f64,
    pub error: String,
    pub client_ip: String,
    pub server_ip: String,
    pub protocol: String,
    pub tls_version: String,
}

/// Summary of requests per host
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Row)]
pub struct HostSummary {
    pub host: String,
    pub request_count: u64,
    pub avg_duration_ms: f64,
    pub last_seen: i64,
}

// SQL schema definitions

const SPANS_TABLE_SCHEMA: &str = r"
CREATE TABLE IF NOT EXISTS spans (
    trace_id String,
    span_id String,
    parent_span_id String,
    name String,
    service_name String,
    kind String,
    start_time Int64,
    end_time Int64,
    duration_ns Int64,
    status_code String,
    status_message String,
    attributes String,
    events String,
    links String,
    INDEX idx_trace_id trace_id TYPE bloom_filter GRANULARITY 1,
    INDEX idx_service_name service_name TYPE bloom_filter GRANULARITY 1
) ENGINE = MergeTree()
ORDER BY (service_name, start_time, trace_id)
PARTITION BY toYYYYMMDD(fromUnixTimestamp64Nano(start_time))
TTL fromUnixTimestamp64Nano(start_time) + INTERVAL 7 DAY
";

const HTTP_REQUESTS_TABLE_SCHEMA: &str = r"
CREATE TABLE IF NOT EXISTS http_requests (
    id String,
    trace_id String,
    span_id String,
    timestamp Int64,
    method String,
    url String,
    host String,
    path String,
    query String,
    request_headers String,
    request_body String,
    request_body_size Int64,
    response_status UInt16,
    response_headers String,
    response_body String,
    response_body_size Int64,
    duration_ms Float64,
    error String,
    client_ip String,
    server_ip String,
    protocol String,
    tls_version String,
    INDEX idx_host host TYPE bloom_filter GRANULARITY 1,
    INDEX idx_trace_id trace_id TYPE bloom_filter GRANULARITY 1,
    INDEX idx_method method TYPE set(10) GRANULARITY 1,
    INDEX idx_status response_status TYPE set(100) GRANULARITY 1
) ENGINE = MergeTree()
ORDER BY (host, timestamp, id)
PARTITION BY toYYYYMMDD(fromUnixTimestamp64Milli(timestamp))
TTL fromUnixTimestamp64Milli(timestamp) + INTERVAL 7 DAY
";

const HOSTS_SUMMARY_VIEW_SCHEMA: &str = r"
CREATE MATERIALIZED VIEW IF NOT EXISTS hosts_summary_mv
ENGINE = SummingMergeTree()
ORDER BY (host)
AS SELECT
    host,
    count() as request_count,
    sum(duration_ms) as total_duration_ms,
    max(timestamp) as last_seen
FROM http_requests
GROUP BY host
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = ClickHouseConfig::default();
        assert_eq!(config.url, "http://localhost:8123");
        assert_eq!(config.database, "roxy");
        assert!(config.username.is_none());
        assert!(config.password.is_none());
    }
}
