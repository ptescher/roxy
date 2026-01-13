//! Datadog integration for querying APM metrics, traces, and logs

use anyhow::Result;
use datadog_api_client::datadog::{APIKey, Configuration};
use rmcp::{model::CallToolResult, schemars, tool};
use roxy_core::{
    clickhouse::{DatadogRumResourceRecord, DatadogRumViewRecord, DatadogSpanRecord},
    RoxyClickHouse,
};
use serde::{Deserialize, Serialize};
use std::env;
use std::sync::Arc;

/// Datadog client wrapper
#[derive(Clone)]
pub struct DatadogClient {
    configuration: Configuration,
    clickhouse: Arc<RoxyClickHouse>,
}

impl DatadogClient {
    /// Create a new Datadog client using DD_API_KEY and DD_APP_KEY env vars
    pub fn new(clickhouse: Arc<RoxyClickHouse>) -> Result<Self> {
        let api_key = env::var("DD_API_KEY")
            .map_err(|_| anyhow::anyhow!("DD_API_KEY environment variable not set"))?;
        let app_key = env::var("DD_APP_KEY")
            .map_err(|_| anyhow::anyhow!("DD_APP_KEY environment variable not set"))?;

        let mut configuration = Configuration::new();
        configuration.set_auth_key(
            "apiKeyAuth",
            APIKey {
                key: api_key,
                prefix: String::new(),
            },
        );
        configuration.set_auth_key(
            "appKeyAuth",
            APIKey {
                key: app_key,
                prefix: String::new(),
            },
        );

        Ok(Self {
            configuration,
            clickhouse,
        })
    }

    pub fn text_result(text: String) -> CallToolResult {
        CallToolResult::success(vec![rmcp::model::Content::text(text)])
    }

    pub fn error_result(msg: String) -> rmcp::Error {
        rmcp::Error::internal_error(msg, None)
    }
}

/// Parameters for querying APM metrics
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct QueryApmMetricsParams {
    /// Service name to query (e.g., "backend", "history")
    pub service: String,

    /// Resource name pattern (e.g., "*history*", "POST /history/v2")
    pub resource_name: Option<String>,

    /// Time range in hours (default: 24)
    #[serde(default = "default_time_range")]
    pub time_range_hours: u32,

    /// Metric to query (default: "trace.web.request.duration")
    #[serde(default = "default_metric")]
    pub metric: String,
}

fn default_time_range() -> u32 {
    24
}

fn default_metric() -> String {
    "trace.web.request.duration".to_string()
}

/// Parameters for querying APM traces
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct QueryApmTracesParams {
    /// Service name to query
    pub service: String,

    /// Resource name pattern (optional)
    pub resource_name: Option<String>,

    /// Minimum duration in milliseconds (optional, for finding slow requests)
    pub min_duration_ms: Option<u64>,

    /// Maximum number of traces to return (default: 50, max: 100)
    #[serde(default = "default_trace_limit")]
    pub limit: u32,

    /// Time range in hours (default: 1)
    #[serde(default = "default_trace_time_range")]
    pub time_range_hours: u32,
}

fn default_trace_limit() -> u32 {
    50
}

fn default_trace_time_range() -> u32 {
    1
}

/// Parameters for querying logs
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct QueryLogsParams {
    /// Log query (e.g., "service:backend status:error")
    pub query: String,

    /// Time range in hours (default: 1)
    #[serde(default = "default_trace_time_range")]
    pub time_range_hours: u32,

    /// Maximum number of logs to return (default: 50)
    #[serde(default = "default_trace_limit")]
    pub limit: u32,
}

/// Parameters for querying RUM views
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct QueryRumViewsParams {
    /// Application ID (optional, filters by specific app)
    pub application_id: Option<String>,
    /// View name pattern (e.g., "home", "HomeTab", "*home*")
    pub view_name: Option<String>,
    /// Time range in hours (default: 24)
    #[serde(default = "default_time_range")]
    pub time_range_hours: u32,
    /// Maximum number of views to return (default: 50, max: 1000)
    #[serde(default = "default_trace_limit")]
    pub limit: u32,
}

/// Parameters for querying RUM resources (API calls made by the app)
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct QueryRumResourcesParams {
    /// Application ID (optional)
    pub application_id: Option<String>,
    /// View ID to filter resources (optional, gets resources for a specific view)
    pub view_id: Option<String>,
    /// Resource URL pattern (e.g., "*api.example.org*", "*portfolio*")
    pub url_pattern: Option<String>,
    /// Time range in hours (default: 1)
    #[serde(default = "default_trace_time_range")]
    pub time_range_hours: u32,
    /// Maximum number of resources to return (default: 100, max: 1000)
    #[serde(default = "default_rum_resource_limit")]
    pub limit: u32,
}

fn default_rum_resource_limit() -> u32 {
    100
}

