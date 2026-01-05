//! PostgreSQL Wire Protocol Parser
//!
//! This module implements parsing of the PostgreSQL wire protocol (v3.0)
//! for extracting query information and creating trace spans.
//!
//! ## Protocol Overview
//!
//! PostgreSQL uses a message-based protocol where each message has:
//! - For startup: length (4 bytes) + protocol version + parameters
//! - For regular messages: type (1 byte) + length (4 bytes) + payload
//!
//! ## References
//! - https://www.postgresql.org/docs/current/protocol-message-formats.html

use std::fmt;

/// PostgreSQL protocol version 3.0 as a 32-bit integer
pub const PROTOCOL_VERSION_3: u32 = 0x0003_0000;

/// SSL request code
pub const SSL_REQUEST_CODE: u32 = 80877103;

/// Cancel request code
pub const CANCEL_REQUEST_CODE: u32 = 80877102;

/// Maximum reasonable message size (256 MB)
const MAX_MESSAGE_SIZE: u32 = 256 * 1024 * 1024;

/// PostgreSQL frontend (client) message types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FrontendMessageType {
    /// Bind a prepared statement to parameters
    Bind = b'B',
    /// Close a prepared statement or portal
    Close = b'C',
    /// Describe a prepared statement or portal
    Describe = b'D',
    /// Execute a bound portal
    Execute = b'E',
    /// Flush pending output
    Flush = b'H',
    /// Parse a query into a prepared statement
    Parse = b'P',
    /// Simple query
    Query = b'Q',
    /// Sync (end of extended query)
    Sync = b'S',
    /// Terminate connection
    Terminate = b'X',
    /// Copy data
    CopyData = b'd',
    /// Copy completion
    CopyDone = b'c',
    /// Copy failure
    CopyFail = b'f',
    /// Password message (during authentication)
    Password = b'p',
}

impl TryFrom<u8> for FrontendMessageType {
    type Error = ParseError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            b'B' => Ok(FrontendMessageType::Bind),
            b'C' => Ok(FrontendMessageType::Close),
            b'D' => Ok(FrontendMessageType::Describe),
            b'E' => Ok(FrontendMessageType::Execute),
            b'H' => Ok(FrontendMessageType::Flush),
            b'P' => Ok(FrontendMessageType::Parse),
            b'Q' => Ok(FrontendMessageType::Query),
            b'S' => Ok(FrontendMessageType::Sync),
            b'X' => Ok(FrontendMessageType::Terminate),
            b'd' => Ok(FrontendMessageType::CopyData),
            b'c' => Ok(FrontendMessageType::CopyDone),
            b'f' => Ok(FrontendMessageType::CopyFail),
            b'p' => Ok(FrontendMessageType::Password),
            _ => Err(ParseError::UnknownMessageType(value)),
        }
    }
}

/// PostgreSQL backend (server) message types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum BackendMessageType {
    /// Authentication request
    Authentication = b'R',
    /// Backend key data (for cancellation)
    BackendKeyData = b'K',
    /// Bind complete
    BindComplete = b'2',
    /// Close complete
    CloseComplete = b'3',
    /// Command complete (with tag like "SELECT 1")
    CommandComplete = b'C',
    /// Data row
    DataRow = b'D',
    /// Empty query response
    EmptyQueryResponse = b'I',
    /// Error response
    ErrorResponse = b'E',
    /// No data (for Describe)
    NoData = b'n',
    /// Notice response
    NoticeResponse = b'N',
    /// Notification response
    NotificationResponse = b'A',
    /// Parameter description
    ParameterDescription = b't',
    /// Parameter status
    ParameterStatus = b'S',
    /// Parse complete
    ParseComplete = b'1',
    /// Portal suspended
    PortalSuspended = b's',
    /// Ready for query
    ReadyForQuery = b'Z',
    /// Row description
    RowDescription = b'T',
    /// Copy data
    CopyData = b'd',
    /// Copy done
    CopyDone = b'c',
    /// Copy in response
    CopyInResponse = b'G',
    /// Copy out response
    CopyOutResponse = b'H',
    /// Copy both response
    CopyBothResponse = b'W',
}

