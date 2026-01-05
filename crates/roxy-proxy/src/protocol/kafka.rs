//! Kafka Binary Protocol Parser
//!
//! This module implements parsing of the Apache Kafka wire protocol
//! for extracting message information and creating trace spans.
//!
//! ## Protocol Overview
//!
//! Kafka uses a request-response binary protocol where:
//! - Requests: length (4 bytes) + api_key (2) + api_version (2) + correlation_id (4) + client_id + payload
//! - Responses: length (4 bytes) + correlation_id (4) + payload
//!
//! ## References
//! - https://kafka.apache.org/protocol.html

use std::fmt;

/// Maximum reasonable message size (256 MB)
const MAX_MESSAGE_SIZE: u32 = 256 * 1024 * 1024;

/// Minimum request size (length + api_key + api_version + correlation_id)
const MIN_REQUEST_SIZE: usize = 12;

/// Kafka API Keys
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i16)]
pub enum ApiKey {
    /// Produce messages to topic partitions
    Produce = 0,
    /// Fetch messages from topic partitions
    Fetch = 1,
    /// List available offsets for topic partitions
    ListOffsets = 2,
    /// Get cluster metadata
    Metadata = 3,
    /// Leader and ISR updates (internal)
    LeaderAndIsr = 4,
    /// Stop replica (internal)
    StopReplica = 5,
    /// Update metadata (internal)
    UpdateMetadata = 6,
    /// Controlled shutdown
    ControlledShutdown = 7,
    /// Offset commit for consumer groups
    OffsetCommit = 8,
    /// Offset fetch for consumer groups
    OffsetFetch = 9,
    /// Find coordinator (group or transaction)
    FindCoordinator = 10,
    /// Join consumer group
    JoinGroup = 11,
    /// Heartbeat for consumer group
    Heartbeat = 12,
    /// Leave consumer group
    LeaveGroup = 13,
    /// Sync consumer group
    SyncGroup = 14,
    /// Describe consumer groups
    DescribeGroups = 15,
    /// List consumer groups
    ListGroups = 16,
    /// SASL handshake
    SaslHandshake = 17,
    /// API versions query
    ApiVersions = 18,
    /// Create topics
    CreateTopics = 19,
    /// Delete topics
    DeleteTopics = 20,
    /// Delete records
    DeleteRecords = 21,
    /// Init producer ID (transactions)
    InitProducerId = 22,
    /// Offset for leader epoch
    OffsetForLeaderEpoch = 23,
    /// Add partitions to transaction
    AddPartitionsToTxn = 24,
    /// Add offsets to transaction
    AddOffsetsToTxn = 25,
    /// End transaction
    EndTxn = 26,
    /// Write transaction markers
    WriteTxnMarkers = 27,
    /// Transaction offset commit
    TxnOffsetCommit = 28,
    /// Describe ACLs
    DescribeAcls = 29,
    /// Create ACLs
    CreateAcls = 30,
    /// Delete ACLs
    DeleteAcls = 31,
    /// Describe configs
    DescribeConfigs = 32,
    /// Alter configs
    AlterConfigs = 33,
    /// Alter replica log dirs
    AlterReplicaLogDirs = 34,
    /// Describe log dirs
    DescribeLogDirs = 35,
    /// SASL authenticate
    SaslAuthenticate = 36,
    /// Create partitions
    CreatePartitions = 37,
    /// Create delegation token
    CreateDelegationToken = 38,
    /// Renew delegation token
    RenewDelegationToken = 39,
    /// Expire delegation token
    ExpireDelegationToken = 40,
    /// Describe delegation token
    DescribeDelegationToken = 41,
    /// Delete groups
    DeleteGroups = 42,
    /// Elect leaders
    ElectLeaders = 43,
    /// Incremental alter configs
    IncrementalAlterConfigs = 44,
    /// Alter partition reassignments
    AlterPartitionReassignments = 45,
    /// List partition reassignments
    ListPartitionReassignments = 46,
    /// Offset delete
    OffsetDelete = 47,
    /// Describe client quotas
    DescribeClientQuotas = 48,
    /// Alter client quotas
    AlterClientQuotas = 49,
    /// Describe user SCRAM credentials
    DescribeUserScramCredentials = 50,
    /// Alter user SCRAM credentials
    AlterUserScramCredentials = 51,
    /// Describe quorum (KRaft)
    DescribeQuorum = 55,
    /// Alter partition
    AlterPartition = 56,
    /// Update features
    UpdateFeatures = 57,
    /// Envelope (for forwarding)
    Envelope = 58,
    /// Fetch snapshot (KRaft)
    FetchSnapshot = 59,
    /// Describe cluster
    DescribeCluster = 60,
    /// Describe producers
    DescribeProducers = 61,
    /// Broker registration (KRaft)
    BrokerRegistration = 62,
    /// Broker heartbeat (KRaft)
    BrokerHeartbeat = 63,
    /// Unregister broker
    UnregisterBroker = 64,
    /// Describe transactions
    DescribeTransactions = 65,
    /// List transactions
    ListTransactions = 66,
    /// Allocate producer IDs
    AllocateProducerIds = 67,
    /// Consumer group heartbeat
    ConsumerGroupHeartbeat = 68,
    /// Unknown/unsupported API key
    Unknown = -1,
}

