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
