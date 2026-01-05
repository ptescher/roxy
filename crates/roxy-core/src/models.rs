//! HTTP request and response models for Roxy
//!
//! These types represent network traffic captured by the proxy.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// HTTP method
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Delete,
    Patch,
    Head,
    Options,
    Connect,
    Trace,
}

impl std::fmt::Display for HttpMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HttpMethod::Get => write!(f, "GET"),
            HttpMethod::Post => write!(f, "POST"),
            HttpMethod::Put => write!(f, "PUT"),
            HttpMethod::Delete => write!(f, "DELETE"),
            HttpMethod::Patch => write!(f, "PATCH"),
            HttpMethod::Head => write!(f, "HEAD"),
            HttpMethod::Options => write!(f, "OPTIONS"),
            HttpMethod::Connect => write!(f, "CONNECT"),
            HttpMethod::Trace => write!(f, "TRACE"),
        }
    }
}

impl From<&http::Method> for HttpMethod {
    fn from(method: &http::Method) -> Self {
        match *method {
            http::Method::GET => HttpMethod::Get,
            http::Method::POST => HttpMethod::Post,
            http::Method::PUT => HttpMethod::Put,
            http::Method::DELETE => HttpMethod::Delete,
            http::Method::PATCH => HttpMethod::Patch,
            http::Method::HEAD => HttpMethod::Head,
            http::Method::OPTIONS => HttpMethod::Options,
            http::Method::CONNECT => HttpMethod::Connect,
            http::Method::TRACE => HttpMethod::Trace,
            _ => HttpMethod::Get, // Fallback for custom methods
        }
    }
}

/// An HTTP header
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpHeader {
    pub name: String,
    pub value: String,
}

/// HTTP request captured by the proxy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpRequest {
    /// Unique identifier for this request
    pub id: Uuid,

    /// HTTP method
    pub method: HttpMethod,

    /// Full URL including scheme, host, path, and query
    pub url: String,

    /// Just the host portion
    pub host: String,

    /// Request path
    pub path: String,

    /// Query string (without leading ?)
    pub query: Option<String>,

    /// HTTP version (e.g., "HTTP/1.1", "HTTP/2")
    pub http_version: String,

    /// Request headers
    pub headers: Vec<HttpHeader>,

    /// Request body (if captured)
    pub body: Option<Vec<u8>>,

    /// Content-Type header value
    pub content_type: Option<String>,

    /// Size of the request body in bytes
    pub body_size: u64,

    /// Timestamp when request was received
    pub timestamp: DateTime<Utc>,
}

/// HTTP response captured by the proxy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpResponse {
    /// HTTP status code
    pub status_code: u16,

    /// Status reason phrase
    pub status_text: String,

    /// HTTP version
    pub http_version: String,

    /// Response headers
    pub headers: Vec<HttpHeader>,

    /// Response body (if captured)
    pub body: Option<Vec<u8>>,

    /// Content-Type header value
    pub content_type: Option<String>,

    /// Size of the response body in bytes
    pub body_size: u64,

    /// Timestamp when response was received
    pub timestamp: DateTime<Utc>,
}

/// A complete HTTP transaction (request + response)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpTransaction {
    /// Unique identifier for this transaction
    pub id: Uuid,

    /// OpenTelemetry trace ID
    pub trace_id: String,

    /// OpenTelemetry span ID
    pub span_id: String,

    /// Parent span ID (if this is a nested request)
    pub parent_span_id: Option<String>,

    /// The request
    pub request: HttpRequest,

    /// The response (None if still pending or failed)
    pub response: Option<HttpResponse>,

    /// Total duration in milliseconds
    pub duration_ms: Option<f64>,

    /// Time to first byte in milliseconds
    pub ttfb_ms: Option<f64>,

    /// Error message if the request failed
    pub error: Option<String>,

    /// Source application name (if known)
    pub source_app: Option<String>,

    /// Source process ID
    pub source_pid: Option<u32>,

    /// Tags/labels for filtering
    pub tags: Vec<String>,
}

/// Status of a transaction
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransactionStatus {
    /// Request is in progress
    Pending,
    /// Transaction completed successfully
    Complete,
    /// Transaction failed with an error
    Failed,
    /// Connection was aborted
    Aborted,
}

/// Summary of a remote host
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostSummary {
    /// Hostname
    pub host: String,

    /// Total number of requests
    pub request_count: u64,

    /// Number of failed requests
    pub error_count: u64,

    /// Average response time in ms
    pub avg_response_time_ms: f64,

    /// Total bytes sent
    pub bytes_sent: u64,

    /// Total bytes received
    pub bytes_received: u64,

    /// Last request timestamp
    pub last_seen: DateTime<Utc>,
}

/// Summary of a local application
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSummary {
    /// Application name
    pub name: String,

    /// Process ID
    pub pid: Option<u32>,

    /// Total number of requests
    pub request_count: u64,

    /// Number of failed requests
    pub error_count: u64,

    /// Last request timestamp
    pub last_seen: DateTime<Utc>,
}

