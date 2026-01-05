//! Protocol Detection Module for Roxy
//!
//! This module provides wire protocol detection and parsing for various
//! protocols that pass through the SOCKS proxy, including:
//!
//! - PostgreSQL (wire protocol v3.0)
//! - Kafka (binary protocol)
//!
//! ## Detection Strategy
//!
//! When a SOCKS connection is established, we peek at the initial bytes
//! to detect the protocol. Each protocol has distinctive signatures:
//!
//! - PostgreSQL: Starts with length (4 bytes) + protocol version or message type
//! - Kafka: Starts with length (4 bytes) + API key (2 bytes) + API version (2 bytes)

pub mod intercept;
pub mod kafka;
pub mod postgres;

pub use intercept::{run_intercepting_tunnel, InterceptConfig, InterceptStats};

use std::fmt;

/// Default port numbers for common protocols
pub const POSTGRES_DEFAULT_PORT: u16 = 5432;
pub const KAFKA_DEFAULT_PORT: u16 = 9092;

/// Detected protocol type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Protocol {
    /// PostgreSQL wire protocol (v3.0)
    PostgreSQL,
    /// Apache Kafka binary protocol
    Kafka,
    /// Unknown/unrecognized protocol
    Unknown,
}

impl Protocol {
    /// Check if this protocol is a database protocol
    pub fn is_database(&self) -> bool {
        matches!(self, Protocol::PostgreSQL)
    }

    /// Check if this protocol is a messaging protocol
    pub fn is_messaging(&self) -> bool {
        matches!(self, Protocol::Kafka)
    }

    /// Get the default port for this protocol
    pub fn default_port(&self) -> Option<u16> {
        match self {
            Protocol::PostgreSQL => Some(POSTGRES_DEFAULT_PORT),
            Protocol::Kafka => Some(KAFKA_DEFAULT_PORT),
            Protocol::Unknown => None,
        }
    }

    /// Get the protocol name as used in spans
    pub fn span_name(&self) -> &'static str {
        match self {
            Protocol::PostgreSQL => "postgresql",
            Protocol::Kafka => "kafka",
            Protocol::Unknown => "unknown",
        }
    }

    /// Get the OpenTelemetry semantic convention db.system value
    pub fn db_system(&self) -> Option<&'static str> {
        match self {
            Protocol::PostgreSQL => Some("postgresql"),
            Protocol::Kafka => None,
            Protocol::Unknown => None,
        }
    }

    /// Get the OpenTelemetry semantic convention messaging.system value
    pub fn messaging_system(&self) -> Option<&'static str> {
        match self {
            Protocol::PostgreSQL => None,
            Protocol::Kafka => Some("kafka"),
            Protocol::Unknown => None,
        }
    }
}

impl fmt::Display for Protocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Protocol::PostgreSQL => write!(f, "PostgreSQL"),
            Protocol::Kafka => write!(f, "Kafka"),
            Protocol::Unknown => write!(f, "Unknown"),
        }
    }
}

/// Detect the protocol from initial connection bytes
///
/// This function examines the first bytes of a connection to determine
/// which protocol is being used. It requires at least 8 bytes for reliable
/// detection.
///
/// # Arguments
/// * `data` - Initial bytes from the connection (at least 8 bytes recommended)
/// * `port` - The destination port (used as a hint)
///
/// # Returns
/// The detected protocol, or `Protocol::Unknown` if unrecognized
pub fn detect_protocol(data: &[u8], port: u16) -> Protocol {
    // Need at least 8 bytes for reliable detection
    if data.len() < 8 {
        // Fall back to port-based detection
        return detect_by_port(port);
    }

    // Try PostgreSQL detection first
    if postgres::is_postgres_protocol(data) {
        return Protocol::PostgreSQL;
    }

    // Try Kafka detection
    if kafka::is_kafka_protocol(data) {
        return Protocol::Kafka;
    }

    // Fall back to port-based detection
    detect_by_port(port)
}

/// Detect protocol based on port number alone
///
/// This is a fallback when we can't determine protocol from data
fn detect_by_port(port: u16) -> Protocol {
    match port {
        5432 => Protocol::PostgreSQL,
        9092 => Protocol::Kafka,
        _ => Protocol::Unknown,
    }
}

/// Minimum bytes needed for reliable protocol detection
pub const MIN_DETECTION_BYTES: usize = 8;

/// Result of parsing a protocol message
#[derive(Debug, Clone)]
pub enum ParsedMessage {
    /// A PostgreSQL message was parsed
    PostgreSQL(postgres::PostgresMessage),
    /// A Kafka message was parsed
    Kafka(kafka::KafkaMessage),
    /// Incomplete data - need more bytes
    Incomplete { needed: usize },
    /// Parse error
    Error(String),
}

/// Direction of a message in the protocol
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageDirection {
    /// Message from client to server (request)
    ClientToServer,
    /// Message from server to client (response)
    ServerToClient,
}

impl fmt::Display for MessageDirection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MessageDirection::ClientToServer => write!(f, "request"),
            MessageDirection::ServerToClient => write!(f, "response"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_display() {
        assert_eq!(Protocol::PostgreSQL.to_string(), "PostgreSQL");
        assert_eq!(Protocol::Kafka.to_string(), "Kafka");
        assert_eq!(Protocol::Unknown.to_string(), "Unknown");
    }

    #[test]
    fn test_protocol_default_port() {
        assert_eq!(Protocol::PostgreSQL.default_port(), Some(5432));
        assert_eq!(Protocol::Kafka.default_port(), Some(9092));
        assert_eq!(Protocol::Unknown.default_port(), None);
    }

    #[test]
    fn test_detect_by_port() {
        assert_eq!(detect_by_port(5432), Protocol::PostgreSQL);
        assert_eq!(detect_by_port(9092), Protocol::Kafka);
        assert_eq!(detect_by_port(8080), Protocol::Unknown);
    }

    #[test]
    fn test_protocol_categories() {
        assert!(Protocol::PostgreSQL.is_database());
        assert!(!Protocol::PostgreSQL.is_messaging());

        assert!(!Protocol::Kafka.is_database());
        assert!(Protocol::Kafka.is_messaging());

        assert!(!Protocol::Unknown.is_database());
        assert!(!Protocol::Unknown.is_messaging());
    }

    #[test]
    fn test_span_names() {
        assert_eq!(Protocol::PostgreSQL.span_name(), "postgresql");
        assert_eq!(Protocol::Kafka.span_name(), "kafka");
    }

    #[test]
    fn test_otel_systems() {
        assert_eq!(Protocol::PostgreSQL.db_system(), Some("postgresql"));
        assert_eq!(Protocol::PostgreSQL.messaging_system(), None);

        assert_eq!(Protocol::Kafka.db_system(), None);
        assert_eq!(Protocol::Kafka.messaging_system(), Some("kafka"));
    }
}