impl TryFrom<u8> for BackendMessageType {
    type Error = ParseError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            b'R' => Ok(BackendMessageType::Authentication),
            b'K' => Ok(BackendMessageType::BackendKeyData),
            b'2' => Ok(BackendMessageType::BindComplete),
            b'3' => Ok(BackendMessageType::CloseComplete),
            b'C' => Ok(BackendMessageType::CommandComplete),
            b'D' => Ok(BackendMessageType::DataRow),
            b'I' => Ok(BackendMessageType::EmptyQueryResponse),
            b'E' => Ok(BackendMessageType::ErrorResponse),
            b'n' => Ok(BackendMessageType::NoData),
            b'N' => Ok(BackendMessageType::NoticeResponse),
            b'A' => Ok(BackendMessageType::NotificationResponse),
            b't' => Ok(BackendMessageType::ParameterDescription),
            b'S' => Ok(BackendMessageType::ParameterStatus),
            b'1' => Ok(BackendMessageType::ParseComplete),
            b's' => Ok(BackendMessageType::PortalSuspended),
            b'Z' => Ok(BackendMessageType::ReadyForQuery),
            b'T' => Ok(BackendMessageType::RowDescription),
            b'd' => Ok(BackendMessageType::CopyData),
            b'c' => Ok(BackendMessageType::CopyDone),
            b'G' => Ok(BackendMessageType::CopyInResponse),
            b'H' => Ok(BackendMessageType::CopyOutResponse),
            b'W' => Ok(BackendMessageType::CopyBothResponse),
            _ => Err(ParseError::UnknownMessageType(value)),
        }
    }
}

/// Parse errors for PostgreSQL protocol
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// Not enough data to parse
    Incomplete { needed: usize },
    /// Unknown message type
    UnknownMessageType(u8),
    /// Invalid message length
    InvalidLength(u32),
    /// Invalid UTF-8 in string
    InvalidUtf8,
    /// Protocol error
    ProtocolError(String),
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::Incomplete { needed } => {
                write!(f, "incomplete message, need {} more bytes", needed)
            }
            ParseError::UnknownMessageType(t) => write!(f, "unknown message type: 0x{:02x}", t),
            ParseError::InvalidLength(len) => write!(f, "invalid message length: {}", len),
            ParseError::InvalidUtf8 => write!(f, "invalid UTF-8 in message"),
            ParseError::ProtocolError(msg) => write!(f, "protocol error: {}", msg),
        }
    }
}

impl std::error::Error for ParseError {}

/// A parsed PostgreSQL message
#[derive(Debug, Clone)]
pub enum PostgresMessage {
    /// Startup message (beginning of connection)
    Startup(StartupMessage),
    /// SSL negotiation request
    SslRequest,
    /// Cancel request
    CancelRequest { pid: u32, secret_key: u32 },
    /// Simple query
    Query(QueryMessage),
    /// Parse (extended query - prepare statement)
    Parse(ParseMessage),
    /// Bind (extended query - bind parameters)
    Bind(BindMessage),
    /// Execute (extended query - run)
    Execute(ExecuteMessage),
    /// Describe request
    Describe(DescribeMessage),
    /// Close request
    Close(CloseMessage),
    /// Sync message
    Sync,
    /// Terminate connection
    Terminate,
    /// Command complete response
    CommandComplete(CommandCompleteMessage),
    /// Error response
    ErrorResponse(ErrorResponseMessage),
    /// Ready for query
    ReadyForQuery(TransactionStatus),
    /// Row description
    RowDescription(RowDescriptionMessage),
    /// Data row
    DataRow(DataRowMessage),
    /// Authentication message
    Authentication(AuthenticationMessage),
    /// Parameter status
    ParameterStatus { name: String, value: String },
    /// Generic/unhandled message
    Other { message_type: u8, length: u32 },
}

impl PostgresMessage {
    /// Get a description suitable for span names
    pub fn span_name(&self) -> &str {
        match self {
            PostgresMessage::Startup(_) => "pg.startup",
            PostgresMessage::SslRequest => "pg.ssl_request",
            PostgresMessage::CancelRequest { .. } => "pg.cancel",
            PostgresMessage::Query(_) => "pg.query",
            PostgresMessage::Parse(_) => "pg.parse",
            PostgresMessage::Bind(_) => "pg.bind",
            PostgresMessage::Execute(_) => "pg.execute",
            PostgresMessage::Describe(_) => "pg.describe",
            PostgresMessage::Close(_) => "pg.close",
            PostgresMessage::Sync => "pg.sync",
            PostgresMessage::Terminate => "pg.terminate",
            PostgresMessage::CommandComplete(_) => "pg.command_complete",
            PostgresMessage::ErrorResponse(_) => "pg.error",
            PostgresMessage::ReadyForQuery(_) => "pg.ready",
            PostgresMessage::RowDescription(_) => "pg.row_description",
            PostgresMessage::DataRow(_) => "pg.data_row",
            PostgresMessage::Authentication(_) => "pg.auth",
            PostgresMessage::ParameterStatus { .. } => "pg.parameter_status",
            PostgresMessage::Other { .. } => "pg.message",
        }
    }