impl ApiKey {
    /// Create from i16 value
    pub fn from_i16(value: i16) -> Self {
        match value {
            0 => ApiKey::Produce,
            1 => ApiKey::Fetch,
            2 => ApiKey::ListOffsets,
            3 => ApiKey::Metadata,
            4 => ApiKey::LeaderAndIsr,
            5 => ApiKey::StopReplica,
            6 => ApiKey::UpdateMetadata,
            7 => ApiKey::ControlledShutdown,
            8 => ApiKey::OffsetCommit,
            9 => ApiKey::OffsetFetch,
            10 => ApiKey::FindCoordinator,
            11 => ApiKey::JoinGroup,
            12 => ApiKey::Heartbeat,
            13 => ApiKey::LeaveGroup,
            14 => ApiKey::SyncGroup,
            15 => ApiKey::DescribeGroups,
            16 => ApiKey::ListGroups,
            17 => ApiKey::SaslHandshake,
            18 => ApiKey::ApiVersions,
            19 => ApiKey::CreateTopics,
            20 => ApiKey::DeleteTopics,
            21 => ApiKey::DeleteRecords,
            22 => ApiKey::InitProducerId,
            23 => ApiKey::OffsetForLeaderEpoch,
            24 => ApiKey::AddPartitionsToTxn,
            25 => ApiKey::AddOffsetsToTxn,
            26 => ApiKey::EndTxn,
            27 => ApiKey::WriteTxnMarkers,
            28 => ApiKey::TxnOffsetCommit,
            29 => ApiKey::DescribeAcls,
            30 => ApiKey::CreateAcls,
            31 => ApiKey::DeleteAcls,
            32 => ApiKey::DescribeConfigs,
            33 => ApiKey::AlterConfigs,
            34 => ApiKey::AlterReplicaLogDirs,
            35 => ApiKey::DescribeLogDirs,
            36 => ApiKey::SaslAuthenticate,
            37 => ApiKey::CreatePartitions,
            38 => ApiKey::CreateDelegationToken,
            39 => ApiKey::RenewDelegationToken,
            40 => ApiKey::ExpireDelegationToken,
            41 => ApiKey::DescribeDelegationToken,
            42 => ApiKey::DeleteGroups,
            43 => ApiKey::ElectLeaders,
            44 => ApiKey::IncrementalAlterConfigs,
            45 => ApiKey::AlterPartitionReassignments,
            46 => ApiKey::ListPartitionReassignments,
            47 => ApiKey::OffsetDelete,
            48 => ApiKey::DescribeClientQuotas,
            49 => ApiKey::AlterClientQuotas,
            50 => ApiKey::DescribeUserScramCredentials,
            51 => ApiKey::AlterUserScramCredentials,
            55 => ApiKey::DescribeQuorum,
            56 => ApiKey::AlterPartition,
            57 => ApiKey::UpdateFeatures,
            58 => ApiKey::Envelope,
            59 => ApiKey::FetchSnapshot,
            60 => ApiKey::DescribeCluster,
            61 => ApiKey::DescribeProducers,
            62 => ApiKey::BrokerRegistration,
            63 => ApiKey::BrokerHeartbeat,
            64 => ApiKey::UnregisterBroker,
            65 => ApiKey::DescribeTransactions,
            66 => ApiKey::ListTransactions,
            67 => ApiKey::AllocateProducerIds,
            68 => ApiKey::ConsumerGroupHeartbeat,
            _ => ApiKey::Unknown,
        }
    }

