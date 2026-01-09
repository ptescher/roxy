//! MCP tools for Roxy observability and routing
//!
//! This module defines the tools exposed by the Roxy MCP server,
//! allowing AI agents to query network traffic and configure routing.

use anyhow::Result;
use rmcp::{
    model::{CallToolResult, ServerCapabilities, ServerInfo},
    schemars, tool, ServerHandler,
};
use roxy_core::{ClickHouseConfig, RoxyClickHouse};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Roxy MCP server handler
#[derive(Clone)]
pub struct RoxyMcpServer {
    clickhouse: Arc<RoxyClickHouse>,
}

impl RoxyMcpServer {
    /// Create a new MCP server instance, connecting to ClickHouse
    pub async fn new() -> Result<Self> {
        let config = ClickHouseConfig::default();
        let clickhouse = RoxyClickHouse::new(config);

        Ok(Self {
            clickhouse: Arc::new(clickhouse),
        })
    }

    fn text_result(text: String) -> CallToolResult {
        CallToolResult::success(vec![rmcp::model::Content::text(text)])
    }

    fn error_result(msg: String) -> rmcp::Error {
        rmcp::Error::internal_error(msg, None)
    }
}

/// Parameters for querying recent HTTP requests
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetRecentHttpRequestsParams {
    /// Maximum number of requests to return (default: 50, max: 500)
    #[serde(default = "default_limit")]
    pub limit: u32,

    /// Filter by host (optional, supports partial match)
    pub host: Option<String>,

    /// Filter by HTTP method (GET, POST, PUT, DELETE, etc.)
    pub method: Option<String>,

    /// Filter by minimum response status code
    pub min_status: Option<u16>,

    /// Filter by maximum response status code
    pub max_status: Option<u16>,
}

fn default_limit() -> u32 {
    50
}

/// Parameters for querying spans by trace ID
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetTraceSpansParams {
    /// The trace ID to look up
    pub trace_id: String,
}

/// Parameters for querying recent spans
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetRecentSpansParams {
    /// Maximum number of spans to return (default: 50, max: 500)
    #[serde(default = "default_limit")]
    pub limit: u32,

    /// Filter by service name (optional)
    pub service_name: Option<String>,
}

/// Parameters for getting a specific HTTP request by ID
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetRequestByIdParams {
    /// The request ID to look up
    pub id: String,
}

/// Parameters for querying recent database queries
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetRecentDatabaseQueriesParams {
    /// Maximum number of queries to return (default: 50, max: 500)
    #[serde(default = "default_limit")]
    pub limit: u32,

    /// Filter by database system (e.g., "postgresql", "mysql")
    pub db_system: Option<String>,
}

/// Parameters for querying recent Kafka messages
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetRecentKafkaMessagesParams {
    /// Maximum number of messages to return (default: 50, max: 500)
    #[serde(default = "default_limit")]
    pub limit: u32,

    /// Filter by topic name (optional, supports partial match)
    pub topic: Option<String>,
}

/// Parameters for searching HTTP requests
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SearchHttpRequestsParams {
    /// Search query (searches in URL, path, host, request/response bodies)
    pub query: String,

    /// Maximum number of results (default: 50, max: 500)
    #[serde(default = "default_limit")]
    pub limit: u32,
}

/// Parameters for getting host summary statistics
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetHostSummaryParams {
    /// Maximum number of hosts to return (default: 50)
    #[serde(default = "default_limit")]
    #[allow(dead_code)]
    pub limit: u32,
}

/// Summary of HTTP request for display
#[derive(Debug, Serialize)]
struct HttpRequestSummary {
    id: String,
    timestamp: i64,
    method: String,
    url: String,
    host: String,
    path: String,
    response_status: u16,
    duration_ms: f64,
    request_body_size: i64,
    response_body_size: i64,
    error: String,
}