    /// Get the SQL statement if this is a query-related message
    pub fn statement(&self) -> Option<&str> {
        match self {
            PostgresMessage::Query(q) => Some(&q.query),
            PostgresMessage::Parse(p) => Some(&p.query),
            _ => None,
        }
    }

    /// Get the operation type (SELECT, INSERT, UPDATE, DELETE, etc.)
    pub fn operation(&self) -> Option<&str> {
        self.statement().and_then(|sql| {
            let sql = sql.trim().to_uppercase();
            if sql.starts_with("SELECT") {
                Some("SELECT")
            } else if sql.starts_with("INSERT") {
                Some("INSERT")
            } else if sql.starts_with("UPDATE") {
                Some("UPDATE")
            } else if sql.starts_with("DELETE") {
                Some("DELETE")
            } else if sql.starts_with("BEGIN") || sql.starts_with("START TRANSACTION") {
                Some("BEGIN")
            } else if sql.starts_with("COMMIT") {
                Some("COMMIT")
            } else if sql.starts_with("ROLLBACK") {
                Some("ROLLBACK")
            } else if sql.starts_with("CREATE") {
                Some("CREATE")
            } else if sql.starts_with("DROP") {
                Some("DROP")
            } else if sql.starts_with("ALTER") {
                Some("ALTER")
            } else if sql.starts_with("COPY") {
                Some("COPY")
            } else {
                None
            }
        })
    }
}

/// Startup message sent at connection start
#[derive(Debug, Clone)]
pub struct StartupMessage {
    /// Protocol version (major.minor as u32)
    pub protocol_version: u32,
    /// Connection parameters (user, database, etc.)
    pub parameters: Vec<(String, String)>,
}

impl StartupMessage {
    /// Get the database name
    pub fn database(&self) -> Option<&str> {
        self.parameters
            .iter()
            .find(|(k, _)| k == "database")
            .map(|(_, v)| v.as_str())
    }

    /// Get the user name
    pub fn user(&self) -> Option<&str> {
        self.parameters
            .iter()
            .find(|(k, _)| k == "user")
            .map(|(_, v)| v.as_str())
    }

    /// Get the application name
    pub fn application_name(&self) -> Option<&str> {
        self.parameters
            .iter()
            .find(|(k, _)| k == "application_name")
            .map(|(_, v)| v.as_str())
    }
}

/// Simple query message
#[derive(Debug, Clone)]
pub struct QueryMessage {
    /// The SQL query string
    pub query: String,
}

/// Parse message (extended query protocol)
#[derive(Debug, Clone)]
pub struct ParseMessage {
    /// Prepared statement name (empty for unnamed)
    pub statement_name: String,
    /// The SQL query with parameter placeholders ($1, $2, etc.)
    pub query: String,
    /// OIDs of parameter types (0 means unspecified)
    pub parameter_types: Vec<u32>,
}

/// Bind message (extended query protocol)
#[derive(Debug, Clone)]
pub struct BindMessage {
    /// Portal name (empty for unnamed)
    pub portal_name: String,
    /// Prepared statement name
    pub statement_name: String,
    /// Number of parameter format codes
    pub parameter_format_codes: Vec<i16>,
    /// Parameter values
    pub parameters: Vec<Option<Vec<u8>>>,
    /// Result format codes
    pub result_format_codes: Vec<i16>,
}

/// Execute message (extended query protocol)
#[derive(Debug, Clone)]
pub struct ExecuteMessage {
    /// Portal name
    pub portal_name: String,
    /// Maximum rows to return (0 = unlimited)
    pub max_rows: u32,
}

/// Describe message
#[derive(Debug, Clone)]
pub struct DescribeMessage {
    /// 'S' for statement, 'P' for portal
    pub target_type: char,
    /// Name of the target
    pub name: String,
}

/// Close message
#[derive(Debug, Clone)]
pub struct CloseMessage {
    /// 'S' for statement, 'P' for portal
    pub target_type: char,
    /// Name of the target
    pub name: String,
}

/// Command complete message from server
#[derive(Debug, Clone)]
pub struct CommandCompleteMessage {
    /// Command tag (e.g., "SELECT 5", "INSERT 0 1")
    pub tag: String,
}

impl CommandCompleteMessage {
    /// Get the command type (SELECT, INSERT, UPDATE, DELETE, etc.)
    pub fn command(&self) -> &str {
        self.tag.split_whitespace().next().unwrap_or("")
    }

