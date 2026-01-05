//! Intercepting Tunnel for Protocol-Aware Proxying
//!
//! This module provides a transparent tunnel that:
//! 1. Peeks at initial bytes to detect the protocol
//! 2. Parses protocol messages as they flow through
//! 3. Creates trace spans and records for ClickHouse
//! 4. Forwards all data transparently to the destination
//!
//! The interception is completely transparent - all bytes are forwarded
//! exactly as received, we just observe and record them.

use super::{detect_protocol, kafka, postgres, MessageDirection, Protocol, MIN_DETECTION_BYTES};
use roxy_core::SpanRecord;
use roxy_core::{DatabaseQueryRow, KafkaMessageRow, RoxyClickHouse, TcpConnectionRow};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tracing::{debug, info, trace, warn};

/// Configuration for the intercepting tunnel
#[derive(Debug, Clone)]
pub struct InterceptConfig {
    /// Whether to enable protocol detection
    pub detect_protocol: bool,
    /// Whether to parse and record messages
    pub record_messages: bool,
    /// Maximum message size to parse (larger messages are skipped)
    pub max_parse_size: usize,
    /// Buffer size for reading
    pub buffer_size: usize,
    /// Timeout for protocol detection (milliseconds)
    pub detection_timeout_ms: u64,
    /// Whether to record connection-level spans
    pub record_connection_spans: bool,
    /// Connection timeout in seconds (0 = no timeout)
    pub connection_timeout_secs: u64,
}

impl Default for InterceptConfig {
    fn default() -> Self {
        Self {
            detect_protocol: true,
            record_messages: true,
            max_parse_size: 1024 * 1024, // 1 MB
            buffer_size: 32 * 1024,      // 32 KB
            detection_timeout_ms: 500,   // 500ms for protocol detection
            record_connection_spans: true,
            connection_timeout_secs: 60, // 60 second connection timeout
        }
    }
}

/// Statistics from an intercepting tunnel session
#[derive(Debug, Clone, Default)]
pub struct InterceptStats {
    /// Detected protocol
    pub protocol: Option<Protocol>,
    /// Bytes sent from client to server
    pub bytes_sent: u64,
    /// Bytes received from server to client
    pub bytes_received: u64,
    /// Number of client messages parsed
    pub client_messages: u64,
    /// Number of server messages parsed
    pub server_messages: u64,
    /// Number of parse errors
    pub parse_errors: u64,
    /// Connection start time
    pub start_time: Option<Instant>,
    /// Connection end time
    pub end_time: Option<Instant>,
    /// Whether protocol was detected from wire (vs port fallback)
    pub wire_detected: bool,
    /// Error message if connection failed
    pub error: Option<String>,
    /// Initial bytes from client (for debugging unknown protocols)
    pub initial_bytes: Vec<u8>,
    /// Sample of request data
    pub sample_request: Vec<u8>,
    /// Sample of response data
    pub sample_response: Vec<u8>,
    /// Original target hostname
    pub target_host: String,
}

/// Maximum bytes to capture for samples
const MAX_SAMPLE_BYTES: usize = 256;

/// State for tracking PostgreSQL connections
#[derive(Debug, Default)]
struct PostgresState {
    /// Whether we've seen the startup message
    startup_complete: bool,
    /// Database name from startup
    database: Option<String>,
    /// User name from startup
    user: Option<String>,
    /// Application name from startup
    application_name: Option<String>,
    /// Current pending query (for timing)
    pending_query: Option<PendingQuery>,
}

/// A pending query waiting for response
#[derive(Debug, Clone)]
struct PendingQuery {
    /// The SQL statement
    statement: String,
    /// Operation type (SELECT, INSERT, etc.)
    operation: Option<String>,
    /// When the query started
    start_time: Instant,
    /// Trace ID for this query
    trace_id: String,
    /// Span ID for this query
    span_id: String,
}

/// State for tracking Kafka connections
#[derive(Debug, Default)]
struct KafkaState {
    /// Client ID from requests
    client_id: Option<String>,
    /// Pending requests by correlation ID
    pending_requests: HashMap<i32, PendingKafkaRequest>,
}

