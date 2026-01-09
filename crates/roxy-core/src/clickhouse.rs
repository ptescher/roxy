//! ClickHouse client and schema definitions for Roxy
//!
//! This module provides the database layer for storing and querying
//! HTTP traffic data captured by the proxy.

use anyhow::{Context, Result};
use clickhouse::{Client, Row};
use serde::{Deserialize, Serialize};
use std::fmt;

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

        // Migration: Add client_name column if it doesn't exist (for existing tables)
        self.client
            .query(
                "ALTER TABLE http_requests ADD COLUMN IF NOT EXISTS client_name String DEFAULT ''",
            )
            .execute()
            .await
            .context("Failed to add client_name column")?;

        // Create hosts summary materialized view
        self.client
            .query(HOSTS_SUMMARY_VIEW_SCHEMA)
            .execute()
            .await
            .context("Failed to create hosts_summary view")?;

        // Create database queries table
        self.client
            .query(DATABASE_QUERIES_TABLE_SCHEMA)
            .execute()
            .await
            .context("Failed to create database_queries table")?;

        // Migration: Add target_host column to database_queries if it doesn't exist
        self.client
            .query(
                "ALTER TABLE database_queries ADD COLUMN IF NOT EXISTS target_host String DEFAULT ''",
            )
            .execute()
            .await
            .context("Failed to add target_host column to database_queries")?;

        // Create kafka messages table
        self.client
            .query(KAFKA_MESSAGES_TABLE_SCHEMA)
            .execute()
            .await
            .context("Failed to create kafka_messages table")?;

        // Migration: Add target_host column to kafka_messages if it doesn't exist
        self.client
            .query(
                "ALTER TABLE kafka_messages ADD COLUMN IF NOT EXISTS target_host String DEFAULT ''",
            )
            .execute()
            .await
            .context("Failed to add target_host column to kafka_messages")?;

        // Create TCP connections table for raw packet tracking
        self.client
            .query(TCP_CONNECTIONS_TABLE_SCHEMA)
            .execute()
            .await
            .context("Failed to create tcp_connections table")?;

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
                duration_ms, error, client_ip, server_ip, protocol, tls_version, client_name
            ) VALUES (
                '{id}', '{trace_id}', '{span_id}', {timestamp}, '{method}', '{url}',
                '{host}', '{path}', '{query}', '{request_headers}', '{request_body}',
                {request_body_size}, {response_status}, '{response_headers}',
                '{response_body}', {response_body_size}, {duration_ms}, '{error}',
                '{client_ip}', '{server_ip}', '{protocol}', '{tls_version}', '{client_name}'
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
            client_name = escape_string(&request.client_name),
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

    /// Get all unique client services/applications
    ///
    /// Returns an empty vec if the client_name column doesn't exist yet
    /// (graceful handling for schema migrations).
    pub async fn get_client_services(&self) -> Result<Vec<ClientServiceSummary>> {
        // First check if the client_name column exists
        let column_exists = self
            .client
            .query(
                r"
                SELECT count() as cnt
                FROM system.columns
                WHERE database = 'roxy' AND table = 'http_requests' AND name = 'client_name'
                ",
            )
            .fetch_one::<u64>()
            .await
            .unwrap_or(0);

        if column_exists == 0 {
            // Column doesn't exist yet, return empty
            return Ok(Vec::new());
        }

        let services = self
            .client
            .query(
                r"
                SELECT
                    client_name,
                    count() as request_count,
                    avg(duration_ms) as avg_duration_ms,
                    max(timestamp) as last_seen
                FROM http_requests
                WHERE client_name != ''
                GROUP BY client_name
                ORDER BY request_count DESC
                ",
            )
            .fetch_all::<ClientServiceSummary>()
            .await?;
        Ok(services)
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

    /// Insert a database query record
    pub async fn insert_database_query(&self, record: &DatabaseQueryRow) -> Result<()> {
        let query = format!(
            r"INSERT INTO database_queries (
                id, trace_id, span_id, parent_span_id, timestamp, duration_ms,
                db_system, db_name, db_user, db_operation, db_statement, db_rows_affected,
                server_address, server_port, target_host, client_address, success,
                error_message, error_code, application_name, attributes
            ) VALUES (
                '{id}', '{trace_id}', '{span_id}', '{parent_span_id}', {timestamp}, {duration_ms},
                '{db_system}', '{db_name}', '{db_user}', '{db_operation}', '{db_statement}', {db_rows_affected},
                '{server_address}', {server_port}, '{target_host}', '{client_address}', {success},
                '{error_message}', '{error_code}', '{application_name}', '{attributes}'
            )",
            id = record.id,
            trace_id = escape_string(&record.trace_id),
            span_id = escape_string(&record.span_id),
            parent_span_id = escape_string(&record.parent_span_id),
            timestamp = record.timestamp,
            duration_ms = record.duration_ms,
            db_system = escape_string(&record.db_system),
            db_name = escape_string(&record.db_name),
            db_user = escape_string(&record.db_user),
            db_operation = escape_string(&record.db_operation),
            db_statement = escape_string(&record.db_statement),
            db_rows_affected = record.db_rows_affected,
            server_address = escape_string(&record.server_address),
            server_port = record.server_port,
            target_host = escape_string(&record.target_host),
            client_address = escape_string(&record.client_address),
            success = record.success,
            error_message = escape_string(&record.error_message),
            error_code = escape_string(&record.error_code),
            application_name = escape_string(&record.application_name),
            attributes = escape_string(&record.attributes),
        );

        self.client
            .query(&query)
            .execute()
            .await
            .context("Failed to insert database query")?;

        Ok(())
    }

    /// Insert a Kafka message record
    pub async fn insert_kafka_message(&self, record: &KafkaMessageRow) -> Result<()> {
        let query = format!(
            r"INSERT INTO kafka_messages (
                id, trace_id, span_id, parent_span_id, timestamp, duration_ms,
                messaging_system, messaging_operation, operation_type,
                messaging_destination, messaging_consumer_group, messaging_client_id,
                kafka_api_key, kafka_api_version, kafka_correlation_id,
                message_count, payload_size, server_address, server_port, target_host, client_address,
                success, error_code, error_message, attributes
            ) VALUES (
                '{id}', '{trace_id}', '{span_id}', '{parent_span_id}', {timestamp}, {duration_ms},
                '{messaging_system}', '{messaging_operation}', '{operation_type}',
                '{messaging_destination}', '{messaging_consumer_group}', '{messaging_client_id}',
                {kafka_api_key}, {kafka_api_version}, {kafka_correlation_id},
                {message_count}, {payload_size}, '{server_address}', {server_port}, '{target_host}', '{client_address}',
                {success}, {error_code}, '{error_message}', '{attributes}'
            )",
            id = record.id,
            trace_id = escape_string(&record.trace_id),
            span_id = escape_string(&record.span_id),
            parent_span_id = escape_string(&record.parent_span_id),
            timestamp = record.timestamp,
            duration_ms = record.duration_ms,
            messaging_system = escape_string(&record.messaging_system),
            messaging_operation = escape_string(&record.messaging_operation),
            operation_type = escape_string(&record.operation_type),
            messaging_destination = escape_string(&record.messaging_destination),
            messaging_consumer_group = escape_string(&record.messaging_consumer_group),
            messaging_client_id = escape_string(&record.messaging_client_id),
            kafka_api_key = record.kafka_api_key,
            kafka_api_version = record.kafka_api_version,
            kafka_correlation_id = record.kafka_correlation_id,
            message_count = record.message_count,
            payload_size = record.payload_size,
            server_address = escape_string(&record.server_address),
            server_port = record.server_port,
            target_host = escape_string(&record.target_host),
            client_address = escape_string(&record.client_address),
            success = record.success,
            error_code = record.error_code,
            error_message = escape_string(&record.error_message),
            attributes = escape_string(&record.attributes),
        );

        self.client
            .query(&query)
            .execute()
            .await
            .context("Failed to insert Kafka message")?;

        Ok(())
    }

    /// Query recent database queries
    pub async fn get_recent_database_queries(&self, limit: u32) -> Result<Vec<DatabaseQueryRow>> {
        // Explicit column list to handle schema migrations (target_host added later)
        let queries = self
            .client
            .query(
                "SELECT id, trace_id, span_id, parent_span_id, timestamp, duration_ms, \
                 db_system, db_name, db_user, db_operation, db_statement, db_rows_affected, \
                 server_address, server_port, target_host, client_address, success, \
                 error_message, error_code, application_name, attributes \
                 FROM database_queries ORDER BY timestamp DESC LIMIT ?",
            )
            .bind(limit)
            .fetch_all::<DatabaseQueryRow>()
            .await?;
        Ok(queries)
    }

    /// Query recent Kafka messages
    pub async fn get_recent_kafka_messages(&self, limit: u32) -> Result<Vec<KafkaMessageRow>> {
        // Explicit column list to handle schema migrations (target_host added later)
        let messages = self
            .client
            .query(
                "SELECT id, trace_id, span_id, parent_span_id, timestamp, duration_ms, \
                 messaging_system, messaging_operation, operation_type, messaging_destination, \
                 messaging_consumer_group, messaging_client_id, kafka_api_key, kafka_api_version, \
                 kafka_correlation_id, message_count, payload_size, server_address, server_port, \
                 target_host, client_address, success, error_code, error_message, attributes \
                 FROM kafka_messages ORDER BY timestamp DESC LIMIT ?",
            )
            .bind(limit)
            .fetch_all::<KafkaMessageRow>()
            .await?;
        Ok(messages)
    }

    /// Query database queries by db_system (e.g., "postgresql")
    pub async fn get_database_queries_by_system(
        &self,
        db_system: &str,
        limit: u32,
    ) -> Result<Vec<DatabaseQueryRow>> {
        // Explicit column list to handle schema migrations (target_host added later)
        let queries = self
            .client
            .query(
                "SELECT id, trace_id, span_id, parent_span_id, timestamp, duration_ms, \
                 db_system, db_name, db_user, db_operation, db_statement, db_rows_affected, \
                 server_address, server_port, target_host, client_address, success, \
                 error_message, error_code, application_name, attributes \
                 FROM database_queries WHERE db_system = ? ORDER BY timestamp DESC LIMIT ?",
            )
            .bind(db_system)
            .bind(limit)
            .fetch_all::<DatabaseQueryRow>()
            .await?;
        Ok(queries)
    }

    /// Query Kafka messages by topic
    pub async fn get_kafka_messages_by_topic(
        &self,
        topic: &str,
        limit: u32,
    ) -> Result<Vec<KafkaMessageRow>> {
        // Explicit column list to handle schema migrations (target_host added later)
        let messages = self
            .client
            .query(
                "SELECT id, trace_id, span_id, parent_span_id, timestamp, duration_ms, \
                 messaging_system, messaging_operation, operation_type, messaging_destination, \
                 messaging_consumer_group, messaging_client_id, kafka_api_key, kafka_api_version, \
                 kafka_correlation_id, message_count, payload_size, server_address, server_port, \
                 target_host, client_address, success, error_code, error_message, attributes \
                 FROM kafka_messages WHERE messaging_destination LIKE ? ORDER BY timestamp DESC LIMIT ?",
            )
            .bind(format!("%{}%", topic))
            .bind(limit)
            .fetch_all::<KafkaMessageRow>()
            .await?;
        Ok(messages)
    }

    /// Insert a TCP connection record
    pub async fn insert_tcp_connection(&self, record: &TcpConnectionRow) -> Result<()> {
        let query = format!(
            r"INSERT INTO tcp_connections (
                id, trace_id, span_id, timestamp, duration_ms,
                protocol, protocol_detected, wire_detected,
                client_address, client_port, server_address, server_port,
                target_host, bytes_sent, bytes_received,
                client_messages, server_messages, parse_errors,
                initial_bytes_hex, initial_bytes_ascii, sample_request_hex, sample_response_hex,
                status, error_message, attributes
            ) VALUES (
                '{id}', '{trace_id}', '{span_id}', {timestamp}, {duration_ms},
                '{protocol}', '{protocol_detected}', {wire_detected},
                '{client_address}', {client_port}, '{server_address}', {server_port},
                '{target_host}', {bytes_sent}, {bytes_received},
                {client_messages}, {server_messages}, {parse_errors},
                '{initial_bytes_hex}', '{initial_bytes_ascii}', '{sample_request_hex}', '{sample_response_hex}',
                '{status}', '{error_message}', '{attributes}'
            )",
            id = record.id,
            trace_id = escape_string(&record.trace_id),
            span_id = escape_string(&record.span_id),
            timestamp = record.timestamp,
            duration_ms = record.duration_ms,
            protocol = escape_string(&record.protocol),
            protocol_detected = escape_string(&record.protocol_detected),
            wire_detected = record.wire_detected,
            client_address = escape_string(&record.client_address),
            client_port = record.client_port,
            server_address = escape_string(&record.server_address),
            server_port = record.server_port,
            target_host = escape_string(&record.target_host),
            bytes_sent = record.bytes_sent,
            bytes_received = record.bytes_received,
            client_messages = record.client_messages,
            server_messages = record.server_messages,
            parse_errors = record.parse_errors,
            initial_bytes_hex = escape_string(&record.initial_bytes_hex),
            initial_bytes_ascii = escape_string(&record.initial_bytes_ascii),
            sample_request_hex = escape_string(&record.sample_request_hex),
            sample_response_hex = escape_string(&record.sample_response_hex),
            status = escape_string(&record.status),
            error_message = escape_string(&record.error_message),
            attributes = escape_string(&record.attributes),
        );

        self.client
            .query(&query)
            .execute()
            .await
            .context("Failed to insert TCP connection")?;

        Ok(())
    }

    /// Query recent TCP connections (all protocols)
    pub async fn get_recent_connections(&self, limit: u32) -> Result<Vec<TcpConnectionRow>> {
        let connections = self
            .client
            .query("SELECT * FROM tcp_connections ORDER BY timestamp DESC LIMIT ?")
            .bind(limit)
            .fetch_all::<TcpConnectionRow>()
            .await?;
        Ok(connections)
    }

    /// Query TCP connections by protocol
    pub async fn get_connections_by_protocol(
        &self,
        protocol: &str,
        limit: u32,
    ) -> Result<Vec<TcpConnectionRow>> {
        let connections = self
            .client
            .query(
                "SELECT * FROM tcp_connections WHERE protocol = ? ORDER BY timestamp DESC LIMIT ?",
            )
            .bind(protocol)
            .bind(limit)
            .fetch_all::<TcpConnectionRow>()
            .await?;
        Ok(connections)
    }

    /// Query TCP connections by target host
    pub async fn get_connections_by_host(
        &self,
        host: &str,
        limit: u32,
    ) -> Result<Vec<TcpConnectionRow>> {
        let connections = self
            .client
            .query("SELECT * FROM tcp_connections WHERE target_host LIKE ? ORDER BY timestamp DESC LIMIT ?")
            .bind(format!("%{}%", host))
            .bind(limit)
            .fetch_all::<TcpConnectionRow>()
            .await?;
        Ok(connections)
    }

    /// Get active Kubernetes service connections (connections in the last N seconds)
    ///
    /// This returns unique K8s service connections that are recent enough to be
    /// considered "active", useful for showing port forwards in the UI.
    pub async fn get_active_k8s_connections(
        &self,
        max_age_seconds: u64,
    ) -> Result<Vec<ActiveK8sConnection>> {
        let connections = self
            .client
            .query(
                r"
                SELECT
                    target_host,
                    server_port,
                    count() as connection_count,
                    max(timestamp) as last_seen
                FROM tcp_connections
                WHERE target_host LIKE '%.svc.cluster.local'
                  AND timestamp > (now64(3) - ?) * 1000
                GROUP BY target_host, server_port
                ORDER BY last_seen DESC
                ",
            )
            .bind(max_age_seconds as i64)
            .fetch_all::<ActiveK8sConnection>()
            .await?;
        Ok(connections)
    }
}