impl HttpTransaction {
    /// Create a new transaction from a request
    pub fn new(request: HttpRequest, trace_id: String, span_id: String) -> Self {
        Self {
            id: request.id,
            trace_id,
            span_id,
            parent_span_id: None,
            request,
            response: None,
            duration_ms: None,
            ttfb_ms: None,
            error: None,
            source_app: None,
            source_pid: None,
            tags: Vec::new(),
        }
    }

    /// Get the status of this transaction
    pub fn status(&self) -> TransactionStatus {
        if self.error.is_some() {
            TransactionStatus::Failed
        } else if self.response.is_some() {
            TransactionStatus::Complete
        } else {
            TransactionStatus::Pending
        }
    }

    /// Check if this is an error response (4xx or 5xx)
    pub fn is_error(&self) -> bool {
        self.response
            .as_ref()
            .map(|r| r.status_code >= 400)
            .unwrap_or(false)
            || self.error.is_some()
    }
}

/// Protocol type for database/messaging connections
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProtocolType {
    /// PostgreSQL wire protocol
    PostgreSQL,
    /// Apache Kafka binary protocol
    Kafka,
    /// Unknown protocol
    Unknown,
}

impl std::fmt::Display for ProtocolType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProtocolType::PostgreSQL => write!(f, "postgresql"),
            ProtocolType::Kafka => write!(f, "kafka"),
            ProtocolType::Unknown => write!(f, "unknown"),
        }
    }
}

/// Database query record for PostgreSQL and other databases
///
/// Follows OpenTelemetry semantic conventions for database spans:
/// https://opentelemetry.io/docs/specs/semconv/database/
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseQueryRecord {
    /// Unique identifier for this query
    pub id: Uuid,

    /// OpenTelemetry trace ID
    pub trace_id: String,

    /// OpenTelemetry span ID
    pub span_id: String,

    /// Parent span ID (if part of a larger trace)
    pub parent_span_id: Option<String>,

    /// Timestamp when query started (milliseconds since epoch)
    pub timestamp: i64,

    /// Duration in milliseconds
    pub duration_ms: f64,

    /// Database system (e.g., "postgresql")
    pub db_system: String,

    /// Database name
    pub db_name: Option<String>,

    /// Database user
    pub db_user: Option<String>,

    /// Database operation (SELECT, INSERT, UPDATE, DELETE, etc.)
    pub db_operation: Option<String>,

    /// The SQL statement (sanitized/truncated for large queries)
    pub db_statement: String,

    /// Number of rows affected/returned
    pub db_rows_affected: Option<i64>,

    /// Server address (hostname or IP)
    pub server_address: String,

    /// Server port
    pub server_port: u16,

    /// Client address
    pub client_address: String,

    /// Whether the query succeeded
    pub success: bool,

    /// Error message if query failed
    pub error_message: Option<String>,

    /// Error code (e.g., SQLSTATE for PostgreSQL)
    pub error_code: Option<String>,

    /// Application name (if provided in connection)
    pub application_name: Option<String>,

    /// Additional attributes as JSON
    pub attributes: String,
}

impl DatabaseQueryRecord {
    /// Create a new database query record
    pub fn new(
        db_system: &str,
        db_statement: String,
        server_address: String,
        server_port: u16,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            trace_id: format!("{:032x}", rand::random::<u128>()),
            span_id: format!("{:016x}", rand::random::<u64>()),
            parent_span_id: None,
            timestamp: chrono::Utc::now().timestamp_millis(),
            duration_ms: 0.0,
            db_system: db_system.to_string(),
            db_name: None,
            db_user: None,
            db_operation: None,
            db_statement,
            db_rows_affected: None,
            server_address,
            server_port,
            client_address: String::new(),
            success: true,
            error_message: None,
            error_code: None,
            application_name: None,
            attributes: "{}".to_string(),
        }
    }

    /// Set the database name
    pub fn with_db_name(mut self, name: &str) -> Self {
        self.db_name = Some(name.to_string());
        self
    }

    /// Set the database user
    pub fn with_db_user(mut self, user: &str) -> Self {
        self.db_user = Some(user.to_string());
        self
    }

    /// Set the operation type
    pub fn with_operation(mut self, operation: &str) -> Self {
        self.db_operation = Some(operation.to_string());
        self
    }

    /// Mark as failed with error
    pub fn with_error(mut self, message: &str, code: Option<&str>) -> Self {
        self.success = false;
        self.error_message = Some(message.to_string());
        self.error_code = code.map(|c| c.to_string());
        self
    }
}

/// Kafka message operation type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KafkaOperation {
    /// Publishing/producing messages
    Publish,
    /// Receiving/consuming messages
    Receive,
    /// Processing (consumer group operations, etc.)
    Process,
}