/// A pending Kafka request waiting for response
#[derive(Debug, Clone)]
struct PendingKafkaRequest {
    /// API key
    api_key: kafka::ApiKey,
    /// API version
    api_version: i16,
    /// Operation name
    operation: String,
    /// Topics involved (if any)
    topics: Vec<String>,
    /// Consumer group (if any)
    group_id: Option<String>,
    /// When the request started
    start_time: Instant,
    /// Trace ID
    trace_id: String,
    /// Span ID
    span_id: String,
}

/// Run an intercepting tunnel between client and target
///
/// This function:
/// 1. Detects the protocol from initial bytes
/// 2. Sets up message parsing for known protocols
/// 3. Records messages to ClickHouse
/// 4. Returns statistics about the session
pub async fn run_intercepting_tunnel(
    client: TcpStream,
    target: TcpStream,
    client_addr: SocketAddr,
    target_addr: SocketAddr,
    target_port: u16,
    target_host: String,
    clickhouse: Option<Arc<RoxyClickHouse>>,
    config: InterceptConfig,
) -> InterceptStats {
    let mut stats = InterceptStats::default();
    stats.start_time = Some(Instant::now());
    stats.target_host = target_host;

    // Split streams for bidirectional communication
    let (client_read, client_write) = client.into_split();
    let (target_read, target_write) = target.into_split();

    // Wrap in buffered intercepting streams with optional timeout
    let tunnel_future = intercept_bidirectional(
        client_read,
        client_write,
        target_read,
        target_write,
        client_addr,
        target_addr,
        target_port,
        clickhouse.clone(),
        config.clone(),
        &mut stats,
    );

    let result = if config.connection_timeout_secs > 0 {
        match tokio::time::timeout(
            std::time::Duration::from_secs(config.connection_timeout_secs),
            tunnel_future,
        )
        .await
        {
            Ok(inner_result) => inner_result,
            Err(_) => {
                // Timeout expired
                warn!(
                    "Connection timeout ({}s) for {} -> {}:{}",
                    config.connection_timeout_secs, client_addr, target_addr, target_port
                );
                Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!(
                        "connection timeout after {}s",
                        config.connection_timeout_secs
                    ),
                ))
            }
        }
    } else {
        tunnel_future.await
    };

    stats.end_time = Some(Instant::now());

    if let Err(ref e) = result {
        debug!("Tunnel ended with error: {}", e);
        stats.error = Some(e.to_string());
    }

    // Record connection-level span
    if config.record_connection_spans {
        if let Some(ref ch) = clickhouse {
            record_connection_span(&stats, &client_addr, &target_addr, target_port, ch).await;
        }
    }

    stats
}