    /// Get the human-readable name for this API
    pub fn name(&self) -> &'static str {
        match self {
            ApiKey::Produce => "Produce",
            ApiKey::Fetch => "Fetch",
            ApiKey::ListOffsets => "ListOffsets",
            ApiKey::Metadata => "Metadata",
            ApiKey::LeaderAndIsr => "LeaderAndIsr",
            ApiKey::StopReplica => "StopReplica",
            ApiKey::UpdateMetadata => "UpdateMetadata",
            ApiKey::ControlledShutdown => "ControlledShutdown",
            ApiKey::OffsetCommit => "OffsetCommit",
            ApiKey::OffsetFetch => "OffsetFetch",
            ApiKey::FindCoordinator => "FindCoordinator",
            ApiKey::JoinGroup => "JoinGroup",
            ApiKey::Heartbeat => "Heartbeat",
            ApiKey::LeaveGroup => "LeaveGroup",
            ApiKey::SyncGroup => "SyncGroup",
            ApiKey::DescribeGroups => "DescribeGroups",
            ApiKey::ListGroups => "ListGroups",
            ApiKey::SaslHandshake => "SaslHandshake",
            ApiKey::ApiVersions => "ApiVersions",
            ApiKey::CreateTopics => "CreateTopics",
            ApiKey::DeleteTopics => "DeleteTopics",
            ApiKey::DeleteRecords => "DeleteRecords",
            ApiKey::InitProducerId => "InitProducerId",
            ApiKey::OffsetForLeaderEpoch => "OffsetForLeaderEpoch",
            ApiKey::AddPartitionsToTxn => "AddPartitionsToTxn",
            ApiKey::AddOffsetsToTxn => "AddOffsetsToTxn",
            ApiKey::EndTxn => "EndTxn",
            ApiKey::WriteTxnMarkers => "WriteTxnMarkers",
            ApiKey::TxnOffsetCommit => "TxnOffsetCommit",
            ApiKey::DescribeAcls => "DescribeAcls",
            ApiKey::CreateAcls => "CreateAcls",
            ApiKey::DeleteAcls => "DeleteAcls",
            ApiKey::DescribeConfigs => "DescribeConfigs",
            ApiKey::AlterConfigs => "AlterConfigs",
            ApiKey::AlterReplicaLogDirs => "AlterReplicaLogDirs",
            ApiKey::DescribeLogDirs => "DescribeLogDirs",
            ApiKey::SaslAuthenticate => "SaslAuthenticate",
            ApiKey::CreatePartitions => "CreatePartitions",
            ApiKey::CreateDelegationToken => "CreateDelegationToken",
            ApiKey::RenewDelegationToken => "RenewDelegationToken",
            ApiKey::ExpireDelegationToken => "ExpireDelegationToken",
            ApiKey::DescribeDelegationToken => "DescribeDelegationToken",
            ApiKey::DeleteGroups => "DeleteGroups",
            ApiKey::ElectLeaders => "ElectLeaders",
            ApiKey::IncrementalAlterConfigs => "IncrementalAlterConfigs",
            ApiKey::AlterPartitionReassignments => "AlterPartitionReassignments",
            ApiKey::ListPartitionReassignments => "ListPartitionReassignments",
            ApiKey::OffsetDelete => "OffsetDelete",
            ApiKey::DescribeClientQuotas => "DescribeClientQuotas",
            ApiKey::AlterClientQuotas => "AlterClientQuotas",
            ApiKey::DescribeUserScramCredentials => "DescribeUserScramCredentials",
            ApiKey::AlterUserScramCredentials => "AlterUserScramCredentials",
            ApiKey::DescribeQuorum => "DescribeQuorum",
            ApiKey::AlterPartition => "AlterPartition",
            ApiKey::UpdateFeatures => "UpdateFeatures",
            ApiKey::Envelope => "Envelope",
            ApiKey::FetchSnapshot => "FetchSnapshot",
            ApiKey::DescribeCluster => "DescribeCluster",
            ApiKey::DescribeProducers => "DescribeProducers",
            ApiKey::BrokerRegistration => "BrokerRegistration",
            ApiKey::BrokerHeartbeat => "BrokerHeartbeat",
            ApiKey::UnregisterBroker => "UnregisterBroker",
            ApiKey::DescribeTransactions => "DescribeTransactions",
            ApiKey::ListTransactions => "ListTransactions",
            ApiKey::AllocateProducerIds => "AllocateProducerIds",
            ApiKey::ConsumerGroupHeartbeat => "ConsumerGroupHeartbeat",
            ApiKey::Unknown => "Unknown",
        }
    }

    /// Check if this is a produce-related operation
    pub fn is_produce(&self) -> bool {
        matches!(self, ApiKey::Produce)
    }

    /// Check if this is a consume-related operation
    pub fn is_consume(&self) -> bool {
        matches!(
            self,
            ApiKey::Fetch | ApiKey::OffsetFetch | ApiKey::OffsetCommit
        )
    }

    /// Check if this is a consumer group operation
    pub fn is_consumer_group(&self) -> bool {
        matches!(
            self,
            ApiKey::FindCoordinator
                | ApiKey::JoinGroup
                | ApiKey::Heartbeat
                | ApiKey::LeaveGroup
                | ApiKey::SyncGroup
                | ApiKey::DescribeGroups
                | ApiKey::ListGroups
                | ApiKey::DeleteGroups
                | ApiKey::OffsetCommit
                | ApiKey::OffsetFetch
                | ApiKey::ConsumerGroupHeartbeat
        )
    }

    /// Check if this is an admin operation
    pub fn is_admin(&self) -> bool {
        matches!(
            self,
            ApiKey::CreateTopics
                | ApiKey::DeleteTopics
                | ApiKey::CreatePartitions
                | ApiKey::DescribeConfigs
                | ApiKey::AlterConfigs
                | ApiKey::IncrementalAlterConfigs
                | ApiKey::CreateAcls
                | ApiKey::DeleteAcls
                | ApiKey::DescribeAcls
        )
    }

    /// Check if this is a metadata/info operation
    pub fn is_metadata(&self) -> bool {
        matches!(
            self,
            ApiKey::Metadata | ApiKey::ApiVersions | ApiKey::DescribeCluster
        )
    }

    /// Get the OpenTelemetry messaging.operation type
    pub fn otel_operation(&self) -> &'static str {
        if self.is_produce() {
            "publish"
        } else if self.is_consume() {
            "receive"
        } else {
            "process"
        }
    }
}

impl fmt::Display for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// Parse errors for Kafka protocol
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// Not enough data to parse
    Incomplete { needed: usize },
    /// Invalid message length
    InvalidLength(u32),
    /// Invalid API key
    InvalidApiKey(i16),
    /// Invalid string encoding
    InvalidString,
    /// Protocol error
    ProtocolError(String),
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::Incomplete { needed } => {
                write!(f, "incomplete message, need {} more bytes", needed)
            }
            ParseError::InvalidLength(len) => write!(f, "invalid message length: {}", len),
            ParseError::InvalidApiKey(key) => write!(f, "invalid API key: {}", key),
            ParseError::InvalidString => write!(f, "invalid string encoding"),
            ParseError::ProtocolError(msg) => write!(f, "protocol error: {}", msg),
        }
    }
}