impl std::fmt::Display for KafkaOperation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KafkaOperation::Publish => write!(f, "publish"),
            KafkaOperation::Receive => write!(f, "receive"),
            KafkaOperation::Process => write!(f, "process"),
        }
    }
}

/// Kafka message record for message tracing
///
/// Follows OpenTelemetry semantic conventions for messaging spans:
/// https://opentelemetry.io/docs/specs/semconv/messaging/
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KafkaMessageRecord {
    /// Unique identifier for this message/operation
    pub id: Uuid,

    /// OpenTelemetry trace ID
    pub trace_id: String,

    /// OpenTelemetry span ID
    pub span_id: String,

    /// Parent span ID (if part of a larger trace)
    pub parent_span_id: Option<String>,

    /// Timestamp when operation started (milliseconds since epoch)
    pub timestamp: i64,

    /// Duration in milliseconds
    pub duration_ms: f64,

    /// Messaging system identifier ("kafka")
    pub messaging_system: String,

    /// Kafka API operation name (e.g., "Produce", "Fetch", "JoinGroup")
    pub messaging_operation: String,

    /// Operation type (publish, receive, process)
    pub operation_type: KafkaOperation,

    /// Destination topic name(s) - comma separated if multiple
    pub messaging_destination: Option<String>,

    /// Consumer group ID (for consumer operations)
    pub messaging_consumer_group: Option<String>,

    /// Client ID
    pub messaging_client_id: Option<String>,

    /// Kafka API key (numeric)
    pub kafka_api_key: i16,

    /// Kafka API version
    pub kafka_api_version: i16,

    /// Correlation ID for request/response matching
    pub kafka_correlation_id: i32,

    /// Number of messages in batch (for produce/fetch)
    pub message_count: Option<i32>,

    /// Total payload size in bytes
    pub payload_size: Option<i64>,

    /// Server address (broker hostname or IP)
    pub server_address: String,

    /// Server port
    pub server_port: u16,

    /// Client address
    pub client_address: String,

    /// Whether the operation succeeded
    pub success: bool,

    /// Error code (Kafka error code)
    pub error_code: Option<i16>,

    /// Error message
    pub error_message: Option<String>,

    /// Additional attributes as JSON
    pub attributes: String,
}

impl KafkaMessageRecord {
    /// Create a new Kafka message record
    pub fn new(
        operation: &str,
        operation_type: KafkaOperation,
        api_key: i16,
        api_version: i16,
        correlation_id: i32,
        server_address: String,
        server_port: u16,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            trace_id: format!("{:032x}", rand::random::<u128>()),
            span_id: format!("{:016x}", rand::random::<u64>()),
            parent_span_id: None,
            timestamp: chrono::Utc::now().timestamp_millis(),
            duration_ms: 0.0,
            messaging_system: "kafka".to_string(),
            messaging_operation: operation.to_string(),
            operation_type,
            messaging_destination: None,
            messaging_consumer_group: None,
            messaging_client_id: None,
            kafka_api_key: api_key,
            kafka_api_version: api_version,
            kafka_correlation_id: correlation_id,
            message_count: None,
            payload_size: None,
            server_address,
            server_port,
            client_address: String::new(),
            success: true,
            error_code: None,
            error_message: None,
            attributes: "{}".to_string(),
        }
    }

    /// Set the destination topic(s)
    pub fn with_destination(mut self, topics: &[String]) -> Self {
        if !topics.is_empty() {
            self.messaging_destination = Some(topics.join(","));
        }
        self
    }

    /// Set the consumer group ID
    pub fn with_consumer_group(mut self, group_id: &str) -> Self {
        self.messaging_consumer_group = Some(group_id.to_string());
        self
    }

    /// Set the client ID
    pub fn with_client_id(mut self, client_id: &str) -> Self {
        self.messaging_client_id = Some(client_id.to_string());
        self
    }

    /// Mark as failed with error
    pub fn with_error(mut self, code: i16, message: Option<&str>) -> Self {
        self.success = false;
        self.error_code = Some(code);
        self.error_message = message.map(|m| m.to_string());
        self
    }
}

impl HttpRequest {
    /// Create a new request with a generated ID
    pub fn new(method: HttpMethod, url: String) -> Self {
        let parsed = url::Url::parse(&url).ok();
        let host = parsed
            .as_ref()
            .map(|u| u.host_str().unwrap_or("").to_string())
            .unwrap_or_default();
        let path = parsed
            .as_ref()
            .map(|u| u.path().to_string())
            .unwrap_or_default();
        let query = parsed.as_ref().and_then(|u| u.query().map(String::from));

        Self {
            id: Uuid::new_v4(),
            method,
            url,
            host,
            path,
            query,
            http_version: "HTTP/1.1".to_string(),
            headers: Vec::new(),
            body: None,
            content_type: None,
            body_size: 0,
            timestamp: Utc::now(),
        }
    }
}