    /// Get the row count affected/returned (if applicable)
    pub fn row_count(&self) -> Option<u64> {
        let parts: Vec<&str> = self.tag.split_whitespace().collect();
        match parts.as_slice() {
            ["SELECT", n] => n.parse().ok(),
            ["INSERT", _, n] => n.parse().ok(),
            ["UPDATE", n] => n.parse().ok(),
            ["DELETE", n] => n.parse().ok(),
            ["COPY", n] => n.parse().ok(),
            ["FETCH", n] => n.parse().ok(),
            ["MOVE", n] => n.parse().ok(),
            _ => None,
        }
    }
}

/// Error response message
#[derive(Debug, Clone)]
pub struct ErrorResponseMessage {
    /// Severity (ERROR, FATAL, PANIC)
    pub severity: String,
    /// SQLSTATE error code
    pub code: String,
    /// Primary error message
    pub message: String,
    /// Optional detail
    pub detail: Option<String>,
    /// Optional hint
    pub hint: Option<String>,
}

/// Transaction status indicator
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransactionStatus {
    /// Not in a transaction block
    Idle,
    /// In a transaction block
    InTransaction,
    /// In a failed transaction block
    Failed,
}

impl TryFrom<u8> for TransactionStatus {
    type Error = ParseError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            b'I' => Ok(TransactionStatus::Idle),
            b'T' => Ok(TransactionStatus::InTransaction),
            b'E' => Ok(TransactionStatus::Failed),
            _ => Err(ParseError::ProtocolError(format!(
                "invalid transaction status: {}",
                value as char
            ))),
        }
    }
}

/// Row description (column metadata)
#[derive(Debug, Clone)]
pub struct RowDescriptionMessage {
    /// Column descriptions
    pub columns: Vec<ColumnDescription>,
}

/// Column description within a row
#[derive(Debug, Clone)]
pub struct ColumnDescription {
    /// Column name
    pub name: String,
    /// Table OID (0 if not a table column)
    pub table_oid: u32,
    /// Column attribute number
    pub column_id: i16,
    /// Data type OID
    pub type_oid: u32,
    /// Data type size
    pub type_size: i16,
    /// Type modifier
    pub type_modifier: i32,
    /// Format code (0 = text, 1 = binary)
    pub format_code: i16,
}

/// Data row message
#[derive(Debug, Clone)]
pub struct DataRowMessage {
    /// Column values (None = NULL)
    pub values: Vec<Option<Vec<u8>>>,
}

/// Authentication message
#[derive(Debug, Clone)]
pub enum AuthenticationMessage {
    /// Authentication successful
    Ok,
    /// Kerberos V5 required
    KerberosV5,
    /// Clear-text password required
    CleartextPassword,
    /// MD5 password required (with salt)
    Md5Password { salt: [u8; 4] },
    /// SASL authentication
    Sasl { mechanisms: Vec<String> },
    /// SASL continue
    SaslContinue { data: Vec<u8> },
    /// SASL final
    SaslFinal { data: Vec<u8> },
    /// Other authentication type
    Other { auth_type: u32 },
}

/// Check if the given bytes look like PostgreSQL protocol
pub fn is_postgres_protocol(data: &[u8]) -> bool {
    if data.len() < 8 {
        return false;
    }

    // Read the first 4 bytes as message length (big-endian)
    let length = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);

    // Startup message has format: length (4) + protocol version (4) + params
    // Check for protocol version 3.0 (0x00030000) or SSL request
    if length >= 8 && length < MAX_MESSAGE_SIZE {
        let code = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);

        // Protocol version 3.0
        if code == PROTOCOL_VERSION_3 {
            return true;
        }

        // SSL request
        if code == SSL_REQUEST_CODE {
            return true;
        }

        // Cancel request
        if code == CANCEL_REQUEST_CODE {
            return true;
        }
    }

    // Regular message format: type (1) + length (4) + payload
    // Check for common frontend message types
    let msg_type = data[0];
    let msg_length = u32::from_be_bytes([data[1], data[2], data[3], data[4]]);

    if msg_length >= 4 && msg_length < MAX_MESSAGE_SIZE {
        matches!(
            msg_type,
            b'Q' | b'P' | b'B' | b'E' | b'D' | b'C' | b'H' | b'S' | b'X' | b'p'
        )
    } else {
        false
    }
}

/// Parse a PostgreSQL message from the given buffer
///
/// Returns the parsed message and the number of bytes consumed
pub fn parse_message(
    data: &[u8],
    is_startup: bool,
) -> Result<(PostgresMessage, usize), ParseError> {
    if is_startup {
        parse_startup_message(data)
    } else {
        parse_regular_message(data)
    }
}