/// Active Kubernetes service connection summary
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Row)]
pub struct ActiveK8sConnection {
    /// Target host (e.g., postgres.database.svc.cluster.local)
    pub target_host: String,
    /// Server port
    pub server_port: u16,
    /// Number of connections
    pub connection_count: u64,
    /// Last time this connection was seen (timestamp)
    pub last_seen: i64,
}

impl ActiveK8sConnection {
    /// Parse the namespace from the target host
    pub fn namespace(&self) -> Option<String> {
        // Format: service.namespace.svc.cluster.local
        let parts: Vec<&str> = self.target_host.split('.').collect();
        if parts.len() >= 2 {
            Some(parts[1].to_string())
        } else {
            None
        }
    }

    /// Parse the service name from the target host
    pub fn service_name(&self) -> Option<String> {
        // Format: service.namespace.svc.cluster.local
        let parts: Vec<&str> = self.target_host.split('.').collect();
        if !parts.is_empty() {
            Some(parts[0].to_string())
        } else {
            None
        }
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
    /// Name of the client application making requests (extracted from headers)
    pub client_name: String,
}

/// Summary of requests per host
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Row)]
pub struct HostSummary {
    pub host: String,
    pub request_count: u64,
    pub avg_duration_ms: f64,
    pub last_seen: i64,
}