/// Record a connection-level span and TCP connection record to ClickHouse
async fn record_connection_span(
    stats: &InterceptStats,
    client_addr: &SocketAddr,
    target_addr: &SocketAddr,
    target_port: u16,
    clickhouse: &Arc<RoxyClickHouse>,
) {
    let duration_ms = stats
        .start_time
        .zip(stats.end_time)
        .map(|(start, end)| end.duration_since(start).as_secs_f64() * 1000.0)
        .unwrap_or(0.0);

    let duration_ns = (duration_ms * 1_000_000.0) as i64;

    let protocol = stats.protocol.unwrap_or(Protocol::Unknown);
    let now = chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0);

    let span_name = match protocol {
        Protocol::PostgreSQL => "postgresql.connection",
        Protocol::Kafka => "kafka.connection",
        Protocol::Unknown => "tcp.connection",
    };

    let status = if stats.error.is_some() { "error" } else { "ok" };

    // Create TCP connection record with raw bytes
    let tcp_record = TcpConnectionRow {
        id: uuid::Uuid::new_v4().to_string(),
        trace_id: format!("{:032x}", rand::random::<u128>()),
        span_id: format!("{:016x}", rand::random::<u64>()),
        timestamp: chrono::Utc::now().timestamp_millis(),
        duration_ms,
        protocol: protocol.span_name().to_string(),
        protocol_detected: if stats.wire_detected { "wire" } else { "port" }.to_string(),
        wire_detected: if stats.wire_detected { 1 } else { 0 },
        client_address: client_addr.ip().to_string(),
        client_port: client_addr.port(),
        server_address: target_addr.ip().to_string(),
        server_port: target_port,
        target_host: stats.target_host.clone(),
        bytes_sent: stats.bytes_sent,
        bytes_received: stats.bytes_received,
        client_messages: stats.client_messages,
        server_messages: stats.server_messages,
        parse_errors: stats.parse_errors,
        initial_bytes_hex: TcpConnectionRow::bytes_to_hex(&stats.initial_bytes, MAX_SAMPLE_BYTES),
        initial_bytes_ascii: TcpConnectionRow::bytes_to_ascii(
            &stats.initial_bytes,
            MAX_SAMPLE_BYTES,
        ),
        sample_request_hex: TcpConnectionRow::bytes_to_hex(&stats.sample_request, MAX_SAMPLE_BYTES),
        sample_response_hex: TcpConnectionRow::bytes_to_hex(
            &stats.sample_response,
            MAX_SAMPLE_BYTES,
        ),
        status: status.to_string(),
        error_message: stats.error.clone().unwrap_or_default(),
        attributes: "{}".to_string(),
    };

    // Also create a span record for the spans table
    let attributes = serde_json::json!({
        "net.peer.ip": target_addr.ip().to_string(),
        "net.peer.port": target_port,
        "net.sock.peer.addr": client_addr.to_string(),
        "target.host": stats.target_host,
        "protocol": protocol.span_name(),
        "protocol.wire_detected": stats.wire_detected,
        "bytes.sent": stats.bytes_sent,
        "bytes.received": stats.bytes_received,
        "messages.client": stats.client_messages,
        "messages.server": stats.server_messages,
        "parse.errors": stats.parse_errors,
        "initial_bytes": TcpConnectionRow::bytes_to_hex(&stats.initial_bytes, 64),
    });

    let span = SpanRecord {
        trace_id: tcp_record.trace_id.clone(),
        span_id: tcp_record.span_id.clone(),
        parent_span_id: String::new(),
        name: span_name.to_string(),
        service_name: "roxy-proxy".to_string(),
        kind: "CLIENT".to_string(),
        start_time: now - duration_ns,
        end_time: now,
        duration_ns,
        status_code: if stats.error.is_some() { "ERROR" } else { "OK" }.to_string(),
        status_message: stats.error.clone().unwrap_or_default(),
        attributes: attributes.to_string(),
        events: "[]".to_string(),
        links: "[]".to_string(),
    };

    let ch = clickhouse.clone();
    tokio::spawn(async move {
        // Insert TCP connection record
        if let Err(e) = ch.insert_tcp_connection(&tcp_record).await {
            warn!("Failed to insert TCP connection: {}", e);
        }
        // Insert span record
        if let Err(e) = ch.insert_span(&span).await {
            warn!("Failed to insert connection span: {}", e);
        }
    });
}