impl std::error::Error for ParseError {}

/// A parsed Kafka message
#[derive(Debug, Clone)]
pub enum KafkaMessage {
    /// A Kafka request
    Request(KafkaRequest),
    /// A Kafka response
    Response(KafkaResponse),
}

impl KafkaMessage {
    /// Get a description suitable for span names
    pub fn span_name(&self) -> String {
        match self {
            KafkaMessage::Request(req) => format!("kafka.{}", req.api_key.name().to_lowercase()),
            KafkaMessage::Response(resp) => {
                format!("kafka.{}.response", resp.api_key.name().to_lowercase())
            }
        }
    }

    /// Get the correlation ID for matching requests to responses
    pub fn correlation_id(&self) -> i32 {
        match self {
            KafkaMessage::Request(req) => req.correlation_id,
            KafkaMessage::Response(resp) => resp.correlation_id,
        }
    }

    /// Get the API key
    pub fn api_key(&self) -> Option<ApiKey> {
        match self {
            KafkaMessage::Request(req) => Some(req.api_key),
            KafkaMessage::Response(resp) => Some(resp.api_key),
        }
    }

    /// Get topics if this is a produce or fetch request
    pub fn topics(&self) -> Option<&[String]> {
        match self {
            KafkaMessage::Request(req) => match &req.details {
                RequestDetails::Produce(p) => Some(&p.topics),
                RequestDetails::Fetch(f) => Some(&f.topics),
                RequestDetails::Metadata(m) => Some(&m.topics),
                _ => None,
            },
            KafkaMessage::Response(_) => None,
        }
    }

    /// Get the consumer group ID if applicable
    pub fn group_id(&self) -> Option<&str> {
        match self {
            KafkaMessage::Request(req) => match &req.details {
                RequestDetails::JoinGroup(j) => Some(&j.group_id),
                RequestDetails::Heartbeat(h) => Some(&h.group_id),
                RequestDetails::LeaveGroup(l) => Some(&l.group_id),
                RequestDetails::SyncGroup(s) => Some(&s.group_id),
                RequestDetails::OffsetCommit(o) => Some(&o.group_id),
                RequestDetails::OffsetFetch(o) => Some(&o.group_id),
                _ => None,
            },
            KafkaMessage::Response(_) => None,
        }
    }
}

/// A Kafka request message
#[derive(Debug, Clone)]
pub struct KafkaRequest {
    /// Total message length
    pub length: u32,
    /// API key identifying the request type
    pub api_key: ApiKey,
    /// API version
    pub api_version: i16,
    /// Correlation ID for matching responses
    pub correlation_id: i32,
    /// Client ID (nullable)
    pub client_id: Option<String>,
    /// Parsed request details (if available)
    pub details: RequestDetails,
}

/// Detailed request information for specific API types
#[derive(Debug, Clone)]
pub enum RequestDetails {
    /// Produce request details
    Produce(ProduceRequest),
    /// Fetch request details
    Fetch(FetchRequest),
    /// Metadata request details
    Metadata(MetadataRequest),
    /// JoinGroup request details
    JoinGroup(JoinGroupRequest),
    /// Heartbeat request details
    Heartbeat(HeartbeatRequest),
    /// LeaveGroup request details
    LeaveGroup(LeaveGroupRequest),
    /// SyncGroup request details
    SyncGroup(SyncGroupRequest),
    /// OffsetCommit request details
    OffsetCommit(OffsetCommitRequest),
    /// OffsetFetch request details
    OffsetFetch(OffsetFetchRequest),
    /// ApiVersions request (usually no details)
    ApiVersions,
    /// FindCoordinator request details
    FindCoordinator(FindCoordinatorRequest),
    /// Generic/unparsed request
    Other { raw_length: u32 },
}

/// Produce request details
#[derive(Debug, Clone)]
pub struct ProduceRequest {
    /// Required acks (-1, 0, 1)
    pub acks: i16,
    /// Timeout in milliseconds
    pub timeout_ms: i32,
    /// Target topics
    pub topics: Vec<String>,
    /// Total message count (if parseable)
    pub message_count: Option<u32>,
}

/// Fetch request details
#[derive(Debug, Clone)]
pub struct FetchRequest {
    /// Maximum wait time in ms
    pub max_wait_ms: i32,
    /// Minimum bytes to return
    pub min_bytes: i32,
    /// Target topics
    pub topics: Vec<String>,
}

/// Metadata request details
#[derive(Debug, Clone)]
pub struct MetadataRequest {
    /// Topics to get metadata for (empty = all topics)
    pub topics: Vec<String>,
}

/// JoinGroup request details
#[derive(Debug, Clone)]
pub struct JoinGroupRequest {
    /// Consumer group ID
    pub group_id: String,
    /// Session timeout in ms
    pub session_timeout_ms: i32,
    /// Member ID (empty for new members)
    pub member_id: String,
    /// Protocol type (usually "consumer")
    pub protocol_type: String,
}