/// Parse a startup-phase message
fn parse_startup_message(data: &[u8]) -> Result<(PostgresMessage, usize), ParseError> {
    if data.len() < 8 {
        return Err(ParseError::Incomplete {
            needed: 8 - data.len(),
        });
    }

    let length = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;

    if length > MAX_MESSAGE_SIZE as usize {
        return Err(ParseError::InvalidLength(length as u32));
    }

    if data.len() < length {
        return Err(ParseError::Incomplete {
            needed: length - data.len(),
        });
    }

    let code = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);

    match code {
        SSL_REQUEST_CODE => Ok((PostgresMessage::SslRequest, length)),
        CANCEL_REQUEST_CODE => {
            if length < 16 {
                return Err(ParseError::InvalidLength(length as u32));
            }
            let pid = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);
            let secret_key = u32::from_be_bytes([data[12], data[13], data[14], data[15]]);
            Ok((PostgresMessage::CancelRequest { pid, secret_key }, length))
        }
        PROTOCOL_VERSION_3 => {
            // Parse parameters (null-terminated key-value pairs)
            let mut parameters = Vec::new();
            let mut offset = 8;

            while offset < length - 1 {
                // Parse key
                let key_end = data[offset..]
                    .iter()
                    .position(|&b| b == 0)
                    .ok_or(ParseError::InvalidUtf8)?;
                let key = String::from_utf8(data[offset..offset + key_end].to_vec())
                    .map_err(|_| ParseError::InvalidUtf8)?;
                offset += key_end + 1;

                if key.is_empty() {
                    break;
                }

                // Parse value
                let value_end = data[offset..]
                    .iter()
                    .position(|&b| b == 0)
                    .ok_or(ParseError::InvalidUtf8)?;
                let value = String::from_utf8(data[offset..offset + value_end].to_vec())
                    .map_err(|_| ParseError::InvalidUtf8)?;
                offset += value_end + 1;

                parameters.push((key, value));
            }

            Ok((
                PostgresMessage::Startup(StartupMessage {
                    protocol_version: code,
                    parameters,
                }),
                length,
            ))
        }
        _ => Err(ParseError::ProtocolError(format!(
            "unknown startup code: 0x{:08x}",
            code
        ))),
    }
}

/// Parse a regular (non-startup) message
fn parse_regular_message(data: &[u8]) -> Result<(PostgresMessage, usize), ParseError> {
    if data.len() < 5 {
        return Err(ParseError::Incomplete {
            needed: 5 - data.len(),
        });
    }

    let msg_type = data[0];
    let length = u32::from_be_bytes([data[1], data[2], data[3], data[4]]) as usize;
    let total_length = length + 1; // type byte + length field value

    if length < 4 || length > MAX_MESSAGE_SIZE as usize {
        return Err(ParseError::InvalidLength(length as u32));
    }

    if data.len() < total_length {
        return Err(ParseError::Incomplete {
            needed: total_length - data.len(),
        });
    }

    let payload = &data[5..total_length];

    let message = match msg_type {
        b'Q' => parse_query_message(payload)?,
        b'P' => parse_parse_message(payload)?,
        b'B' => parse_bind_message(payload)?,
        b'E' => parse_execute_message(payload)?,
        b'D' => parse_describe_message(payload)?,
        b'C' => parse_close_or_complete_message(payload, msg_type)?,
        b'S' => PostgresMessage::Sync,
        b'X' => PostgresMessage::Terminate,
        b'Z' => {
            if payload.is_empty() {
                return Err(ParseError::Incomplete { needed: 1 });
            }
            PostgresMessage::ReadyForQuery(TransactionStatus::try_from(payload[0])?)
        }
        b'T' => parse_row_description(payload)?,
        b'R' => parse_authentication(payload)?,
        b'1' | b'2' | b'3' | b'n' | b's' | b'I' | b'c' => {
            // Simple acknowledgment messages
            PostgresMessage::Other {
                message_type: msg_type,
                length: length as u32,
            }
        }
        _ => PostgresMessage::Other {
            message_type: msg_type,
            length: length as u32,
        },
    };

    Ok((message, total_length))
}

fn parse_query_message(payload: &[u8]) -> Result<PostgresMessage, ParseError> {
    // Query is a null-terminated string
    let query_end = payload
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(payload.len());
    let query =
        String::from_utf8(payload[..query_end].to_vec()).map_err(|_| ParseError::InvalidUtf8)?;
    Ok(PostgresMessage::Query(QueryMessage { query }))
}