/// Bidirectional interception with protocol detection
async fn intercept_bidirectional<CR, CW, TR, TW>(
    mut client_read: CR,
    mut client_write: CW,
    mut target_read: TR,
    mut target_write: TW,
    client_addr: SocketAddr,
    target_addr: SocketAddr,
    target_port: u16,
    clickhouse: Option<Arc<RoxyClickHouse>>,
    config: InterceptConfig,
    stats: &mut InterceptStats,
) -> Result<(), std::io::Error>
where
    CR: AsyncRead + Unpin,
    CW: AsyncWrite + Unpin,
    TR: AsyncRead + Unpin,
    TW: AsyncWrite + Unpin,
{
    // Buffer for initial protocol detection
    let mut detect_buf = vec![0u8; MIN_DETECTION_BYTES.max(config.buffer_size)];

    // Try to read initial bytes from client for protocol detection
    let initial_bytes = if config.detect_protocol {
        match tokio::time::timeout(
            std::time::Duration::from_millis(config.detection_timeout_ms),
            client_read.read(&mut detect_buf),
        )
        .await
        {
            Ok(Ok(n)) if n > 0 => {
                // Detect protocol from wire data
                let protocol = detect_protocol(&detect_buf[..n], target_port);
                stats.protocol = Some(protocol);
                stats.wire_detected = true;

                info!(
                    "Detected protocol: {} for {}:{} (from {} bytes, wire detection)",
                    protocol, target_addr, target_port, n
                );

                let initial = detect_buf[..n].to_vec();
                stats.initial_bytes = initial.clone();
                stats.sample_request = initial.clone();
                Some(initial)
            }
            Ok(Ok(_)) => {
                // Zero bytes read (connection closed immediately)
                let protocol = detect_protocol(&[], target_port);
                stats.protocol = Some(protocol);
                stats.wire_detected = false;

                info!(
                    "Connection closed immediately, using port-based detection: {} for {}:{}",
                    protocol, target_addr, target_port
                );

                None
            }
            Ok(Err(e)) => {
                // Read error
                let protocol = detect_protocol(&[], target_port);
                stats.protocol = Some(protocol);
                stats.wire_detected = false;

                info!(
                    "Read error during detection ({}), using port-based detection: {} for {}:{}",
                    e, protocol, target_addr, target_port
                );

                None
            }
            Err(_) => {
                // Timeout - client hasn't sent data yet (common for Kafka where server speaks first)
                let protocol = detect_protocol(&[], target_port);
                stats.protocol = Some(protocol);
                stats.wire_detected = false;

                info!(
                    "Detection timeout ({}ms), using port-based detection: {} for {}:{}",
                    config.detection_timeout_ms, protocol, target_addr, target_port
                );

                None
            }
        }
    } else {
        stats.protocol = Some(Protocol::Unknown);
        stats.wire_detected = false;
        None
    };

    // Forward initial bytes if we read any
    if let Some(ref initial) = initial_bytes {
        target_write.write_all(initial).await?;
        stats.bytes_sent += initial.len() as u64;
    }

    // Set up protocol-specific state
    let mut postgres_state = PostgresState::default();
    let mut kafka_state = KafkaState::default();

    // Parse initial bytes if we have them
    if let Some(ref initial) = initial_bytes {
        if config.record_messages {
            match stats.protocol {
                Some(Protocol::PostgreSQL) => {
                    parse_postgres_data(
                        initial,
                        MessageDirection::ClientToServer,
                        &mut postgres_state,
                        &client_addr,
                        &target_addr,
                        target_port,
                        &clickhouse,
                        stats,
                    )
                    .await;
                }
                Some(Protocol::Kafka) => {
                    parse_kafka_data(
                        initial,
                        MessageDirection::ClientToServer,
                        &mut kafka_state,
                        &client_addr,
                        &target_addr,
                        target_port,
                        &clickhouse,
                        stats,
                    )
                    .await;
                }
                _ => {}
            }
        }
    }

    // Create buffers for bidirectional copy
    let mut client_buf = vec![0u8; config.buffer_size];
    let mut target_buf = vec![0u8; config.buffer_size];

    loop {
        tokio::select! {
            // Client -> Target
            result = client_read.read(&mut client_buf) => {
                match result {
                    Ok(0) => {
                        // Client closed connection
                        debug!("Client closed connection");
                        let _ = target_write.shutdown().await;
                        break;
                    }
                    Ok(n) => {
                        // Parse and forward
                        // Capture sample request data if we don't have any yet
                        if stats.sample_request.is_empty() && n > 0 {
                            stats.sample_request = client_buf[..n.min(MAX_SAMPLE_BYTES)].to_vec();
                        }
                        if config.record_messages {
                            match stats.protocol {
                                Some(Protocol::PostgreSQL) => {
                                    parse_postgres_data(
                                        &client_buf[..n],
                                        MessageDirection::ClientToServer,
                                        &mut postgres_state,
                                        &client_addr,
                                        &target_addr,
                                        target_port,
                                        &clickhouse,
                                        stats,
                                    ).await;
                                }
                                Some(Protocol::Kafka) => {
                                    parse_kafka_data(
                                        &client_buf[..n],
                                        MessageDirection::ClientToServer,
                                        &mut kafka_state,
                                        &client_addr,
                                        &target_addr,
                                        target_port,
                                        &clickhouse,
                                        stats,
                                    ).await;
                                }
                                _ => {}
                            }
                        }
                        target_write.write_all(&client_buf[..n]).await?;
                        stats.bytes_sent += n as u64;
                    }
                    Err(e) => {
                        debug!("Client read error: {}", e);
                        break;
                    }
                }
            }
            // Target -> Client
            result = target_read.read(&mut target_buf) => {
                match result {
                    Ok(0) => {
                        // Target closed connection
                        debug!("Target closed connection");
                        let _ = client_write.shutdown().await;
                        break;
                    }
                    Ok(n) => {
                        // Capture sample response data if we don't have any yet
                        if stats.sample_response.is_empty() && n > 0 {
                            stats.sample_response = target_buf[..n.min(MAX_SAMPLE_BYTES)].to_vec();
                        }
                        // Parse and forward
                        if config.record_messages {
                            match stats.protocol {
                                Some(Protocol::PostgreSQL) => {
                                    parse_postgres_data(
                                        &target_buf[..n],
                                        MessageDirection::ServerToClient,
                                        &mut postgres_state,
                                        &client_addr,
                                        &target_addr,
                                        target_port,
                                        &clickhouse,
                                        stats,
                                    ).await;
                                }
                                Some(Protocol::Kafka) => {
                                    parse_kafka_data(
                                        &target_buf[..n],
                                        MessageDirection::ServerToClient,
                                        &mut kafka_state,
                                        &client_addr,
                                        &target_addr,
                                        target_port,
                                        &clickhouse,
                                        stats,
                                    ).await;
                                }
                                _ => {}
                            }
                        }
                        client_write.write_all(&target_buf[..n]).await?;
                        stats.bytes_received += n as u64;
                    }
                    Err(e) => {
                        debug!("Target read error: {}", e);
                        break;
                    }
                }
            }
        }
    }

    Ok(())
}