/// Heartbeat request details
#[derive(Debug, Clone)]
pub struct HeartbeatRequest {
    /// Consumer group ID
    pub group_id: String,
    /// Generation ID
    pub generation_id: i32,
    /// Member ID
    pub member_id: String,
}

/// LeaveGroup request details
#[derive(Debug, Clone)]
pub struct LeaveGroupRequest {
    /// Consumer group ID
    pub group_id: String,
    /// Member ID
    pub member_id: String,
}

/// SyncGroup request details
#[derive(Debug, Clone)]
pub struct SyncGroupRequest {
    /// Consumer group ID
    pub group_id: String,
    /// Generation ID
    pub generation_id: i32,
    /// Member ID
    pub member_id: String,
}

/// OffsetCommit request details
#[derive(Debug, Clone)]
pub struct OffsetCommitRequest {
    /// Consumer group ID
    pub group_id: String,
    /// Topics with offsets to commit
    pub topics: Vec<String>,
}

/// OffsetFetch request details
#[derive(Debug, Clone)]
pub struct OffsetFetchRequest {
    /// Consumer group ID
    pub group_id: String,
    /// Topics to fetch offsets for
    pub topics: Vec<String>,
}

/// FindCoordinator request details
#[derive(Debug, Clone)]
pub struct FindCoordinatorRequest {
    /// Key to find coordinator for (group ID or transactional ID)
    pub key: String,
    /// Key type (0 = group, 1 = transaction)
    pub key_type: i8,
}

/// A Kafka response message
#[derive(Debug, Clone)]
pub struct KafkaResponse {
    /// Total message length
    pub length: u32,
    /// Correlation ID matching the request
    pub correlation_id: i32,
    /// API key (if known from request tracking)
    pub api_key: ApiKey,
    /// Error code (if applicable)
    pub error_code: Option<i16>,
    /// Parsed response details
    pub details: ResponseDetails,
}

/// Detailed response information
#[derive(Debug, Clone)]
pub enum ResponseDetails {
    /// Produce response
    Produce(ProduceResponse),
    /// Fetch response
    Fetch(FetchResponse),
    /// Metadata response
    Metadata(MetadataResponse),
    /// Generic response
    Other { raw_length: u32 },
}

/// Produce response details
#[derive(Debug, Clone)]
pub struct ProduceResponse {
    /// Per-topic results
    pub topics: Vec<TopicProduceResponse>,
}

/// Per-topic produce response
#[derive(Debug, Clone)]
pub struct TopicProduceResponse {
    /// Topic name
    pub name: String,
    /// Per-partition error codes
    pub partition_errors: Vec<i16>,
}

/// Fetch response details
#[derive(Debug, Clone)]
pub struct FetchResponse {
    /// Throttle time in ms (if applicable)
    pub throttle_time_ms: Option<i32>,
    /// Per-topic results
    pub topics: Vec<TopicFetchResponse>,
}

/// Per-topic fetch response
#[derive(Debug, Clone)]
pub struct TopicFetchResponse {
    /// Topic name
    pub name: String,
    /// Number of records fetched
    pub record_count: u32,
}

/// Metadata response details
#[derive(Debug, Clone)]
pub struct MetadataResponse {
    /// Broker information
    pub brokers: Vec<BrokerInfo>,
    /// Topic metadata
    pub topics: Vec<TopicMetadata>,
}

/// Broker information
#[derive(Debug, Clone)]
pub struct BrokerInfo {
    /// Node ID
    pub node_id: i32,
    /// Host
    pub host: String,
    /// Port
    pub port: i32,
}

/// Topic metadata
#[derive(Debug, Clone)]
pub struct TopicMetadata {
    /// Error code (0 = no error)
    pub error_code: i16,
    /// Topic name
    pub name: String,
    /// Number of partitions
    pub partition_count: u32,
}

/// Check if the given bytes look like Kafka protocol
///
/// Kafka requests start with:
/// - length (4 bytes, big-endian)
/// - api_key (2 bytes, big-endian, 0-68 are valid)
/// - api_version (2 bytes, big-endian, typically 0-15)
/// - correlation_id (4 bytes)
pub fn is_kafka_protocol(data: &[u8]) -> bool {
    if data.len() < MIN_REQUEST_SIZE {
        return false;
    }

    // Read message length
    let length = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);

    // Sanity check on length
    if length < 8 || length > MAX_MESSAGE_SIZE {
        return false;
    }

    // Read API key
    let api_key = i16::from_be_bytes([data[4], data[5]]);

    // Check if API key is in valid range (0-68 as of Kafka 3.x)
    if !(0..=68).contains(&api_key) {
        return false;
    }

    // Read API version
    let api_version = i16::from_be_bytes([data[6], data[7]]);

    // API versions are typically small (0-15 for most APIs)
    if !(0..=20).contains(&api_version) {
        return false;
    }

    // Additional heuristic: correlation ID should be reasonable
    let correlation_id = i32::from_be_bytes([data[8], data[9], data[10], data[11]]);

    // Correlation IDs are typically positive and not huge
    if correlation_id < 0 || correlation_id > 10_000_000 {
        return false;
    }

    true
}