fn parse_parse_message(payload: &[u8]) -> Result<PostgresMessage, ParseError> {
    let mut offset = 0;

    // Statement name (null-terminated)
    let name_end = payload[offset..]
        .iter()
        .position(|&b| b == 0)
        .ok_or(ParseError::InvalidUtf8)?;
    let statement_name = String::from_utf8(payload[offset..offset + name_end].to_vec())
        .map_err(|_| ParseError::InvalidUtf8)?;
    offset += name_end + 1;

    // Query (null-terminated)
    let query_end = payload[offset..]
        .iter()
        .position(|&b| b == 0)
        .ok_or(ParseError::InvalidUtf8)?;
    let query = String::from_utf8(payload[offset..offset + query_end].to_vec())
        .map_err(|_| ParseError::InvalidUtf8)?;
    offset += query_end + 1;

    // Number of parameter types
    if payload.len() < offset + 2 {
        return Err(ParseError::Incomplete {
            needed: offset + 2 - payload.len(),
        });
    }
    let num_params = u16::from_be_bytes([payload[offset], payload[offset + 1]]) as usize;
    offset += 2;

    // Parameter type OIDs
    let mut parameter_types = Vec::with_capacity(num_params);
    for _ in 0..num_params {
        if payload.len() < offset + 4 {
            return Err(ParseError::Incomplete {
                needed: offset + 4 - payload.len(),
            });
        }
        let oid = u32::from_be_bytes([
            payload[offset],
            payload[offset + 1],
            payload[offset + 2],
            payload[offset + 3],
        ]);
        parameter_types.push(oid);
        offset += 4;
    }

    Ok(PostgresMessage::Parse(ParseMessage {
        statement_name,
        query,
        parameter_types,
    }))
}

fn parse_bind_message(payload: &[u8]) -> Result<PostgresMessage, ParseError> {
    let mut offset = 0;

    // Portal name (null-terminated)
    let portal_end = payload[offset..]
        .iter()
        .position(|&b| b == 0)
        .ok_or(ParseError::InvalidUtf8)?;
    let portal_name = String::from_utf8(payload[offset..offset + portal_end].to_vec())
        .map_err(|_| ParseError::InvalidUtf8)?;
    offset += portal_end + 1;

    // Statement name (null-terminated)
    let stmt_end = payload[offset..]
        .iter()
        .position(|&b| b == 0)
        .ok_or(ParseError::InvalidUtf8)?;
    let statement_name = String::from_utf8(payload[offset..offset + stmt_end].to_vec())
        .map_err(|_| ParseError::InvalidUtf8)?;
    offset += stmt_end + 1;

    // Parameter format codes
    if payload.len() < offset + 2 {
        return Err(ParseError::Incomplete { needed: 2 });
    }
    let num_format_codes = u16::from_be_bytes([payload[offset], payload[offset + 1]]) as usize;
    offset += 2;

    let mut parameter_format_codes = Vec::with_capacity(num_format_codes);
    for _ in 0..num_format_codes {
        if payload.len() < offset + 2 {
            return Err(ParseError::Incomplete { needed: 2 });
        }
        let code = i16::from_be_bytes([payload[offset], payload[offset + 1]]);
        parameter_format_codes.push(code);
        offset += 2;
    }

    // Parameter values
    if payload.len() < offset + 2 {
        return Err(ParseError::Incomplete { needed: 2 });
    }
    let num_params = u16::from_be_bytes([payload[offset], payload[offset + 1]]) as usize;
    offset += 2;

    let mut parameters = Vec::with_capacity(num_params);
    for _ in 0..num_params {
        if payload.len() < offset + 4 {
            return Err(ParseError::Incomplete { needed: 4 });
        }
        let param_len = i32::from_be_bytes([
            payload[offset],
            payload[offset + 1],
            payload[offset + 2],
            payload[offset + 3],
        ]);
        offset += 4;

        if param_len < 0 {
            parameters.push(None);
        } else {
            let param_len = param_len as usize;
            if payload.len() < offset + param_len {
                return Err(ParseError::Incomplete {
                    needed: param_len - (payload.len() - offset),
                });
            }
            parameters.push(Some(payload[offset..offset + param_len].to_vec()));
            offset += param_len;
        }
    }

    // Result format codes
    let mut result_format_codes = Vec::new();
    if payload.len() >= offset + 2 {
        let num_result_codes = u16::from_be_bytes([payload[offset], payload[offset + 1]]) as usize;
        offset += 2;

        for _ in 0..num_result_codes {
            if payload.len() >= offset + 2 {
                let code = i16::from_be_bytes([payload[offset], payload[offset + 1]]);
                result_format_codes.push(code);
                offset += 2;
            }
        }
    }

    Ok(PostgresMessage::Bind(BindMessage {
        portal_name,
        statement_name,
        parameter_format_codes,
        parameters,
        result_format_codes,
    }))
}

