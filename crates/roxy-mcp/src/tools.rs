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
    datadog: Option<Arc<crate::datadog::DatadogClient>>,
}

impl RoxyMcpServer {
    /// Create a new MCP server instance, connecting to ClickHouse
    pub async fn new() -> Result<Self> {
        let config = ClickHouseConfig::default();
        let clickhouse = Arc::new(RoxyClickHouse::new(config));

        // Try to initialize Datadog client if credentials are available
        let datadog = match crate::datadog::DatadogClient::new(clickhouse.clone()) {
            Ok(client) => {
                tracing::info!("Datadog integration enabled");
                Some(Arc::new(client))
            }
            Err(e) => {
                tracing::warn!("Datadog integration disabled: {}", e);
                None
            }
        };

        Ok(Self {
            clickhouse,
            datadog,
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

    // Datadog tools (only available if DD_API_KEY and DD_APP_KEY are set)

    /// Query Datadog APM metrics
    #[tool(
        name = "datadog_query_apm_metrics",
        description = "Query Datadog APM metrics for a service. Returns performance metrics like average duration, p95, p99."
    )]
    async fn datadog_query_apm_metrics(
        &self,
        #[tool(aggr)] params: QueryApmMetricsParams,
    ) -> Result<CallToolResult, rmcp::Error> {
        let client = self.datadog.as_ref().ok_or_else(|| {
            Self::error_result(
                "Datadog not configured. Set DD_API_KEY and DD_APP_KEY environment variables."
                    .to_string(),
            )
        })?;

        client.query_apm_metrics(params).await
    }

    /// Query Datadog APM traces
    #[tool(
        name = "datadog_query_apm_traces",
        description = "Query Datadog APM traces to find slow requests. Returns a Datadog URL showing backend trace spans. WORKFLOW STEP 3: Use resource_name from RUM resources (e.g., 'GET /portfolio/v1/balances') to investigate backend performance. Filter by min_duration_ms to find slow requests. Analyze span hierarchy to identify bottlenecks (DB queries, external APIs, etc)."
    )]
    async fn datadog_query_apm_traces(
        &self,
        #[tool(aggr)] params: QueryApmTracesParams,
    ) -> Result<CallToolResult, rmcp::Error> {
        let client = self.datadog.as_ref().ok_or_else(|| {
            Self::error_result(
                "Datadog not configured. Set DD_API_KEY and DD_APP_KEY environment variables."
                    .to_string(),
            )
        })?;

        client.query_apm_traces(params).await
    }

    /// Query Datadog logs
    #[tool(
        name = "datadog_query_logs",
        description = "Query Datadog logs for errors and events."
    )]
    async fn datadog_query_logs(
        &self,
        #[tool(aggr)] params: QueryLogsParams,
    ) -> Result<CallToolResult, rmcp::Error> {
        let client = self.datadog.as_ref().ok_or_else(|| {
            Self::error_result(
                "Datadog not configured. Set DD_API_KEY and DD_APP_KEY environment variables."
                    .to_string(),
            )
        })?;

        client.query_logs(params).await
    }

    /// Query Datadog RUM views
    #[tool(
        name = "datadog_query_rum_views",
        description = "Query Datadog RUM views (screens/pages). Returns a Datadog URL showing mobile app screen performance. WORKFLOW STEP 1: Use this first to identify slow screens and get view IDs. Then use datadog_query_rum_resources with the view_id to see what API calls are made."
    )]
    async fn datadog_query_rum_views(
        &self,
        #[tool(aggr)] params: QueryRumViewsParams,
    ) -> Result<CallToolResult, rmcp::Error> {
        let client = self.datadog.as_ref().ok_or_else(|| {
            Self::error_result(
                "Datadog not configured. Set DD_API_KEY and DD_APP_KEY environment variables."
                    .to_string(),
            )
        })?;

        client.query_rum_views(params).await
    }

    /// Query Datadog RUM resources
    #[tool(
        name = "datadog_query_rum_resources",
        description = "Query Datadog RUM resources (API calls/XHR/Fetch). Returns a Datadog URL showing HTTP requests made by the mobile app. WORKFLOW STEP 2: Use view_id from datadog_query_rum_views to filter resources for a specific screen. Group by @resource.url in Datadog UI to find slow/frequent endpoints. Then use datadog_query_apm_traces to investigate backend performance."
    )]
    async fn datadog_query_rum_resources(
        &self,
        #[tool(aggr)] params: QueryRumResourcesParams,
    ) -> Result<CallToolResult, rmcp::Error> {
        let client = self.datadog.as_ref().ok_or_else(|| {
            Self::error_result(
                "Datadog not configured. Set DD_API_KEY and DD_APP_KEY environment variables."
                    .to_string(),
            )
        })?;

        client.query_rum_resources(params).await
    }

    /// Query Datadog RUM sessions
    #[tool(
        name = "datadog_query_rum_sessions",
        description = "Query Datadog RUM sessions. Use this to see complete user journeys."
    )]
    async fn datadog_query_rum_sessions(
        &self,
        #[tool(aggr)] params: QueryRumSessionsParams,
    ) -> Result<CallToolResult, rmcp::Error> {
        let client = self.datadog.as_ref().ok_or_else(|| {
            Self::error_result(
                "Datadog not configured. Set DD_API_KEY and DD_APP_KEY environment variables."
                    .to_string(),
            )
        })?;

        client.query_rum_sessions(params).await
    }

    /// Fetch and analyze trace spans from Datadog Spans API
    #[tool(
        name = "datadog_fetch_trace_spans",
        description = "Fetch spans using Datadog Spans API v2 (NOT Traces API). Can search by trace_id OR by service+resource_name. CRITICAL for performance investigation: detects if operations run in parallel or sequential (identify parallelization opportunities), extracts SQL queries with timing (find slow queries), shows full span waterfall. WORKFLOW: 1) If you have trace_id from RUM, use it directly. 2) If no trace_id, search by service + resource_name to find matching traces."
    )]
    async fn datadog_fetch_trace_spans(
        &self,
        #[tool(aggr)] params: FetchTraceSpansParams,
    ) -> Result<CallToolResult, rmcp::Error> {
        let client = self.datadog.as_ref().ok_or_else(|| {
            Self::error_result(
                "Datadog not configured. Set DD_API_KEY and DD_APP_KEY environment variables."
                    .to_string(),
            )
        })?;

        client.fetch_trace_spans(params).await
    }
}