/// Parse PostgreSQL data and record to ClickHouse
async fn parse_postgres_data(
    data: &[u8],
    direction: MessageDirection,
    state: &mut PostgresState,
    client_addr: &SocketAddr,
    target_addr: &SocketAddr,
    target_port: u16,
    clickhouse: &Option<Arc<RoxyClickHouse>>,
    stats: &mut InterceptStats,
) {
    let mut offset = 0;

    while offset < data.len() {
        // Try to parse a message
        let remaining = &data[offset..];

        let parse_result =
            if !state.startup_complete && direction == MessageDirection::ClientToServer {
                postgres::parse_message(remaining, true)
            } else {
                postgres::parse_message(remaining, false)
            };

        match parse_result {
            Ok((msg, consumed)) => {
                offset += consumed;

                match direction {
                    MessageDirection::ClientToServer => stats.client_messages += 1,
                    MessageDirection::ServerToClient => stats.server_messages += 1,
                }

                trace!("Parsed PostgreSQL message: {:?}", msg.span_name());

                // Handle the message
                handle_postgres_message(
                    msg,
                    direction,
                    state,
                    client_addr,
                    target_addr,
                    target_port,
                    clickhouse,
                )
                .await;
            }
            Err(postgres::ParseError::Incomplete { .. }) => {
                // Need more data, stop parsing this chunk
                break;
            }
            Err(e) => {
                trace!("PostgreSQL parse error: {}", e);
                stats.parse_errors += 1;
                // Skip this chunk and try again
                break;
            }
        }
    }
}