/// Parse a Kafka request message
///
/// Returns the parsed message and the number of bytes consumed
pub fn parse_request(data: &[u8]) -> Result<(KafkaRequest, usize), ParseError> {
    if data.len() < MIN_REQUEST_SIZE {
        return Err(ParseError::Incomplete {
            needed: MIN_REQUEST_SIZE - data.len(),
        });
    }

    let length = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
    let total_length = (length + 4) as usize; // length field itself not included in length

    if length > MAX_MESSAGE_SIZE {
        return Err(ParseError::InvalidLength(length));
    }

    if data.len() < total_length {
        return Err(ParseError::Incomplete {
            needed: total_length - data.len(),
        });
    }

    let api_key_raw = i16::from_be_bytes([data[4], data[5]]);
    let api_key = ApiKey::from_i16(api_key_raw);
    let api_version = i16::from_be_bytes([data[6], data[7]]);
    let correlation_id = i32::from_be_bytes([data[8], data[9], data[10], data[11]]);

    // Parse client ID (nullable string)
    let mut offset = 12;
    let client_id = if data.len() > offset + 2 {
        let client_id_len = i16::from_be_bytes([data[offset], data[offset + 1]]);
        offset += 2;

        if client_id_len < 0 {
            None
        } else if data.len() >= offset + client_id_len as usize {
            let id = String::from_utf8(data[offset..offset + client_id_len as usize].to_vec()).ok();
            offset += client_id_len as usize;
            id
        } else {
            None
        }
    } else {
        None
    };

    // Parse request-specific details
    let remaining = &data[offset..total_length];
    let details = parse_request_details(api_key, api_version, remaining);

    Ok((
        KafkaRequest {
            length,
            api_key,
            api_version,
            correlation_id,
            client_id,
            details,
        },
        total_length,
    ))
}

/// Parse request-specific details based on API key
fn parse_request_details(api_key: ApiKey, api_version: i16, data: &[u8]) -> RequestDetails {
    match api_key {
        ApiKey::Produce => parse_produce_request(data, api_version),
        ApiKey::Fetch => parse_fetch_request(data, api_version),
        ApiKey::Metadata => parse_metadata_request(data, api_version),
        ApiKey::JoinGroup => parse_join_group_request(data, api_version),
        ApiKey::Heartbeat => parse_heartbeat_request(data, api_version),
        ApiKey::LeaveGroup => parse_leave_group_request(data, api_version),
        ApiKey::SyncGroup => parse_sync_group_request(data, api_version),
        ApiKey::OffsetCommit => parse_offset_commit_request(data, api_version),
        ApiKey::OffsetFetch => parse_offset_fetch_request(data, api_version),
        ApiKey::FindCoordinator => parse_find_coordinator_request(data, api_version),
        ApiKey::ApiVersions => RequestDetails::ApiVersions,
        _ => RequestDetails::Other {
            raw_length: data.len() as u32,
        },
    }
}

/// Parse a Kafka string (2-byte length prefix, nullable)
fn parse_string(data: &[u8], offset: &mut usize) -> Option<String> {
    if data.len() < *offset + 2 {
        return None;
    }
    let len = i16::from_be_bytes([data[*offset], data[*offset + 1]]);
    *offset += 2;

    if len < 0 {
        return None;
    }

    let len = len as usize;
    if data.len() < *offset + len {
        return None;
    }

    let s = String::from_utf8(data[*offset..*offset + len].to_vec()).ok();
    *offset += len;
    s
}

/// Parse a compact string (varint length, for newer API versions)
#[allow(dead_code)]
fn parse_compact_string(data: &[u8], offset: &mut usize) -> Option<String> {
    // Compact strings use unsigned varint for length
    // For simplicity, we'll handle the common case of small strings
    if data.len() < *offset + 1 {
        return None;
    }

    let len_byte = data[*offset];
    *offset += 1;

    if len_byte == 0 {
        return Some(String::new());
    }

    let len = (len_byte - 1) as usize; // Length is encoded as actual_length + 1

    if data.len() < *offset + len {
        return None;
    }

    let s = String::from_utf8(data[*offset..*offset + len].to_vec()).ok();
    *offset += len;
    s
}

/// Parse produce request
fn parse_produce_request(data: &[u8], _api_version: i16) -> RequestDetails {
    // Skip transactional_id (nullable string) for v3+
    // Skip acks (i16)
    // Skip timeout (i32)

    // For simplicity, we'll try to extract topic names
    let mut topics = Vec::new();
    let mut offset;

    // Skip to topic array - this varies by version
    // Version 0-2: acks (2) + timeout (4) + topic_count (4)
    // Version 3+: transactional_id (variable) + acks (2) + timeout (4) + topic_count (4)

    if data.len() < 8 {
        return RequestDetails::Produce(ProduceRequest {
            acks: 0,
            timeout_ms: 0,
            topics,
            message_count: None,
        });
    }

    let acks = i16::from_be_bytes([data[0], data[1]]);
    let timeout_ms = i32::from_be_bytes([data[2], data[3], data[4], data[5]]);
    offset = 6;

    // Try to parse topic array
    if data.len() >= offset + 4 {
        let topic_count = i32::from_be_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]);
        offset += 4;

        for _ in 0..topic_count.min(100) {
            // Limit to prevent runaway parsing
            if let Some(topic) = parse_string(data, &mut offset) {
                topics.push(topic);
                // Skip partition data
                if data.len() >= offset + 4 {
                    let _partition_count = i32::from_be_bytes([
                        data[offset],
                        data[offset + 1],
                        data[offset + 2],
                        data[offset + 3],
                    ]) as usize;
                    // Skip partition entries (complex, just break out)
                    break;
                }
            } else {
                break;
            }
        }
    }

    RequestDetails::Produce(ProduceRequest {
        acks,
        timeout_ms,
        topics,
        message_count: None,
    })
}