#[tool(tool_box)]
impl RoxyMcpServer {
    /// Get recent HTTP requests captured by Roxy
    ///
    /// Returns a list of recent HTTP requests with their method, URL, status,
    /// duration, and other metadata. Use this to understand recent network
    /// traffic patterns.
    #[tool(
        name = "get_recent_http_requests",
        description = "Get recent HTTP requests captured by Roxy. Returns request metadata including method, URL, status code, duration, and body sizes."
    )]
    async fn get_recent_http_requests(
        &self,
        #[tool(aggr)] params: GetRecentHttpRequestsParams,
    ) -> Result<CallToolResult, rmcp::Error> {
        let limit = params.limit.min(500);

        let requests = self
            .clickhouse
            .get_recent_requests(limit)
            .await
            .map_err(|e| Self::error_result(e.to_string()))?;

        // Filter by host if specified
        let requests: Vec<_> = requests
            .into_iter()
            .filter(|r| {
                if let Some(ref host) = params.host {
                    r.host.contains(host)
                } else {
                    true
                }
            })
            .filter(|r| {
                if let Some(ref method) = params.method {
                    r.method.eq_ignore_ascii_case(method)
                } else {
                    true
                }
            })
            .filter(|r| {
                if let Some(min) = params.min_status {
                    r.response_status >= min
                } else {
                    true
                }
            })
            .filter(|r| {
                if let Some(max) = params.max_status {
                    r.response_status <= max
                } else {
                    true
                }
            })
            .map(|r| HttpRequestSummary {
                id: r.id,
                timestamp: r.timestamp,
                method: r.method,
                url: r.url,
                host: r.host,
                path: r.path,
                response_status: r.response_status,
                duration_ms: r.duration_ms,
                request_body_size: r.request_body_size,
                response_body_size: r.response_body_size,
                error: r.error,
            })
            .collect();

        let json = serde_json::to_string_pretty(&requests)
            .map_err(|e| Self::error_result(e.to_string()))?;

        Ok(Self::text_result(json))
    }

    /// Get all spans for a specific trace
    ///
    /// Returns the complete trace with all spans, useful for understanding
    /// the full request flow across services.
    #[tool(
        name = "get_trace_spans",
        description = "Get all spans for a specific trace ID. Returns the complete distributed trace showing the request flow across services."
    )]
    async fn get_trace_spans(
        &self,
        #[tool(aggr)] params: GetTraceSpansParams,
    ) -> Result<CallToolResult, rmcp::Error> {
        let spans = self
            .clickhouse
            .get_trace_spans(&params.trace_id)
            .await
            .map_err(|e| Self::error_result(e.to_string()))?;

        let json =
            serde_json::to_string_pretty(&spans).map_err(|e| Self::error_result(e.to_string()))?;

        Ok(Self::text_result(json))
    }

    /// Get recent spans from all services
    ///
    /// Returns recent OpenTelemetry spans, which represent individual
    /// operations within distributed traces.
    #[tool(
        name = "get_recent_spans",
        description = "Get recent OpenTelemetry spans. Spans represent individual operations in distributed traces, including timing and metadata."
    )]
    async fn get_recent_spans(
        &self,
        #[tool(aggr)] params: GetRecentSpansParams,
    ) -> Result<CallToolResult, rmcp::Error> {
        let limit = params.limit.min(500);

        let spans = self
            .clickhouse
            .get_recent_spans(limit)
            .await
            .map_err(|e| Self::error_result(e.to_string()))?;

        // Filter by service name if specified
        let spans: Vec<_> = if let Some(ref service) = params.service_name {
            spans
                .into_iter()
                .filter(|s| s.service_name.contains(service))
                .collect()
        } else {
            spans
        };

        let json =
            serde_json::to_string_pretty(&spans).map_err(|e| Self::error_result(e.to_string()))?;

        Ok(Self::text_result(json))
    }

    /// Get detailed information about a specific HTTP request
    ///
    /// Returns the full request and response details including headers,
    /// bodies, timing, and error information.
    #[tool(
        name = "get_request_by_id",
        description = "Get detailed information about a specific HTTP request by its ID. Returns full request/response details including headers and bodies."
    )]
    async fn get_request_by_id(
        &self,
        #[tool(aggr)] params: GetRequestByIdParams,
    ) -> Result<CallToolResult, rmcp::Error> {
        let request = self
            .clickhouse
            .get_request_by_id(&params.id)
            .await
            .map_err(|e| Self::error_result(e.to_string()))?;

        match request {
            Some(req) => {
                let json = serde_json::to_string_pretty(&req)
                    .map_err(|e| Self::error_result(e.to_string()))?;
                Ok(Self::text_result(json))
            }
            None => Ok(Self::text_result(format!(
                "No request found with ID: {}",
                params.id
            ))),
        }
    }

    /// Get recent database queries captured by Roxy
    ///
    /// Returns database queries (PostgreSQL, MySQL, etc.) intercepted by
    /// the SOCKS proxy, including query text and timing.
    #[tool(
        name = "get_recent_database_queries",
        description = "Get recent database queries captured by Roxy. Includes SQL statements, timing, and connection metadata for PostgreSQL and other databases."
    )]
    async fn get_recent_database_queries(
        &self,
        #[tool(aggr)] params: GetRecentDatabaseQueriesParams,
    ) -> Result<CallToolResult, rmcp::Error> {
        let limit = params.limit.min(500);

        let queries = if let Some(ref system) = params.db_system {
            self.clickhouse
                .get_database_queries_by_system(system, limit)
                .await
        } else {
            self.clickhouse.get_recent_database_queries(limit).await
        }
        .map_err(|e| Self::error_result(e.to_string()))?;

        let json = serde_json::to_string_pretty(&queries)
            .map_err(|e| Self::error_result(e.to_string()))?;

        Ok(Self::text_result(json))
    }

    /// Get recent Kafka messages captured by Roxy
    ///
    /// Returns Kafka produce/consume operations intercepted by the SOCKS proxy,
    /// including topic, operation type, and message counts.
    #[tool(
        name = "get_recent_kafka_messages",
        description = "Get recent Kafka messages captured by Roxy. Includes topic, operation type (produce/consume), and message metadata."
    )]
    async fn get_recent_kafka_messages(
        &self,
        #[tool(aggr)] params: GetRecentKafkaMessagesParams,
    ) -> Result<CallToolResult, rmcp::Error> {
        let limit = params.limit.min(500);

        let messages = if let Some(ref topic) = params.topic {
            self.clickhouse
                .get_kafka_messages_by_topic(topic, limit)
                .await
        } else {
            self.clickhouse.get_recent_kafka_messages(limit).await
        }
        .map_err(|e| Self::error_result(e.to_string()))?;

        let json = serde_json::to_string_pretty(&messages)
            .map_err(|e| Self::error_result(e.to_string()))?;

        Ok(Self::text_result(json))
    }

    /// Get summary statistics by host
    ///
    /// Returns aggregated statistics for each host, including request count,
    /// average duration, and error rate.
    #[tool(
        name = "get_host_summary",
        description = "Get summary statistics grouped by host. Returns request counts, average latency, and last seen timestamp for each host."
    )]
    async fn get_host_summary(
        &self,
        #[tool(aggr)] _params: GetHostSummaryParams,
    ) -> Result<CallToolResult, rmcp::Error> {
        let hosts = self
            .clickhouse
            .get_hosts()
            .await
            .map_err(|e| Self::error_result(e.to_string()))?;

        let json =
            serde_json::to_string_pretty(&hosts).map_err(|e| Self::error_result(e.to_string()))?;

        Ok(Self::text_result(json))
    }

    /// Search HTTP requests by query string
    ///
    /// Searches across URL, path, host, and body content to find matching requests.
    #[tool(
        name = "search_http_requests",
        description = "Search HTTP requests by query string. Searches across URL, path, host, and request/response bodies."
    )]
    async fn search_http_requests(
        &self,
        #[tool(aggr)] params: SearchHttpRequestsParams,
    ) -> Result<CallToolResult, rmcp::Error> {
        let limit = params.limit.min(500);

        let requests = self
            .clickhouse
            .search_requests(&params.query, limit)
            .await
            .map_err(|e| Self::error_result(e.to_string()))?;

        let summaries: Vec<_> = requests
            .into_iter()
            .map(|r| HttpRequestSummary {
                id: r.id,
                timestamp: r.timestamp,
                method: r.method,
                url: r.url,
                host: r.host,
                path: r.path,
                response_status: r.response_status,
                duration_ms: r.duration_ms,
                request_body_size: r.request_body_size,
                response_body_size: r.response_body_size,
                error: r.error,
            })
            .collect();

        let json = serde_json::to_string_pretty(&summaries)
            .map_err(|e| Self::error_result(e.to_string()))?;

        Ok(Self::text_result(json))
    }
}

#[tool(tool_box)]
impl ServerHandler for RoxyMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some(
                "Roxy MCP server provides access to network observability data. \
                 Query HTTP requests, distributed traces, database queries, and Kafka messages. \
                 Use get_recent_http_requests to see recent traffic, get_trace_spans to explore \
                 a specific trace, and search_http_requests to find specific requests."
                    .into(),
            ),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
        }
    }
}