fn parse_execute_message(payload: &[u8]) -> Result<PostgresMessage, ParseError> {
    let mut offset = 0;

    // Portal name (null-terminated)
    let portal_end = payload[offset..]
        .iter()
        .position(|&b| b == 0)
        .ok_or(ParseError::InvalidUtf8)?;
    let portal_name = String::from_utf8(payload[offset..offset + portal_end].to_vec())
        .map_err(|_| ParseError::InvalidUtf8)?;
    offset += portal_end + 1;

    // Max rows
    let max_rows = if payload.len() >= offset + 4 {
        u32::from_be_bytes([
            payload[offset],
            payload[offset + 1],
            payload[offset + 2],
            payload[offset + 3],
        ])
    } else {
        0
    };

    Ok(PostgresMessage::Execute(ExecuteMessage {
        portal_name,
        max_rows,
    }))
}

fn parse_describe_message(payload: &[u8]) -> Result<PostgresMessage, ParseError> {
    if payload.is_empty() {
        return Err(ParseError::Incomplete { needed: 1 });
    }

    let target_type = payload[0] as char;
    let offset = 1;

    let name_end = payload[offset..]
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(payload.len() - offset);
    let name = String::from_utf8(payload[offset..offset + name_end].to_vec())
        .map_err(|_| ParseError::InvalidUtf8)?;

    Ok(PostgresMessage::Describe(DescribeMessage {
        target_type,
        name,
    }))
}

fn parse_close_or_complete_message(
    payload: &[u8],
    msg_type: u8,
) -> Result<PostgresMessage, ParseError> {
    if msg_type == b'C' {
        // Check if this looks like a Close message (has target type) or CommandComplete
        if !payload.is_empty() && (payload[0] == b'S' || payload[0] == b'P') {
            // Close message
            let target_type = payload[0] as char;
            let offset = 1;

            let name_end = payload[offset..]
                .iter()
                .position(|&b| b == 0)
                .unwrap_or(payload.len() - offset);
            let name = String::from_utf8(payload[offset..offset + name_end].to_vec())
                .map_err(|_| ParseError::InvalidUtf8)?;

            return Ok(PostgresMessage::Close(CloseMessage { target_type, name }));
        }

        // CommandComplete message
        let tag_end = payload
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(payload.len());
        let tag =
            String::from_utf8(payload[..tag_end].to_vec()).map_err(|_| ParseError::InvalidUtf8)?;

        return Ok(PostgresMessage::CommandComplete(CommandCompleteMessage {
            tag,
        }));
    }

    Err(ParseError::UnknownMessageType(msg_type))
}

fn parse_row_description(payload: &[u8]) -> Result<PostgresMessage, ParseError> {
    if payload.len() < 2 {
        return Err(ParseError::Incomplete { needed: 2 });
    }

    let num_columns = u16::from_be_bytes([payload[0], payload[1]]) as usize;
    let mut offset = 2;
    let mut columns = Vec::with_capacity(num_columns);

    for _ in 0..num_columns {
        // Column name (null-terminated)
        let name_end = payload[offset..]
            .iter()
            .position(|&b| b == 0)
            .ok_or(ParseError::InvalidUtf8)?;
        let name = String::from_utf8(payload[offset..offset + name_end].to_vec())
            .map_err(|_| ParseError::InvalidUtf8)?;
        offset += name_end + 1;

        // Need 18 more bytes for the rest of the column info
        if payload.len() < offset + 18 {
            return Err(ParseError::Incomplete {
                needed: 18 - (payload.len() - offset),
            });
        }

        let table_oid = u32::from_be_bytes([
            payload[offset],
            payload[offset + 1],
            payload[offset + 2],
            payload[offset + 3],
        ]);
        offset += 4;

        let column_id = i16::from_be_bytes([payload[offset], payload[offset + 1]]);
        offset += 2;

        let type_oid = u32::from_be_bytes([
            payload[offset],
            payload[offset + 1],
            payload[offset + 2],
            payload[offset + 3],
        ]);
        offset += 4;

        let type_size = i16::from_be_bytes([payload[offset], payload[offset + 1]]);
        offset += 2;

        let type_modifier = i32::from_be_bytes([
            payload[offset],
            payload[offset + 1],
            payload[offset + 2],
            payload[offset + 3],
        ]);
        offset += 4;

        let format_code = i16::from_be_bytes([payload[offset], payload[offset + 1]]);
        offset += 2;

        columns.push(ColumnDescription {
            name,
            table_oid,
            column_id,
            type_oid,
            type_size,
            type_modifier,
            format_code,
        });
    }

    Ok(PostgresMessage::RowDescription(RowDescriptionMessage {
        columns,
    }))
}