/// Parse fetch request
fn parse_fetch_request(data: &[u8], _api_version: i16) -> RequestDetails {
    let mut topics = Vec::new();

    // replica_id (4) + max_wait_ms (4) + min_bytes (4)
    if data.len() < 12 {
        return RequestDetails::Fetch(FetchRequest {
            max_wait_ms: 0,
            min_bytes: 0,
            topics,
        });
    }

    // Skip replica_id (4 bytes)
    let max_wait_ms = i32::from_be_bytes([data[4], data[5], data[6], data[7]]);
    let min_bytes = i32::from_be_bytes([data[8], data[9], data[10], data[11]]);
    let mut offset = 12;

    // Parse topics array
    if data.len() >= offset + 4 {
        let topic_count = i32::from_be_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]);
        offset += 4;

        for _ in 0..topic_count.min(100) {
            if let Some(topic) = parse_string(data, &mut offset) {
                topics.push(topic);
                // Skip partition data
                if data.len() >= offset + 4 {
                    let partition_count = i32::from_be_bytes([
                        data[offset],
                        data[offset + 1],
                        data[offset + 2],
                        data[offset + 3],
                    ]) as usize;
                    offset += 4;
                    // Skip each partition entry (16 bytes each in v0)
                    offset += partition_count * 16;
                }
            } else {
                break;
            }
        }
    }

    RequestDetails::Fetch(FetchRequest {
        max_wait_ms,
        min_bytes,
        topics,
    })
}

/// Parse metadata request
fn parse_metadata_request(data: &[u8], _api_version: i16) -> RequestDetails {
    let mut topics = Vec::new();

    // Parse topics array
    if data.len() >= 4 {
        let topic_count = i32::from_be_bytes([data[0], data[1], data[2], data[3]]);
        let mut offset = 4;

        if topic_count >= 0 {
            for _ in 0..topic_count.min(100) {
                if let Some(topic) = parse_string(data, &mut offset) {
                    topics.push(topic);
                } else {
                    break;
                }
            }
        }
        // topic_count == -1 means all topics
    }

    RequestDetails::Metadata(MetadataRequest { topics })
}

/// Parse join group request
fn parse_join_group_request(data: &[u8], _api_version: i16) -> RequestDetails {
    let mut offset = 0;
    let group_id = parse_string(data, &mut offset).unwrap_or_default();

    let session_timeout_ms = if data.len() >= offset + 4 {
        let v = i32::from_be_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]);
        offset += 4;
        v
    } else {
        0
    };

    // Skip rebalance_timeout_ms for v1+
    if data.len() >= offset + 4 {
        offset += 4;
    }

    let member_id = parse_string(data, &mut offset).unwrap_or_default();
    let protocol_type = parse_string(data, &mut offset).unwrap_or_default();

    RequestDetails::JoinGroup(JoinGroupRequest {
        group_id,
        session_timeout_ms,
        member_id,
        protocol_type,
    })
}

/// Parse heartbeat request
fn parse_heartbeat_request(data: &[u8], _api_version: i16) -> RequestDetails {
    let mut offset = 0;

    let group_id = parse_string(data, &mut offset).unwrap_or_default();

    let generation_id = if data.len() >= offset + 4 {
        let v = i32::from_be_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]);
        offset += 4;
        v
    } else {
        0
    };

    let member_id = parse_string(data, &mut offset).unwrap_or_default();

    RequestDetails::Heartbeat(HeartbeatRequest {
        group_id,
        generation_id,
        member_id,
    })
}

/// Parse leave group request
fn parse_leave_group_request(data: &[u8], _api_version: i16) -> RequestDetails {
    let mut offset = 0;

    let group_id = parse_string(data, &mut offset).unwrap_or_default();
    let member_id = parse_string(data, &mut offset).unwrap_or_default();

    RequestDetails::LeaveGroup(LeaveGroupRequest {
        group_id,
        member_id,
    })
}

/// Parse sync group request
fn parse_sync_group_request(data: &[u8], _api_version: i16) -> RequestDetails {
    let mut offset = 0;

    let group_id = parse_string(data, &mut offset).unwrap_or_default();

    let generation_id = if data.len() >= offset + 4 {
        let v = i32::from_be_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]);
        offset += 4;
        v
    } else {
        0
    };

    let member_id = parse_string(data, &mut offset).unwrap_or_default();

    RequestDetails::SyncGroup(SyncGroupRequest {
        group_id,
        generation_id,
        member_id,
    })
}