/// Parameters for querying RUM sessions
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct QueryRumSessionsParams {
    /// Application ID (optional)
    pub application_id: Option<String>,
    /// Time range in hours (default: 24)
    #[serde(default = "default_time_range")]
    pub time_range_hours: u32,
    /// Maximum number of sessions to return (default: 50)
    #[serde(default = "default_trace_limit")]
    pub limit: u32,
}

/// Parameters for fetching and analyzing trace spans
#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FetchTraceSpansParams {
    /// Trace ID to fetch spans for (optional - if not provided, will search by service/resource)
    pub trace_id: Option<String>,
    /// Service name to search for traces (used when trace_id not provided)
    pub service: Option<String>,
    /// Resource name pattern to search (e.g., "GET /portfolio/v1/fungibles/balances")
    pub resource_name: Option<String>,
    /// Time range in hours to search (default: 1)
    #[serde(default = "default_trace_time_range")]
    pub time_range_hours: u32,
    /// Maximum number of spans to return (default: 100)
    #[serde(default = "default_rum_resource_limit")]
    pub limit: u32,
    /// Analyze spans for parallel vs sequential execution (default: true)
    #[serde(default = "default_true")]
    pub analyze_parallelism: bool,
    /// Extract SQL queries from spans (default: true)
    #[serde(default = "default_true")]
    pub extract_sql: bool,
}

fn default_true() -> bool {
    true
}