fn parse_authentication(payload: &[u8]) -> Result<PostgresMessage, ParseError> {
    if payload.len() < 4 {
        return Err(ParseError::Incomplete { needed: 4 });
    }

    let auth_type = u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]);

    let auth = match auth_type {
        0 => AuthenticationMessage::Ok,
        2 => AuthenticationMessage::KerberosV5,
        3 => AuthenticationMessage::CleartextPassword,
        5 => {
            if payload.len() < 8 {
                return Err(ParseError::Incomplete { needed: 4 });
            }
            let mut salt = [0u8; 4];
            salt.copy_from_slice(&payload[4..8]);
            AuthenticationMessage::Md5Password { salt }
        }
        10 => {
            // SASL - parse mechanism names
            let mut mechanisms = Vec::new();
            let mut offset = 4;
            while offset < payload.len() {
                let mech_end = payload[offset..]
                    .iter()
                    .position(|&b| b == 0)
                    .unwrap_or(payload.len() - offset);
                if mech_end == 0 {
                    break;
                }
                if let Ok(mech) = String::from_utf8(payload[offset..offset + mech_end].to_vec()) {
                    mechanisms.push(mech);
                }
                offset += mech_end + 1;
            }
            AuthenticationMessage::Sasl { mechanisms }
        }
        11 => AuthenticationMessage::SaslContinue {
            data: payload[4..].to_vec(),
        },
        12 => AuthenticationMessage::SaslFinal {
            data: payload[4..].to_vec(),
        },
        _ => AuthenticationMessage::Other { auth_type },
    };

    Ok(PostgresMessage::Authentication(auth))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_postgres_protocol() {
        // Protocol version 3.0 startup message
        let startup = [
            0, 0, 0, 8, // length
            0, 3, 0, 0, // protocol version 3.0
        ];
        assert!(is_postgres_protocol(&startup));

        // SSL request
        let ssl_request = [
            0, 0, 0, 8, // length
            0x04, 0xd2, 0x16, 0x2f, // SSL request code
        ];
        assert!(is_postgres_protocol(&ssl_request));

        // Regular query message
        let query = [
            b'Q', // message type
            0, 0, 0, 10, // length
            b'S', b'E', b'L', b'E', b'C', b'T', // "SELECT"
        ];
        assert!(is_postgres_protocol(&query));

        // Invalid - too short
        assert!(!is_postgres_protocol(&[0, 0, 0]));

        // Invalid - not PostgreSQL
        let invalid = [0, 0, 0, 8, 0, 0, 0, 0];
        assert!(!is_postgres_protocol(&invalid));
    }

    #[test]
    fn test_parse_query_message() {
        // 'Q' type + length + "SELECT 1" + null
        let data = [
            b'Q', 0, 0, 0, 13, // length (including length field)
            b'S', b'E', b'L', b'E', b'C', b'T', b' ', b'1', 0,
        ];

        let (msg, consumed) = parse_message(&data, false).unwrap();
        assert_eq!(consumed, 14);

        if let PostgresMessage::Query(q) = msg {
            assert_eq!(q.query, "SELECT 1");
        } else {
            panic!("Expected Query message");
        }
    }

    #[test]
    fn test_command_complete_row_count() {
        let complete = CommandCompleteMessage {
            tag: "SELECT 5".to_string(),
        };
        assert_eq!(complete.command(), "SELECT");
        assert_eq!(complete.row_count(), Some(5));

        let insert = CommandCompleteMessage {
            tag: "INSERT 0 3".to_string(),
        };
        assert_eq!(insert.command(), "INSERT");
        assert_eq!(insert.row_count(), Some(3));
    }

    #[test]
    fn test_frontend_message_type() {
        assert_eq!(
            FrontendMessageType::try_from(b'Q').unwrap(),
            FrontendMessageType::Query
        );
        assert_eq!(
            FrontendMessageType::try_from(b'P').unwrap(),
            FrontendMessageType::Parse
        );
        assert!(FrontendMessageType::try_from(b'Z').is_err());
    }

    #[test]
    fn test_transaction_status() {
        assert_eq!(
            TransactionStatus::try_from(b'I').unwrap(),
            TransactionStatus::Idle
        );
        assert_eq!(
            TransactionStatus::try_from(b'T').unwrap(),
            TransactionStatus::InTransaction
        );
        assert_eq!(
            TransactionStatus::try_from(b'E').unwrap(),
            TransactionStatus::Failed
        );
    }
}