/// Handle a parsed PostgreSQL message
async fn handle_postgres_message(
    msg: postgres::PostgresMessage,
    _direction: MessageDirection,
    state: &mut PostgresState,
    client_addr: &SocketAddr,
    target_addr: &SocketAddr,
    target_port: u16,
    clickhouse: &Option<Arc<RoxyClickHouse>>,
) {
    match msg {
        postgres::PostgresMessage::Startup(startup) => {
            state.startup_complete = true;
            state.database = startup.database().map(String::from);
            state.user = startup.user().map(String::from);
            state.application_name = startup.application_name().map(String::from);

            info!(
                "PostgreSQL connection: user={:?} database={:?} app={:?}",
                state.user, state.database, state.application_name
            );
        }

        postgres::PostgresMessage::Query(query) => {
            let trace_id = format!("{:032x}", rand::random::<u128>());
            let span_id = format!("{:016x}", rand::random::<u64>());

            let operation = query
                .query
                .trim()
                .split_whitespace()
                .next()
                .map(|s| s.to_uppercase());

            debug!(
                "PostgreSQL query: {} (op: {:?})",
                truncate_query(&query.query, 100),
                operation
            );

            state.pending_query = Some(PendingQuery {
                statement: query.query.clone(),
                operation: operation.clone(),
                start_time: Instant::now(),
                trace_id,
                span_id,
            });
        }

        postgres::PostgresMessage::Parse(parse) => {
            let trace_id = format!("{:032x}", rand::random::<u128>());
            let span_id = format!("{:016x}", rand::random::<u64>());

            let operation = parse
                .query
                .trim()
                .split_whitespace()
                .next()
                .map(|s| s.to_uppercase());

            debug!(
                "PostgreSQL parse: {} (op: {:?})",
                truncate_query(&parse.query, 100),
                operation
            );

            state.pending_query = Some(PendingQuery {
                statement: parse.query.clone(),
                operation,
                start_time: Instant::now(),
                trace_id,
                span_id,
            });
        }

        postgres::PostgresMessage::CommandComplete(complete) => {
            if let Some(pending) = state.pending_query.take() {
                let duration_ms = pending.start_time.elapsed().as_secs_f64() * 1000.0;
                let rows_affected = complete.row_count().map(|r| r as i64);

                info!(
                    "PostgreSQL complete: {} in {:.2}ms ({} rows)",
                    complete.tag,
                    duration_ms,
                    rows_affected.unwrap_or(0)
                );

                // Record to ClickHouse
                if let Some(ch) = clickhouse {
                    let record = DatabaseQueryRow {
                        id: uuid::Uuid::new_v4().to_string(),
                        trace_id: pending.trace_id,
                        span_id: pending.span_id,
                        parent_span_id: String::new(),
                        timestamp: chrono::Utc::now().timestamp_millis(),
                        duration_ms,
                        db_system: "postgresql".to_string(),
                        db_name: state.database.clone().unwrap_or_default(),
                        db_user: state.user.clone().unwrap_or_default(),
                        db_operation: pending.operation.unwrap_or_default(),
                        db_statement: pending.statement,
                        db_rows_affected: rows_affected.unwrap_or(-1),
                        server_address: target_addr.ip().to_string(),
                        server_port: target_port,
                        client_address: client_addr.to_string(),
                        success: 1,
                        error_message: String::new(),
                        error_code: String::new(),
                        application_name: state.application_name.clone().unwrap_or_default(),
                        attributes: "{}".to_string(),
                    };

                    let ch = ch.clone();
                    tokio::spawn(async move {
                        if let Err(e) = ch.insert_database_query(&record).await {
                            warn!("Failed to insert database query: {}", e);
                        }
                    });
                }
            }
        }

        postgres::PostgresMessage::ErrorResponse(error) => {
            if let Some(pending) = state.pending_query.take() {
                let duration_ms = pending.start_time.elapsed().as_secs_f64() * 1000.0;

                warn!(
                    "PostgreSQL error: {} - {} ({})",
                    error.severity, error.message, error.code
                );

                // Record error to ClickHouse
                if let Some(ch) = clickhouse {
                    let record = DatabaseQueryRow {
                        id: uuid::Uuid::new_v4().to_string(),
                        trace_id: pending.trace_id,
                        span_id: pending.span_id,
                        parent_span_id: String::new(),
                        timestamp: chrono::Utc::now().timestamp_millis(),
                        duration_ms,
                        db_system: "postgresql".to_string(),
                        db_name: state.database.clone().unwrap_or_default(),
                        db_user: state.user.clone().unwrap_or_default(),
                        db_operation: pending.operation.unwrap_or_default(),
                        db_statement: pending.statement,
                        db_rows_affected: -1,
                        server_address: target_addr.ip().to_string(),
                        server_port: target_port,
                        client_address: client_addr.to_string(),
                        success: 0,
                        error_message: error.message,
                        error_code: error.code,
                        application_name: state.application_name.clone().unwrap_or_default(),
                        attributes: "{}".to_string(),
                    };

                    let ch = ch.clone();
                    tokio::spawn(async move {
                        if let Err(e) = ch.insert_database_query(&record).await {
                            warn!("Failed to insert database query error: {}", e);
                        }
                    });
                }
            }
        }

        postgres::PostgresMessage::ReadyForQuery(_) => {
            // Connection is ready, clear any pending state
            state.startup_complete = true;
        }

        _ => {
            // Other messages we just observe
        }
    }
}

