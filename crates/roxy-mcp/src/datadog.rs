//! Datadog integration for querying APM metrics, traces, and logs

use anyhow::Result;
use datadog_api_client::datadog::{Configuration, APIKey};
use rmcp::{
    model::CallToolResult,
    schemars, tool,
};
use serde::{Deserialize, Serialize};
use std::env;

/// Datadog client wrapper
#[derive(Clone)]
pub struct DatadogClient {
    configuration: Configuration,
}

impl DatadogClient {
    /// Create a new Datadog client using DD_API_KEY and DD_APP_KEY env vars
    pub fn new() -> Result<Self> {
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

        Ok(Self { configuration })
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
        
        let result = api.query_metrics(from, now, query)
            .await
            .map_err(|e| Self::error_result(format!("Datadog API error: {}", e)))?;

        let json = serde_json::to_string_pretty(&result)
            .map_err(|e| Self::error_result(e.to_string()))?;

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
            url,
            query,
            params.time_range_hours,
            params.limit
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
            url,
            params.query,
            params.time_range_hours,
            params.limit
        );

        Ok(Self::text_result(message))
    }
}