#[tool(tool_box)]
impl ServerHandler for RoxyMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some(
                "Roxy MCP server provides access to network observability data and Datadog telemetry.\n\n\
                 == Roxy Network Observability ==\n\
                 - get_recent_http_requests: See HTTP traffic captured by Roxy proxy\n\
                 - get_trace_spans: View distributed traces across services\n\
                 - get_recent_database_queries: Inspect database queries\n\
                 - get_recent_kafka_messages: View Kafka message traffic\n\
                 - search_http_requests: Search across request/response bodies\n\n\
                 == Datadog RUM (Real User Monitoring) Workflow ==\n\
                 Use this workflow to investigate mobile app performance:\n\n\
                 1. datadog_query_rum_views - Find slow screens\n\
                    • Query for specific view names (e.g., view_name=\"Home\")\n\
                    • Returns Datadog URL with view performance data\n\
                    • Note the view_id from the results\n\n\
                 2. datadog_query_rum_resources - See API calls for that view\n\
                    • Use the view_id from step 1\n\
                    • Shows all API requests made during screen load\n\
                    • Group by @resource.url in Datadog UI to see aggregated stats\n\
                    • Identify slow or frequently called endpoints\n\n\
                 3. datadog_fetch_trace_spans - Deep dive into trace execution ⭐ NEW\n\
                    • Use trace_id from RUM resources OR search by service+resource_name\n\
                    • Detects parallel vs sequential execution patterns\n\
                    • Extracts SQL queries with timing (find slow queries)\n\
                    • Shows complete span waterfall with parent/child relationships\n\n\
                 Example: \"Home tab is slow\" investigation:\n\
                 - Query RUM views for \"Home\" → get view_id\n\
                 - Query RUM resources with that view_id → find slow API endpoints with trace_ids\n\
                 - Fetch trace spans by trace_id OR service+resource_name → analyze parallelism and SQL\n\n\
                 == Datadog Spans API (Performance Deep Dive) ==\n\
                 - datadog_fetch_trace_spans: Fetch spans using Spans API v2 (NOT Traces API)\n\
                   • With trace_id: trace_id=\"abc123\" (from RUM resources)\n\
                   • Without trace_id: service=\"backend\" + resource_name=\"GET /portfolio/v1/fungibles/balances\"\n\
                   • Returns: span hierarchy, parallelism analysis, SQL queries, timing waterfall\n\n\
                 == Datadog APM & Logs ==\n\
                 - datadog_query_apm_metrics: Performance metrics (p95, avg latency)\n\
                 - datadog_query_apm_traces: Get trace query URL for Datadog UI\n\
                 - datadog_query_logs: Search logs for errors and events\n\n\
                 Most Datadog tools return URLs to the Datadog UI. datadog_fetch_trace_spans returns actual data."
                    .into(),
            ),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
        }
    }
}

// Import Datadog types for the tool parameters
use crate::datadog::{
    FetchTraceSpansParams, QueryApmMetricsParams, QueryApmTracesParams, QueryLogsParams,
    QueryRumResourcesParams, QueryRumSessionsParams, QueryRumViewsParams,
};