/// Summary of requests per client service/application
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Row)]
pub struct ClientServiceSummary {
    /// Client name (from X-Client-Name header or User-Agent)
    pub client_name: String,
    /// Number of HTTP requests from this client
    pub request_count: u64,
    /// Average response time in ms
    pub avg_duration_ms: f64,
    /// Last time this client was seen (timestamp)
    pub last_seen: i64,
}

/// Database query record stored in ClickHouse
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Row)]
pub struct DatabaseQueryRow {
    pub id: String,
    pub trace_id: String,
    pub span_id: String,
    pub parent_span_id: String,
    pub timestamp: i64,
    pub duration_ms: f64,
    pub db_system: String,
    pub db_name: String,
    pub db_user: String,
    pub db_operation: String,
    pub db_statement: String,
    pub db_rows_affected: i64,
    pub server_address: String,
    pub server_port: u16,
    /// Original target hostname (e.g., postgres.database.svc.cluster.local)
    pub target_host: String,
    pub client_address: String,
    pub success: u8,
    pub error_message: String,
    pub error_code: String,
    pub application_name: String,
    pub attributes: String,
}

/// Kafka message record stored in ClickHouse
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Row)]
pub struct KafkaMessageRow {
    pub id: String,
    pub trace_id: String,
    pub span_id: String,
    pub parent_span_id: String,
    pub timestamp: i64,
    pub duration_ms: f64,
    pub messaging_system: String,
    pub messaging_operation: String,
    pub operation_type: String,
    pub messaging_destination: String,
    pub messaging_consumer_group: String,
    pub messaging_client_id: String,
    pub kafka_api_key: i16,
    pub kafka_api_version: i16,
    pub kafka_correlation_id: i32,
    pub message_count: i32,
    pub payload_size: i64,
    pub server_address: String,
    pub server_port: u16,
    /// Original target hostname (e.g., kafka.messaging.svc.cluster.local)
    pub target_host: String,
    pub client_address: String,
    pub success: u8,
    pub error_code: i16,
    pub error_message: String,
    pub attributes: String,
}

