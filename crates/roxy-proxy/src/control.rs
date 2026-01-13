//! Control API for runtime configuration
//!
//! This module provides a simple HTTP API for controlling the proxy
//! at runtime, such as enabling/disabling features.

use http_body_util::BodyExt;
use hyper::{
    body::Incoming, server::conn::http1, service::service_fn, Method, Request, Response, StatusCode,
};
use hyper_util::rt::TokioIo;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::net::TcpListener;
use tracing::{error, info};

/// Default control API port
pub const DEFAULT_CONTROL_PORT: u16 = 8889;

/// Control command for the proxy
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum ControlCommand {
    /// Enable or disable auto port-forwarding
    SetAutoPortForward { enabled: bool },
    /// Get current status
    GetStatus,
}

/// Response from control API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlResponse {
    pub success: bool,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl ControlResponse {
    fn success(message: impl Into<String>) -> Self {
        Self {
            success: true,
            message: message.into(),
            data: None,
        }
    }

    fn success_with_data(message: impl Into<String>, data: serde_json::Value) -> Self {
        Self {
            success: true,
            message: message.into(),
            data: Some(data),
        }
    }

    fn error(message: impl Into<String>) -> Self {
        Self {
            success: false,
            message: message.into(),
            data: None,
        }
    }
}

/// Control server state
pub struct ControlServer {
    port: u16,
    auto_port_forward_enabled: Arc<AtomicBool>,
}

impl ControlServer {
    /// Create a new control server
    pub fn new(port: u16, auto_port_forward_enabled: Arc<AtomicBool>) -> Self {
        Self {
            port,
            auto_port_forward_enabled,
        }
    }

    /// Run the control server
    pub async fn run(self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let addr = SocketAddr::from(([127, 0, 0, 1], self.port));
        let listener = TcpListener::bind(addr).await?;

        info!("Control API listening on http://{}", addr);

        let state = Arc::new(self);

        loop {
            let (stream, _) = listener.accept().await?;
            let io = TokioIo::new(stream);
            let state = state.clone();

            tokio::spawn(async move {
                let service = service_fn(move |req| {
                    let state = state.clone();
                    async move { handle_request(req, state).await }
                });

                if let Err(e) = http1::Builder::new().serve_connection(io, service).await {
                    error!("Error serving connection: {}", e);
                }
            });
        }
    }
}

/// Handle a control API request
async fn handle_request(
    req: Request<Incoming>,
    state: Arc<ControlServer>,
) -> Result<Response<String>, hyper::Error> {
    let (parts, body) = req.into_parts();

    match (parts.method, parts.uri.path()) {
        (Method::POST, "/control") => {
            // Read body
            let body_bytes = match body.collect().await {
                Ok(collected) => collected.to_bytes(),
                Err(e) => {
                    let response = ControlResponse::error(format!("Failed to read body: {}", e));
                    return Ok(json_response(StatusCode::BAD_REQUEST, response));
                }
            };

            // Parse command
            let command: ControlCommand = match serde_json::from_slice(&body_bytes) {
                Ok(cmd) => cmd,
                Err(e) => {
                    let response = ControlResponse::error(format!("Invalid JSON: {}", e));
                    return Ok(json_response(StatusCode::BAD_REQUEST, response));
                }
            };

            // Handle command
            let response = handle_command(command, &state);
            Ok(json_response(StatusCode::OK, response))
        }
        (Method::GET, "/health") => {
            let response = ControlResponse::success("OK");
            Ok(json_response(StatusCode::OK, response))
        }
        (Method::GET, "/status") => {
            let auto_port_forward_enabled = state.auto_port_forward_enabled.load(Ordering::Relaxed);

            let data = serde_json::json!({
                "auto_port_forward_enabled": auto_port_forward_enabled,
            });

            let response = ControlResponse::success_with_data("Status retrieved", data);
            Ok(json_response(StatusCode::OK, response))
        }
        _ => {
            let response = ControlResponse::error("Not found");
            Ok(json_response(StatusCode::NOT_FOUND, response))
        }
    }
}

/// Handle a control command
fn handle_command(command: ControlCommand, state: &ControlServer) -> ControlResponse {
    match command {
        ControlCommand::SetAutoPortForward { enabled } => {
            state
                .auto_port_forward_enabled
                .store(enabled, Ordering::Relaxed);

            let status = if enabled { "enabled" } else { "disabled" };
            info!("Auto port-forward {} via control API", status);

            ControlResponse::success(format!("Auto port-forward {}", status))
        }
        ControlCommand::GetStatus => {
            let auto_port_forward_enabled = state.auto_port_forward_enabled.load(Ordering::Relaxed);

            let data = serde_json::json!({
                "auto_port_forward_enabled": auto_port_forward_enabled,
            });

            ControlResponse::success_with_data("Status retrieved", data)
        }
    }
}

/// Create a JSON response
fn json_response(status: StatusCode, response: ControlResponse) -> Response<String> {
    let body = serde_json::to_string(&response).unwrap_or_else(|_| "{}".to_string());

    Response::builder()
        .status(status)
        .header("Content-Type", "application/json")
        .header("Access-Control-Allow-Origin", "*")
        .body(body)
        .unwrap()
}