#[tool(tool_box)]
impl DatadogClient {
    /// Query APM metrics for a service
    ///
    /// Returns aggregated performance metrics like average duration, request count, etc.
    #[tool(
        name = "datadog_query_apm_metrics",
        description = "Query Datadog APM metrics for a service. Returns performance metrics like average duration, p95, p99, request counts."
    )]
    pub async fn query_apm_metrics(
        &self,
        #[tool(aggr)] params: QueryApmMetricsParams,
    ) -> Result<CallToolResult, rmcp::Error> {
        use datadog_api_client::datadogV1::api_metrics::MetricsAPI;

        let now = chrono::Utc::now().timestamp();
        let from = now - (params.time_range_hours as i64 * 3600);

        // Build query string
        let mut query = format!("avg:{}{{service:{}", params.metric, params.service);
        if let Some(ref resource) = params.resource_name {
            query.push_str(&format!(",resource_name:{}", resource));
        }
        query.push_str("}");

        let api = MetricsAPI::with_config(self.configuration.clone());

        let result = api
            .query_metrics(from, now, query)
            .await
            .map_err(|e| Self::error_result(format!("Datadog API error: {}", e)))?;

        let json =
            serde_json::to_string_pretty(&result).map_err(|e| Self::error_result(e.to_string()))?;

        Ok(Self::text_result(json))
    }

    /// Query APM traces to find slow requests
    ///
    /// Returns a link to Datadog APM UI with the appropriate query.
    #[tool(
        name = "datadog_query_apm_traces",
        description = "Get Datadog APM trace query link. Use this to investigate slow requests and view detailed trace spans."
    )]
    pub async fn query_apm_traces(
        &self,
        #[tool(aggr)] params: QueryApmTracesParams,
    ) -> Result<CallToolResult, rmcp::Error> {
        let mut query_parts = vec![format!("service:{}", params.service)];

        if let Some(ref resource) = params.resource_name {
            query_parts.push(format!("resource_name:{}", resource));
        }

        if let Some(min_dur) = params.min_duration_ms {
            query_parts.push(format!("@duration:>={}", min_dur * 1000000)); // nanoseconds
        }

        let query = query_parts.join(" ");
        let url = format!(
            "https://app.datadoghq.com/apm/traces?query={}&from_ts={}&to_ts={}&live=false",
            urlencoding::encode(&query),
            chrono::Utc::now().timestamp_millis() - (params.time_range_hours as i64 * 3600 * 1000),
            chrono::Utc::now().timestamp_millis()
        );

        let message = format!(
            "View traces in Datadog APM:\n{}\n\n\
            Query: {}\n\
            Time range: Last {} hour(s)\n\
            Limit: {} traces",
            url, query, params.time_range_hours, params.limit
        );

        Ok(Self::text_result(message))
    }

    /// Query Datadog logs
    ///
    /// Returns a link to Datadog Logs UI with the appropriate query.
    #[tool(
        name = "datadog_query_logs",
        description = "Get Datadog logs query link. Use this to search for errors, specific events, or patterns in application logs."
    )]
    pub async fn query_logs(
        &self,
        #[tool(aggr)] params: QueryLogsParams,
    ) -> Result<CallToolResult, rmcp::Error> {
        let url = format!(
            "https://app.datadoghq.com/logs?query={}&from_ts={}&to_ts={}&live=false",
            urlencoding::encode(&params.query),
            chrono::Utc::now().timestamp_millis() - (params.time_range_hours as i64 * 3600 * 1000),
            chrono::Utc::now().timestamp_millis()
        );

        let message = format!(
            "View logs in Datadog:\n{}\n\n\
            Query: {}\n\
            Time range: Last {} hour(s)\n\
            Limit: {} logs",
            url, params.query, params.time_range_hours, params.limit
        );

        Ok(Self::text_result(message))
    }

    /// Query RUM views to see user sessions and page loads
    #[tool(
        name = "datadog_query_rum_views",
        description = "Query Datadog RUM views (screens/pages). Returns actual view data including load times, view IDs, and performance metrics."
    )]
    pub async fn query_rum_views(
        &self,
        #[tool(aggr)] params: QueryRumViewsParams,
    ) -> Result<CallToolResult, rmcp::Error> {
        let mut query_parts = vec!["@type:view".to_string()];
        if let Some(ref app_id) = params.application_id {
            query_parts.push(format!("@application.id:{}", app_id));
        }
        if let Some(ref view_name) = params.view_name {
            query_parts.push(format!("@view.name:*{}*", view_name));
        }
        let query = query_parts.join(" ");

        // Use relative time format like "now-24h" which Datadog API prefers
        let from_time = format!("now-{}h", params.time_range_hours);
        let to_time = "now".to_string();

        // Also calculate millisecond timestamps for the URL
        let from_ms =
            chrono::Utc::now().timestamp_millis() - (params.time_range_hours as i64 * 3600 * 1000);
        let to_ms = chrono::Utc::now().timestamp_millis();

        // Make HTTP request to Datadog RUM Events API
        // Check if DD_SITE env var is set to use correct region
        let dd_site = std::env::var("DD_SITE").unwrap_or_else(|_| "datadoghq.com".to_string());
        let api_url = format!("https://api.{}/api/v2/rum/events/search", dd_site);

        let client = reqwest::Client::new();
        let response = client
            .post(&api_url)
            .header(
                "DD-API-KEY",
                std::env::var("DD_API_KEY")
                    .map_err(|_| Self::error_result("DD_API_KEY not set".to_string()))?,
            )
            .header(
                "DD-APPLICATION-KEY",
                std::env::var("DD_APP_KEY")
                    .map_err(|_| Self::error_result("DD_APP_KEY not set".to_string()))?,
            )
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .json(&serde_json::json!({
                "filter": {
                    "query": query,
                    "from": from_time,
                    "to": to_time
                },
                "options": {
                    "timezone": "UTC"
                },
                "page": {
                    "limit": params.limit
                },
                "sort": "timestamp"
            }))
            .send()
            .await
            .map_err(|e| Self::error_result(format!("Failed to fetch RUM views: {}", e)))?;

        let status = response.status();
        let response_text = response
            .text()
            .await
            .map_err(|e| Self::error_result(format!("Failed to read response: {}", e)))?;

        if !status.is_success() {
            return Err(Self::error_result(format!(
                "Datadog API error ({}): {}",
                status, response_text
            )));
        }

        let rum_data: serde_json::Value = serde_json::from_str(&response_text)
            .map_err(|e| Self::error_result(format!("Failed to parse response: {}", e)))?;

        // Log the raw response for debugging
        tracing::info!(
            "Datadog RUM API response: {}",
            serde_json::to_string_pretty(&rum_data).unwrap_or_else(|_| "unparseable".to_string())
        );

        // Extract and format view data
        let views = Self::extract_rum_views(&rum_data);

        // Store views in ClickHouse for local access
        if let Err(e) = self.store_rum_views_in_clickhouse(&views).await {
            tracing::warn!("Failed to store Datadog RUM views in ClickHouse: {}", e);
            // Don't fail the request if storage fails - just log it
        }

        let url = format!(
            "https://app.datadoghq.com/rum/explorer?query={}&from_ts={}&to_ts={}&live=false",
            urlencoding::encode(&query),
            from_ms,
            to_ms
        );

        let result = serde_json::json!({
            "query": query,
            "time_range_hours": params.time_range_hours,
            "view_count": views.len(),
            "views": views,
            "datadog_url": url,
            "debug_info": {
                "api_response_keys": rum_data.as_object().map(|obj| obj.keys().collect::<Vec<_>>()),
                "data_array_length": rum_data.get("data").and_then(|d| d.as_array()).map(|a| a.len()),
                "has_meta": rum_data.get("meta").is_some(),
                "has_errors": rum_data.get("errors").is_some(),
                "sample_event": rum_data.get("data").and_then(|d| d.as_array()).and_then(|a| a.first()).cloned()
            }
        });

        let json =
            serde_json::to_string_pretty(&result).map_err(|e| Self::error_result(e.to_string()))?;

        Ok(Self::text_result(json))
    }

    /// Extract RUM view information from API response
    fn extract_rum_views(rum_data: &serde_json::Value) -> Vec<serde_json::Value> {
        let events = match rum_data.get("data").and_then(|d| d.as_array()) {
            Some(e) => e,
            None => return Vec::new(),
        };

        events
            .iter()
            .filter_map(|event| {
                let attrs = event.get("attributes")?;
                // The actual view data is nested under attributes.attributes.view
                let inner_attrs = attrs.get("attributes")?;
                let view = inner_attrs.get("view")?;
                let session = inner_attrs.get("session")?;
                let application = inner_attrs.get("application")?;

                Some(serde_json::json!({
                    "view_id": view.get("id")?.as_str()?,
                    "view_name": view.get("name")?.as_str()?,
                    "view_url": view.get("url")?.as_str().unwrap_or(""),
                    "duration_ms": (view.get("time_spent")?.as_i64()? as f64) / 1_000_000.0,
                    "loading_time_ms": view.get("loading_time").and_then(|v| v.as_i64()).map(|t| t as f64 / 1_000_000.0),
                    "timestamp": attrs.get("timestamp")?.as_str()?,
                    "session_id": session.get("id")?.as_str()?,
                    "application_id": application.get("id")?.as_str()?,
                    "error_count": view.get("error")?.get("count").and_then(|v| v.as_i64()).unwrap_or(0),
                    "resource_count": view.get("resource")?.get("count").and_then(|v| v.as_i64()).unwrap_or(0)
                }))
            })
            .collect()
    }

    /// Query RUM resources (API calls, XHR, Fetch) made by the app
    #[tool(
        name = "datadog_query_rum_resources",
        description = "Query Datadog RUM resources (API calls/XHR/Fetch). Returns actual resource data including URLs, timing, status codes, and trace IDs."
    )]
    pub async fn query_rum_resources(
        &self,
        #[tool(aggr)] params: QueryRumResourcesParams,
    ) -> Result<CallToolResult, rmcp::Error> {
        let mut query_parts = vec!["@type:resource".to_string()];
        if let Some(ref app_id) = params.application_id {
            query_parts.push(format!("@application.id:{}", app_id));
        }
        if let Some(ref view_id) = params.view_id {
            query_parts.push(format!("@view.id:{}", view_id));
        }
        if let Some(ref url) = params.url_pattern {
            query_parts.push(format!("@resource.url:*{}*", url));
        }
        let query = query_parts.join(" ");

        // Use relative time format like "now-24h" which Datadog API prefers
        let from_time = format!("now-{}h", params.time_range_hours);
        let to_time = "now".to_string();

        // Also calculate millisecond timestamps for the URL
        let from_ms =
            chrono::Utc::now().timestamp_millis() - (params.time_range_hours as i64 * 3600 * 1000);
        let to_ms = chrono::Utc::now().timestamp_millis();

        // Make HTTP request to Datadog RUM Events API
        // Check if DD_SITE env var is set to use correct region
        let dd_site = std::env::var("DD_SITE").unwrap_or_else(|_| "datadoghq.com".to_string());
        let api_url = format!("https://api.{}/api/v2/rum/events/search", dd_site);

        let client = reqwest::Client::new();
        let response = client
            .post(&api_url)
            .header(
                "DD-API-KEY",
                std::env::var("DD_API_KEY")
                    .map_err(|_| Self::error_result("DD_API_KEY not set".to_string()))?,
            )
            .header(
                "DD-APPLICATION-KEY",
                std::env::var("DD_APP_KEY")
                    .map_err(|_| Self::error_result("DD_APP_KEY not set".to_string()))?,
            )
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .json(&serde_json::json!({
                "filter": {
                    "query": query,
                    "from": from_time,
                    "to": to_time
                },
                "options": {
                    "timezone": "UTC"
                },
                "page": {
                    "limit": params.limit
                },
                "sort": "timestamp"
            }))
            .send()
            .await
            .map_err(|e| Self::error_result(format!("Failed to fetch RUM resources: {}", e)))?;

        let status = response.status();
        let response_text = response
            .text()
            .await
            .map_err(|e| Self::error_result(format!("Failed to read response: {}", e)))?;

        if !status.is_success() {
            return Err(Self::error_result(format!(
                "Datadog API error ({}): {}",
                status, response_text
            )));
        }

        let rum_data: serde_json::Value = serde_json::from_str(&response_text)
            .map_err(|e| Self::error_result(format!("Failed to parse response: {}", e)))?;

        // Extract and format resource data
        let resources = Self::extract_rum_resources(&rum_data);

        // Store resources in ClickHouse for local access
        if let Err(e) = self.store_rum_resources_in_clickhouse(&resources).await {
            tracing::warn!("Failed to store Datadog RUM resources in ClickHouse: {}", e);
            // Don't fail the request if storage fails - just log it
        }

        let url = format!(
            "https://app.datadoghq.com/rum/explorer?query={}&from_ts={}&to_ts={}&live=false",
            urlencoding::encode(&query),
            from_ms,
            to_ms
        );

        let result = serde_json::json!({
            "query": query,
            "time_range_hours": params.time_range_hours,
            "resource_count": resources.len(),
            "resources": resources,
            "datadog_url": url
        });

        let json =
            serde_json::to_string_pretty(&result).map_err(|e| Self::error_result(e.to_string()))?;

        Ok(Self::text_result(json))
    }

    /// Extract RUM resource information from API response
    fn extract_rum_resources(rum_data: &serde_json::Value) -> Vec<serde_json::Value> {
        let events = match rum_data.get("data").and_then(|d| d.as_array()) {
            Some(e) => e,
            None => return Vec::new(),
        };

        events
            .iter()
            .filter_map(|event| {
                let attrs = event.get("attributes")?;
                // The actual resource data is nested under attributes.attributes.resource
                let inner_attrs = attrs.get("attributes")?;
                let resource = inner_attrs.get("resource")?;
                let view = inner_attrs.get("view")?;
                let session = inner_attrs.get("session")?;

                Some(serde_json::json!({
                    "resource_url": resource.get("url")?.as_str()?,
                    "resource_type": resource.get("type")?.as_str()?,
                    "method": resource.get("method").and_then(|v| v.as_str()).unwrap_or(""),
                    "status_code": resource.get("status_code").and_then(|v| v.as_i64()).unwrap_or(0),
                    "duration_ms": (resource.get("duration")?.as_i64()? as f64) / 1_000_000.0,
                    "size_bytes": resource.get("size").and_then(|v| v.as_i64()),
                    "timestamp": attrs.get("timestamp")?.as_str()?,
                    "view_id": view.get("id")?.as_str()?,
                    "view_name": view.get("name")?.as_str()?,
                    "session_id": session.get("id")?.as_str()?,
                    "trace_id": resource.get("trace_id").and_then(|v| v.as_str()),
                    "span_id": resource.get("span_id").and_then(|v| v.as_str())
                }))
            })
            .collect()
    }

    /// Query RUM sessions to see user journeys
    #[tool(
        name = "datadog_query_rum_sessions",
        description = "Query Datadog RUM sessions. Use this to see complete user journeys including all screens visited, actions taken, and errors encountered."
    )]
    pub async fn query_rum_sessions(
        &self,
        #[tool(aggr)] params: QueryRumSessionsParams,
    ) -> Result<CallToolResult, rmcp::Error> {
        let mut query_parts = vec!["@type:session".to_string()];
        if let Some(ref app_id) = params.application_id {
            query_parts.push(format!("@application.id:{}", app_id));
        }
        let query = query_parts.join(" ");
        let from_ts =
            chrono::Utc::now().timestamp_millis() - (params.time_range_hours as i64 * 3600 * 1000);
        let to_ts = chrono::Utc::now().timestamp_millis();
        let url = format!(
            "https://app.datadoghq.com/rum/explorer?query={}&from_ts={}&to_ts={}&live=false",
            urlencoding::encode(&query),
            from_ts,
            to_ts
        );
        let message = format!(
            "View RUM sessions in Datadog:\n{}\n\nQuery: {}\nTime range: Last {} hour(s)\nLimit: {} sessions\n\nTIP: Click on a session to see the complete user journey (Session Replay).",
            url, query, params.time_range_hours, params.limit
        );
        Ok(Self::text_result(message))
    }

    /// Fetch trace spans from Datadog to analyze parallelism and identify bottlenecks
    ///
    /// Returns actual span data with timing, parent relationships, and attributes (including SQL queries)
    pub async fn fetch_trace_spans(
        &self,
        params: FetchTraceSpansParams,
    ) -> Result<CallToolResult, rmcp::Error> {
        // Use Datadog Spans API v2 to fetch spans
        let from_ms =
            chrono::Utc::now().timestamp_millis() - (params.time_range_hours as i64 * 3600 * 1000);
        let to_ms = chrono::Utc::now().timestamp_millis();

        // Build query based on available parameters
        let query = if let Some(trace_id) = &params.trace_id {
            // Search by trace_id if provided
            format!("trace_id:{}", trace_id)
        } else if let Some(service) = &params.service {
            // Search by service and optionally resource_name
            let mut parts = vec![format!("service:{}", service)];
            if let Some(resource) = &params.resource_name {
                parts.push(format!("resource_name:\"{}\"", resource));
            }
            parts.join(" ")
        } else {
            return Err(Self::error_result(
                "Must provide either trace_id or service parameter".to_string(),
            ));
        };

        // Make HTTP request to Datadog Spans API v2
        // API requires data.attributes wrapper structure
        let client = reqwest::Client::new();
        let response = client
            .post("https://api.datadoghq.com/api/v2/spans/events/search")
            .header(
                "DD-API-KEY",
                std::env::var("DD_API_KEY")
                    .map_err(|_| Self::error_result("DD_API_KEY not set".to_string()))?,
            )
            .header(
                "DD-APPLICATION-KEY",
                std::env::var("DD_APP_KEY")
                    .map_err(|_| Self::error_result("DD_APP_KEY not set".to_string()))?,
            )
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .json(&serde_json::json!({
                "data": {
                    "attributes": {
                        "filter": {
                            "query": query,
                            "from": from_ms.to_string(),
                            "to": to_ms.to_string()
                        },
                        "page": {
                            "limit": params.limit
                        },
                        "sort": "-duration"  // Sort by duration descending to get slowest spans first
                    },
                    "type": "search_request"
                }
            }))
            .send()
            .await
            .map_err(|e| Self::error_result(format!("Failed to fetch spans: {}", e)))?;

        let status = response.status();
        let response_text = response
            .text()
            .await
            .map_err(|e| Self::error_result(format!("Failed to read response: {}", e)))?;

        if !status.is_success() {
            return Err(Self::error_result(format!(
                "Datadog API error ({}): {}",
                status, response_text
            )));
        }

        let spans_data: serde_json::Value = serde_json::from_str(&response_text)
            .map_err(|e| Self::error_result(format!("Failed to parse response: {}", e)))?;

        // If searching by service/resource, extract unique trace_ids for further analysis
        let trace_ids = if params.trace_id.is_none() {
            Self::extract_trace_ids(&spans_data)
        } else {
            Vec::new()
        };

        // Analyze spans for parallelism and SQL queries
        let analysis =
            Self::analyze_trace_spans(&spans_data, params.analyze_parallelism, params.extract_sql);

        // Store spans in ClickHouse for local access
        if let Err(e) = self.store_spans_in_clickhouse(&spans_data, &params).await {
            tracing::warn!("Failed to store Datadog spans in ClickHouse: {}", e);
            // Don't fail the request if storage fails - just log it
        }

        let result = serde_json::json!({
            "query": query,
            "trace_ids": trace_ids,
            "spans": spans_data,
            "analysis": analysis
        });

        let json =
            serde_json::to_string_pretty(&result).map_err(|e| Self::error_result(e.to_string()))?;

        Ok(Self::text_result(json))
    }

    /// Extract unique trace IDs from span search results
    fn extract_trace_ids(spans_data: &serde_json::Value) -> Vec<String> {
        let spans = match spans_data.get("data").and_then(|d| d.as_array()) {
            Some(s) => s,
            None => return Vec::new(),
        };

        let mut trace_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
        for span in spans {
            if let Some(trace_id) = span
                .get("attributes")
                .and_then(|a| a.get("trace_id"))
                .and_then(|t| t.as_str())
            {
                trace_ids.insert(trace_id.to_string());
            }
        }

        trace_ids.into_iter().collect()
    }

    /// Analyze trace spans to detect parallelism and extract SQL queries
    fn analyze_trace_spans(
        spans_data: &serde_json::Value,
        analyze_parallelism: bool,
        extract_sql: bool,
    ) -> serde_json::Value {
        let spans = match spans_data.get("data").and_then(|d| d.as_array()) {
            Some(s) => s,
            None => return serde_json::json!({"error": "No spans found in response"}),
        };

        let mut analysis = serde_json::Map::new();

        // Extract span information
        let mut span_info: Vec<serde_json::Value> = spans
            .iter()
            .filter_map(|span| {
                let attrs = span.get("attributes")?;
                Some(serde_json::json!({
                    "span_id": attrs.get("span_id")?.as_str()?,
                    "parent_id": attrs.get("parent_id").and_then(|v| v.as_str()).unwrap_or(""),
                    "name": attrs.get("resource_name")?.as_str()?,
                    "service": attrs.get("service")?.as_str()?,
                    "start_ns": attrs.get("start")?.as_i64()?,
                    "duration_ns": attrs.get("duration")?.as_i64()?,
                    "attributes": attrs.get("attributes").cloned().unwrap_or(serde_json::json!({}))
                }))
            })
            .collect();

        // Sort by start time
        span_info.sort_by_key(|s| s.get("start_ns").and_then(|v| v.as_i64()).unwrap_or(0));

        analysis.insert("span_count".to_string(), serde_json::json!(span_info.len()));

        // Parallelism analysis
        if analyze_parallelism && !span_info.is_empty() {
            let parallelism = Self::detect_parallelism(&span_info);
            analysis.insert("parallelism".to_string(), parallelism);
        }

        // SQL extraction
        if extract_sql {
            let sql_queries = Self::extract_sql_queries(&span_info);
            analysis.insert("sql_queries".to_string(), sql_queries);
        }

        // Calculate total trace duration
        if let (Some(first), Some(last)) = (span_info.first(), span_info.last()) {
            if let (Some(start), Some(last_start), Some(last_dur)) = (
                first.get("start_ns").and_then(|v| v.as_i64()),
                last.get("start_ns").and_then(|v| v.as_i64()),
                last.get("duration_ns").and_then(|v| v.as_i64()),
            ) {
                let total_duration_ms = ((last_start + last_dur - start) as f64) / 1_000_000.0;
                analysis.insert(
                    "total_duration_ms".to_string(),
                    serde_json::json!(total_duration_ms),
                );
            }
        }

        analysis.insert("spans".to_string(), serde_json::json!(span_info));

        serde_json::Value::Object(analysis)
    }

    /// Detect parallel vs sequential execution patterns
    fn detect_parallelism(spans: &[serde_json::Value]) -> serde_json::Value {
        let mut parallel_groups: Vec<Vec<String>> = Vec::new();
        let mut sequential_chains: Vec<Vec<String>> = Vec::new();

        // Build a map of parent_id -> children
        let mut children_map: std::collections::HashMap<String, Vec<usize>> =
            std::collections::HashMap::new();
        for (idx, span) in spans.iter().enumerate() {
            if let Some(parent_id) = span.get("parent_id").and_then(|v| v.as_str()) {
                if !parent_id.is_empty() {
                    children_map
                        .entry(parent_id.to_string())
                        .or_insert_with(Vec::new)
                        .push(idx);
                }
            }
        }

        // Detect parallel execution: spans with same parent that overlap in time
        for (_parent_id, children_indices) in children_map.iter() {
            if children_indices.len() > 1 {
                // Check if these children overlap in time (parallel execution)
                let mut overlapping = true;
                for i in 0..children_indices.len() - 1 {
                    let span1 = &spans[children_indices[i]];
                    let span2 = &spans[children_indices[i + 1]];

                    if let (Some(start1), Some(dur1), Some(start2)) = (
                        span1.get("start_ns").and_then(|v| v.as_i64()),
                        span1.get("duration_ns").and_then(|v| v.as_i64()),
                        span2.get("start_ns").and_then(|v| v.as_i64()),
                    ) {
                        let end1 = start1 + dur1;
                        // If span2 starts after span1 ends, they're sequential
                        if start2 >= end1 {
                            overlapping = false;
                            break;
                        }
                    }
                }

                let span_names: Vec<String> = children_indices
                    .iter()
                    .filter_map(|&idx| {
                        spans[idx]
                            .get("name")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string())
                    })
                    .collect();

                if overlapping && span_names.len() > 1 {
                    parallel_groups.push(span_names);
                } else if !overlapping && span_names.len() > 1 {
                    sequential_chains.push(span_names);
                }
            }
        }

        serde_json::json!({
            "parallel_groups": parallel_groups,
            "sequential_chains": sequential_chains,
            "has_parallelism": !parallel_groups.is_empty(),
            "optimization_opportunity": if !sequential_chains.is_empty() {
                "Sequential operations detected that could potentially be parallelized"
            } else {
                "No obvious sequential operations detected"
            }
        })
    }

    /// Store Datadog spans in ClickHouse for local analysis
    async fn store_spans_in_clickhouse(
        &self,
        spans_data: &serde_json::Value,
        params: &FetchTraceSpansParams,
    ) -> Result<()> {
        let events = match spans_data.get("data").and_then(|d| d.as_array()) {
            Some(e) if !e.is_empty() => e,
            _ => return Ok(()), // No spans to store
        };

        let fetched_at = chrono::Utc::now().timestamp_millis();
        let query_params = serde_json::to_string(&params)?;

        let mut records = Vec::new();

        for event in events {
            let attrs = match event.get("attributes") {
                Some(a) => a,
                None => continue,
            };

            // Extract required fields
            let trace_id = attrs
                .get("trace_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let span_id = attrs
                .get("span_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let parent_id = attrs
                .get("parent_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let service = attrs
                .get("service")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let resource_name = attrs
                .get("resource_name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let operation_name = attrs
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let start_ns = attrs.get("start").and_then(|v| v.as_i64()).unwrap_or(0);
            let duration_ns = attrs.get("duration").and_then(|v| v.as_i64()).unwrap_or(0);
            let status = attrs
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            let error = attrs.get("error").and_then(|v| v.as_u64()).unwrap_or(0) as u8;
            let span_type = attrs
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            // Extract tags, meta, and metrics as JSON strings
            let tags = attrs
                .get("tags")
                .map(|v| serde_json::to_string(v).unwrap_or_default())
                .unwrap_or_else(|| "{}".to_string());
            let meta = attrs
                .get("meta")
                .map(|v| serde_json::to_string(v).unwrap_or_default())
                .unwrap_or_else(|| "{}".to_string());
            let metrics = attrs
                .get("metrics")
                .map(|v| serde_json::to_string(v).unwrap_or_default())
                .unwrap_or_else(|| "{}".to_string());

            // Store the raw span JSON
            let raw_span_json = serde_json::to_string(event).unwrap_or_default();

            let record = DatadogSpanRecord {
                id: uuid::Uuid::new_v4().to_string(),
                trace_id,
                span_id,
                parent_id,
                fetched_at,
                query_params: query_params.clone(),
                service,
                resource_name,
                operation_name,
                start_ns,
                duration_ns,
                status,
                error,
                span_type,
                tags,
                meta,
                metrics,
                raw_span_json,
            };

            records.push(record);
        }

        if !records.is_empty() {
            self.clickhouse.insert_datadog_spans(&records).await?;
            tracing::info!("Stored {} Datadog spans in ClickHouse", records.len());
        }

        Ok(())
    }

    /// Store Datadog RUM views in ClickHouse
    async fn store_rum_views_in_clickhouse(&self, views: &[serde_json::Value]) -> Result<()> {
        if views.is_empty() {
            return Ok(());
        }

        let fetched_at = chrono::Utc::now().timestamp_millis();
        let mut records = Vec::new();

        for view in views {
            let record = DatadogRumViewRecord {
                id: uuid::Uuid::new_v4().to_string(),
                view_id: view
                    .get("view_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                session_id: view
                    .get("session_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                application_id: view
                    .get("application_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                fetched_at,
                timestamp: view
                    .get("timestamp")
                    .and_then(|v| v.as_str())
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                    .map(|dt| dt.timestamp_millis())
                    .unwrap_or(fetched_at),
                view_name: view
                    .get("view_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                view_url: view
                    .get("view_url")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                duration_ms: view
                    .get("duration_ms")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0),
                loading_time_ms: view
                    .get("loading_time_ms")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0),
                error_count: view
                    .get("error_count")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0),
                resource_count: view
                    .get("resource_count")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0),
                action_count: view
                    .get("action_count")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0),
                long_task_count: view
                    .get("long_task_count")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0),
                user_id: view
                    .get("user_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                user_email: view
                    .get("user_email")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                user_name: view
                    .get("user_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                device_type: view
                    .get("device_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                os_name: view
                    .get("os_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                os_version: view
                    .get("os_version")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                browser_name: view
                    .get("browser_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                browser_version: view
                    .get("browser_version")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                country: view
                    .get("country")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                city: view
                    .get("city")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                raw_view_json: serde_json::to_string(view).unwrap_or_default(),
            };

            records.push(record);
        }

        if !records.is_empty() {
            self.clickhouse.insert_datadog_rum_views(&records).await?;
            tracing::info!("Stored {} Datadog RUM views in ClickHouse", records.len());
        }

        Ok(())
    }

    /// Store Datadog RUM resources in ClickHouse
    async fn store_rum_resources_in_clickhouse(
        &self,
        resources: &[serde_json::Value],
    ) -> Result<()> {
        if resources.is_empty() {
            return Ok(());
        }

        let fetched_at = chrono::Utc::now().timestamp_millis();
        let mut records = Vec::new();

        for resource in resources {
            let record = DatadogRumResourceRecord {
                id: uuid::Uuid::new_v4().to_string(),
                resource_id: resource
                    .get("resource_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                view_id: resource
                    .get("view_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                session_id: resource
                    .get("session_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                application_id: resource
                    .get("application_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                fetched_at,
                timestamp: resource
                    .get("timestamp")
                    .and_then(|v| v.as_str())
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                    .map(|dt| dt.timestamp_millis())
                    .unwrap_or(fetched_at),
                resource_url: resource
                    .get("resource_url")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                resource_type: resource
                    .get("resource_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                method: resource
                    .get("method")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                status_code: resource
                    .get("status_code")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0),
                duration_ms: resource
                    .get("duration_ms")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0),
                size_bytes: resource
                    .get("size_bytes")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0),
                trace_id: resource
                    .get("trace_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                span_id: resource
                    .get("span_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                dns_duration_ms: resource
                    .get("dns_duration_ms")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0),
                connect_duration_ms: resource
                    .get("connect_duration_ms")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0),
                ssl_duration_ms: resource
                    .get("ssl_duration_ms")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0),
                download_duration_ms: resource
                    .get("download_duration_ms")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0),
                first_byte_duration_ms: resource
                    .get("first_byte_duration_ms")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0),
                provider_name: resource
                    .get("provider_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                provider_type: resource
                    .get("provider_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                raw_resource_json: serde_json::to_string(resource).unwrap_or_default(),
            };

            records.push(record);
        }

        if !records.is_empty() {
            self.clickhouse
                .insert_datadog_rum_resources(&records)
                .await?;
            tracing::info!(
                "Stored {} Datadog RUM resources in ClickHouse",
                records.len()
            );
        }

        Ok(())
    }

    /// Extract SQL queries from span attributes
    fn extract_sql_queries(spans: &[serde_json::Value]) -> serde_json::Value {
        let sql_spans: Vec<serde_json::Value> = spans.iter().filter_map(|span| {
            let attrs = span.get("attributes")?;
            let sql = attrs.get("db.statement")
                .or_else(|| attrs.get("sql.query"))
                .or_else(|| attrs.get("query"))?
                .as_str()?;

            Some(serde_json::json!({
                "span_name": span.get("name")?.as_str()?,
                "service": span.get("service")?.as_str()?,
                "duration_ms": (span.get("duration_ns")?.as_i64()? as f64) / 1_000_000.0,
                "sql": sql,
                "db_system": attrs.get("db.system").and_then(|v| v.as_str()).unwrap_or("unknown"),
                "db_name": attrs.get("db.name").and_then(|v| v.as_str())
            }))
        }).collect();

        // Sort by duration (slowest first)
        let mut sorted_sql = sql_spans;
        sorted_sql.sort_by(|a, b| {
            let dur_a = a.get("duration_ms").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let dur_b = b.get("duration_ms").and_then(|v| v.as_f64()).unwrap_or(0.0);
            dur_b
                .partial_cmp(&dur_a)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        serde_json::json!({
            "count": sorted_sql.len(),
            "queries": sorted_sql
        })
    }
}