/// TCP connection record for raw packet tracking
/// This captures ALL connections through the SOCKS proxy, even if protocol is unknown
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Row)]
pub struct TcpConnectionRow {
    /// Unique connection ID
    pub id: String,
    /// Trace ID for distributed tracing
    pub trace_id: String,
    /// Span ID
    pub span_id: String,
    /// Connection start timestamp (milliseconds since epoch)
    pub timestamp: i64,
    /// Connection duration in milliseconds
    pub duration_ms: f64,
    /// Detected protocol (postgresql, kafka, unknown, etc.)
    pub protocol: String,
    /// How protocol was detected (wire, port, none)
    pub protocol_detected: String,
    /// Whether protocol was detected from wire data (vs port fallback)
    pub wire_detected: u8,
    /// Client IP address
    pub client_address: String,
    /// Client port
    pub client_port: u16,
    /// Server IP address (resolved)
    pub server_address: String,
    /// Server port
    pub server_port: u16,
    /// Original target hostname (e.g., kafka.messaging.svc.cluster.local)
    pub target_host: String,
    /// Total bytes sent (client to server)
    pub bytes_sent: u64,
    /// Total bytes received (server to client)
    pub bytes_received: u64,
    /// Number of client messages parsed
    pub client_messages: u64,
    /// Number of server messages parsed
    pub server_messages: u64,
    /// Number of parse errors
    pub parse_errors: u64,
    /// First N bytes of connection (hex encoded) for debugging
    pub initial_bytes_hex: String,
    /// First N bytes as ASCII (printable chars only)
    pub initial_bytes_ascii: String,
    /// Sample of request data (hex)
    pub sample_request_hex: String,
    /// Sample of response data (hex)
    pub sample_response_hex: String,
    /// Connection status (ok, error, timeout)
    pub status: String,
    /// Error message if connection failed
    pub error_message: String,
    /// Additional attributes as JSON
    pub attributes: String,
}