/// Parse Kafka data and record to ClickHouse
async fn parse_kafka_data(
    data: &[u8],
    direction: MessageDirection,
    state: &mut KafkaState,
    client_addr: &SocketAddr,
    target_addr: &SocketAddr,
    target_port: u16,
    clickhouse: &Option<Arc<RoxyClickHouse>>,
    stats: &mut InterceptStats,
) {
    let mut offset = 0;

    while offset < data.len() {
        let remaining = &data[offset..];

        // Kafka requests and responses have different formats
        if direction == MessageDirection::ClientToServer {
            match kafka::parse_request(remaining) {
                Ok((request, consumed)) => {
                    offset += consumed;
                    stats.client_messages += 1;

                    debug!(
                        "Kafka request: {} (v{}) correlation_id={}",
                        request.api_key.name(),
                        request.api_version,
                        request.correlation_id
                    );

                    // Update client ID if present
                    if let Some(ref client_id) = request.client_id {
                        state.client_id = Some(client_id.clone());
                    }

                    // Extract topics and group ID based on request type
                    let topics: Vec<String> = match &request.details {
                        kafka::RequestDetails::Produce(p) => p.topics.clone(),
                        kafka::RequestDetails::Fetch(f) => f.topics.clone(),
                        kafka::RequestDetails::Metadata(m) => m.topics.clone(),
                        _ => Vec::new(),
                    };

                    let group_id = match &request.details {
                        kafka::RequestDetails::JoinGroup(j) => Some(j.group_id.clone()),
                        kafka::RequestDetails::Heartbeat(h) => Some(h.group_id.clone()),
                        kafka::RequestDetails::LeaveGroup(l) => Some(l.group_id.clone()),
                        kafka::RequestDetails::SyncGroup(s) => Some(s.group_id.clone()),
                        kafka::RequestDetails::OffsetCommit(o) => Some(o.group_id.clone()),
                        kafka::RequestDetails::OffsetFetch(o) => Some(o.group_id.clone()),
                        _ => None,
                    };

                    // Store pending request
                    let trace_id = format!("{:032x}", rand::random::<u128>());
                    let span_id = format!("{:016x}", rand::random::<u64>());

                    state.pending_requests.insert(
                        request.correlation_id,
                        PendingKafkaRequest {
                            api_key: request.api_key,
                            api_version: request.api_version,
                            operation: request.api_key.name().to_string(),
                            topics,
                            group_id,
                            start_time: Instant::now(),
                            trace_id,
                            span_id,
                        },
                    );
                }
                Err(kafka::ParseError::Incomplete { .. }) => break,
                Err(e) => {
                    trace!("Kafka request parse error: {}", e);
                    stats.parse_errors += 1;
                    break;
                }
            }
        } else {
            // Parse response (simplified - just get correlation_id)
            if remaining.len() >= 8 {
                let _length =
                    u32::from_be_bytes([remaining[0], remaining[1], remaining[2], remaining[3]]);
                let correlation_id =
                    i32::from_be_bytes([remaining[4], remaining[5], remaining[6], remaining[7]]);

                stats.server_messages += 1;

                // Match with pending request
                if let Some(pending) = state.pending_requests.remove(&correlation_id) {
                    let duration_ms = pending.start_time.elapsed().as_secs_f64() * 1000.0;

                    debug!(
                        "Kafka response for {} in {:.2}ms",
                        pending.operation, duration_ms
                    );

                    // Record to ClickHouse
                    if let Some(ch) = clickhouse {
                        let operation_type = if pending.api_key.is_produce() {
                            "publish"
                        } else if pending.api_key.is_consume() {
                            "receive"
                        } else {
                            "process"
                        };

                        let record = KafkaMessageRow {
                            id: uuid::Uuid::new_v4().to_string(),
                            trace_id: pending.trace_id,
                            span_id: pending.span_id,
                            parent_span_id: String::new(),
                            timestamp: chrono::Utc::now().timestamp_millis(),
                            duration_ms,
                            messaging_system: "kafka".to_string(),
                            messaging_operation: pending.operation,
                            operation_type: operation_type.to_string(),
                            messaging_destination: pending.topics.join(","),
                            messaging_consumer_group: pending.group_id.unwrap_or_default(),
                            messaging_client_id: state.client_id.clone().unwrap_or_default(),
                            kafka_api_key: pending.api_key as i16,
                            kafka_api_version: pending.api_version,
                            kafka_correlation_id: correlation_id,
                            message_count: 0,
                            payload_size: 0,
                            server_address: target_addr.ip().to_string(),
                            server_port: target_port,
                            client_address: client_addr.to_string(),
                            success: 1,
                            error_code: 0,
                            error_message: String::new(),
                            attributes: "{}".to_string(),
                        };

                        let ch = ch.clone();
                        tokio::spawn(async move {
                            if let Err(e) = ch.insert_kafka_message(&record).await {
                                warn!("Failed to insert Kafka message: {}", e);
                            }
                        });
                    }
                }

                // Skip this message (we don't fully parse responses yet)
                let total_len = (_length + 4) as usize;
                if remaining.len() >= total_len {
                    offset += total_len;
                } else {
                    break;
                }
            } else {
                break;
            }
        }
    }
}