/// Parse offset commit request
fn parse_offset_commit_request(data: &[u8], _api_version: i16) -> RequestDetails {
    let mut offset = 0;

    let group_id = parse_string(data, &mut offset).unwrap_or_default();

    // Skip generation_id (v1+) and member_id (v1+)
    // Parse topics
    let mut topics = Vec::new();

    // Find topic array - position varies by version
    // For simplicity, skip to what looks like the topic array
    while offset + 4 <= data.len() {
        let maybe_count = i32::from_be_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]);
        if maybe_count > 0 && maybe_count < 100 {
            offset += 4;
            for _ in 0..maybe_count {
                if let Some(topic) = parse_string(data, &mut offset) {
                    topics.push(topic);
                    // Skip partition data
                    break;
                } else {
                    break;
                }
            }
            break;
        }
        offset += 1;
    }

    RequestDetails::OffsetCommit(OffsetCommitRequest { group_id, topics })
}

/// Parse offset fetch request
fn parse_offset_fetch_request(data: &[u8], _api_version: i16) -> RequestDetails {
    let mut offset = 0;

    let group_id = parse_string(data, &mut offset).unwrap_or_default();

    let mut topics = Vec::new();

    // Parse topics array
    if data.len() >= offset + 4 {
        let topic_count = i32::from_be_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]);
        offset += 4;

        if topic_count >= 0 {
            for _ in 0..topic_count.min(100) {
                if let Some(topic) = parse_string(data, &mut offset) {
                    topics.push(topic);
                    // Skip partition array
                    if data.len() >= offset + 4 {
                        let partition_count = i32::from_be_bytes([
                            data[offset],
                            data[offset + 1],
                            data[offset + 2],
                            data[offset + 3],
                        ]) as usize;
                        offset += 4;
                        offset += partition_count * 4; // Each partition is 4 bytes
                    }
                } else {
                    break;
                }
            }
        }
    }

    RequestDetails::OffsetFetch(OffsetFetchRequest { group_id, topics })
}

/// Parse find coordinator request
fn parse_find_coordinator_request(data: &[u8], api_version: i16) -> RequestDetails {
    let mut offset = 0;

    let key = parse_string(data, &mut offset).unwrap_or_default();

    let key_type = if api_version >= 1 && data.len() > offset {
        data[offset] as i8
    } else {
        0 // GROUP
    };

    RequestDetails::FindCoordinator(FindCoordinatorRequest { key, key_type })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_api_key_from_i16() {
        assert_eq!(ApiKey::from_i16(0), ApiKey::Produce);
        assert_eq!(ApiKey::from_i16(1), ApiKey::Fetch);
        assert_eq!(ApiKey::from_i16(3), ApiKey::Metadata);
        assert_eq!(ApiKey::from_i16(18), ApiKey::ApiVersions);
        assert_eq!(ApiKey::from_i16(999), ApiKey::Unknown);
    }

    #[test]
    fn test_api_key_name() {
        assert_eq!(ApiKey::Produce.name(), "Produce");
        assert_eq!(ApiKey::Fetch.name(), "Fetch");
        assert_eq!(ApiKey::Metadata.name(), "Metadata");
    }

    #[test]
    fn test_api_key_categories() {
        assert!(ApiKey::Produce.is_produce());
        assert!(!ApiKey::Produce.is_consume());

        assert!(ApiKey::Fetch.is_consume());
        assert!(!ApiKey::Fetch.is_produce());

        assert!(ApiKey::JoinGroup.is_consumer_group());
        assert!(ApiKey::CreateTopics.is_admin());
        assert!(ApiKey::Metadata.is_metadata());
    }

    #[test]
    fn test_is_kafka_protocol() {
        // Valid Kafka request header: length=100, api_key=0 (Produce), version=0, correlation_id=1
        let valid = [
            0, 0, 0, 100, // length
            0, 0, // api_key (Produce)
            0, 0, // api_version
            0, 0, 0, 1, // correlation_id
        ];
        assert!(is_kafka_protocol(&valid));

        // Invalid - too short
        assert!(!is_kafka_protocol(&[0, 0, 0, 100]));

        // Invalid - bad api_key (999)
        let bad_api = [0, 0, 0, 100, 3, 231, 0, 0, 0, 0, 0, 1];
        assert!(!is_kafka_protocol(&bad_api));
    }

    #[test]
    fn test_parse_string() {
        // "test" with length prefix
        let data = [0, 4, b't', b'e', b's', b't'];
        let mut offset = 0;
        assert_eq!(parse_string(&data, &mut offset), Some("test".to_string()));
        assert_eq!(offset, 6);

        // Null string
        let null_data = [255, 255]; // -1 in i16
        let mut offset = 0;
        assert_eq!(parse_string(&null_data, &mut offset), None);
    }

    #[test]
    fn test_parse_request() {
        // ApiVersions request: length=23, api_key=18, version=3, correlation_id=0, client_id="test"
        let request = [
            0, 0, 0, 15, // length (15 bytes after this)
            0, 18, // api_key (ApiVersions)
            0, 3, // api_version
            0, 0, 0, 0, // correlation_id
            0, 4, b't', b'e', b's', b't', // client_id "test"
            0,    // tags (empty for flexible version)
        ];

        let (parsed, consumed) = parse_request(&request).unwrap();
        assert_eq!(parsed.api_key, ApiKey::ApiVersions);
        assert_eq!(parsed.api_version, 3);
        assert_eq!(parsed.correlation_id, 0);
        assert_eq!(parsed.client_id, Some("test".to_string()));
        assert_eq!(consumed, 19);
    }
}