/// Connection status for TCP connections
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionStatus {
    /// Connection completed successfully
    Ok,
    /// Connection had an error
    Error,
    /// Connection timed out
    Timeout,
    /// Connection was reset
    Reset,
}

impl fmt::Display for ConnectionStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConnectionStatus::Ok => write!(f, "ok"),
            ConnectionStatus::Error => write!(f, "error"),
            ConnectionStatus::Timeout => write!(f, "timeout"),
            ConnectionStatus::Reset => write!(f, "reset"),
        }
    }
}

impl TcpConnectionRow {
    /// Create a new TCP connection record
    pub fn new(
        target_host: String,
        server_address: String,
        server_port: u16,
        client_address: String,
        client_port: u16,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            trace_id: format!("{:032x}", rand::random::<u128>()),
            span_id: format!("{:016x}", rand::random::<u64>()),
            timestamp: chrono::Utc::now().timestamp_millis(),
            duration_ms: 0.0,
            protocol: "unknown".to_string(),
            protocol_detected: "none".to_string(),
            wire_detected: 0,
            client_address,
            client_port,
            server_address,
            server_port,
            target_host,
            bytes_sent: 0,
            bytes_received: 0,
            client_messages: 0,
            server_messages: 0,
            parse_errors: 0,
            initial_bytes_hex: String::new(),
            initial_bytes_ascii: String::new(),
            sample_request_hex: String::new(),
            sample_response_hex: String::new(),
            status: "ok".to_string(),
            error_message: String::new(),
            attributes: "{}".to_string(),
        }
    }

    /// Convert bytes to hex string (limited to first N bytes)
    pub fn bytes_to_hex(data: &[u8], max_bytes: usize) -> String {
        data.iter()
            .take(max_bytes)
            .map(|b| format!("{:02x}", b))
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Convert bytes to ASCII string (printable chars only)
    pub fn bytes_to_ascii(data: &[u8], max_bytes: usize) -> String {
        data.iter()
            .take(max_bytes)
            .map(|&b| {
                if b.is_ascii_graphic() || b == b' ' {
                    b as char
                } else {
                    '.'
                }
            })
            .collect()
    }
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
    client_name String DEFAULT '',
    INDEX idx_host host TYPE bloom_filter GRANULARITY 1,
    INDEX idx_trace_id trace_id TYPE bloom_filter GRANULARITY 1,
    INDEX idx_method method TYPE set(10) GRANULARITY 1,
    INDEX idx_status response_status TYPE set(100) GRANULARITY 1,
    INDEX idx_client_name client_name TYPE bloom_filter GRANULARITY 1
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

const DATABASE_QUERIES_TABLE_SCHEMA: &str = r"
CREATE TABLE IF NOT EXISTS database_queries (
    id String,
    trace_id String,
    span_id String,
    parent_span_id String,
    timestamp Int64,
    duration_ms Float64,
    db_system String,
    db_name String,
    db_user String,
    db_operation String,
    db_statement String,
    db_rows_affected Int64,
    server_address String,
    server_port UInt16,
    target_host String,
    client_address String,
    success UInt8,
    error_message String,
    error_code String,
    application_name String,
    attributes String,
    INDEX idx_trace_id trace_id TYPE bloom_filter GRANULARITY 1,
    INDEX idx_db_system db_system TYPE set(10) GRANULARITY 1,
    INDEX idx_db_operation db_operation TYPE set(20) GRANULARITY 1,
    INDEX idx_success success TYPE set(2) GRANULARITY 1
) ENGINE = MergeTree()
ORDER BY (db_system, timestamp, id)
PARTITION BY toYYYYMMDD(fromUnixTimestamp64Milli(timestamp))
TTL fromUnixTimestamp64Milli(timestamp) + INTERVAL 7 DAY
";

const KAFKA_MESSAGES_TABLE_SCHEMA: &str = r"
CREATE TABLE IF NOT EXISTS kafka_messages (
    id String,
    trace_id String,
    span_id String,
    parent_span_id String,
    timestamp Int64,
    duration_ms Float64,
    messaging_system String,
    messaging_operation String,
    operation_type String,
    messaging_destination String,
    messaging_consumer_group String,
    messaging_client_id String,
    kafka_api_key Int16,
    kafka_api_version Int16,
    kafka_correlation_id Int32,
    message_count Int32,
    payload_size Int64,
    server_address String,
    server_port UInt16,
    target_host String,
    client_address String,
    success UInt8,
    error_code Int16,
    error_message String,
    attributes String,
    INDEX idx_trace_id trace_id TYPE bloom_filter GRANULARITY 1,
    INDEX idx_operation messaging_operation TYPE set(50) GRANULARITY 1,
    INDEX idx_destination messaging_destination TYPE bloom_filter GRANULARITY 1,
    INDEX idx_consumer_group messaging_consumer_group TYPE bloom_filter GRANULARITY 1,
    INDEX idx_success success TYPE set(2) GRANULARITY 1
) ENGINE = MergeTree()
ORDER BY (messaging_operation, timestamp, id)
PARTITION BY toYYYYMMDD(fromUnixTimestamp64Milli(timestamp))
TTL fromUnixTimestamp64Milli(timestamp) + INTERVAL 7 DAY
";

const TCP_CONNECTIONS_TABLE_SCHEMA: &str = r"
CREATE TABLE IF NOT EXISTS tcp_connections (
    id String,
    trace_id String,
    span_id String,
    timestamp Int64,
    duration_ms Float64,
    protocol String,
    protocol_detected String,
    wire_detected UInt8,
    client_address String,
    client_port UInt16,
    server_address String,
    server_port UInt16,
    target_host String,
    bytes_sent UInt64,
    bytes_received UInt64,
    client_messages UInt64,
    server_messages UInt64,
    parse_errors UInt64,
    initial_bytes_hex String,
    initial_bytes_ascii String,
    sample_request_hex String,
    sample_response_hex String,
    status String,
    error_message String,
    attributes String,
    INDEX idx_trace_id trace_id TYPE bloom_filter GRANULARITY 1,
    INDEX idx_protocol protocol TYPE set(20) GRANULARITY 1,
    INDEX idx_target_host target_host TYPE bloom_filter GRANULARITY 1,
    INDEX idx_status status TYPE set(10) GRANULARITY 1
) ENGINE = MergeTree()
ORDER BY (protocol, timestamp, id)
PARTITION BY toYYYYMMDD(fromUnixTimestamp64Milli(timestamp))
TTL fromUnixTimestamp64Milli(timestamp) + INTERVAL 7 DAY
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