/// Truncate a query string for logging
fn truncate_query(query: &str, max_len: usize) -> String {
    // Collapse all whitespace (newlines, tabs, multiple spaces) into single spaces
    let query: String = query.split_whitespace().collect::<Vec<_>>().join(" ");
    if query.len() > max_len {
        format!("{}...", &query[..max_len])
    } else {
        query
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_intercept_config_default() {
        let config = InterceptConfig::default();
        assert!(config.detect_protocol);
        assert!(config.record_messages);
        assert_eq!(config.max_parse_size, 1024 * 1024);
        assert_eq!(config.buffer_size, 32 * 1024);
    }

    #[test]
    fn test_truncate_query() {
        assert_eq!(truncate_query("SELECT 1", 100), "SELECT 1");
        assert_eq!(truncate_query("SELECT 1", 5), "SELEC...");
        assert_eq!(
            truncate_query("SELECT\n  *\n  FROM\n  table", 100),
            "SELECT * FROM table"
        );
    }

    #[test]
    fn test_intercept_stats_default() {
        let stats = InterceptStats::default();
        assert!(stats.protocol.is_none());
        assert_eq!(stats.bytes_sent, 0);
        assert_eq!(stats.bytes_received, 0);
        assert_eq!(stats.client_messages, 0);
        assert_eq!(stats.server_messages, 0);
    }
}
