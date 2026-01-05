//! Roxy Core - Shared types and models for the Roxy network debugger
//!
//! This crate provides common types used across the proxy and UI components,
//! including:
//! - HTTP request/response models
//! - ClickHouse client and schema definitions
//! - Service management for backend subprocesses
//! - Auto-update system
//! - Proxy subprocess manager

pub mod clickhouse;
pub mod models;
pub mod proxy_manager;
pub mod routing;
pub mod services;
pub mod updater;

// Re-export specific items to avoid ambiguous glob exports
pub use models::{
    AppSummary, DatabaseQueryRecord, HttpHeader, HttpMethod, HttpRequest, HttpResponse,
    HttpTransaction, KafkaMessageRecord, KafkaOperation, ProtocolType, TransactionStatus,
};

pub use clickhouse::{
    ClickHouseConfig, ConnectionStatus, DatabaseQueryRow, HostSummary, HttpRequestRecord,
    KafkaMessageRow, RoxyClickHouse, SpanRecord, TcpConnectionRow,
};

pub use services::{ManagedService, ServiceManager};

pub use proxy_manager::{LogLevel, ProxyLogEntry, ProxyManager, ProxyManagerConfig, ProxyStatus};

pub use updater::{
    Component, UpdateChannel, UpdateInfo, UpdateState, Updater, UpdaterConfig, Version,
    CURRENT_VERSION,
};

pub use routing::{
    HttpMethod as RoutingHttpMethod, KubernetesCluster, KubernetesService, LocalTarget,
    MatchConditions, MatchPattern, PortForward, PortForwardManager, PortForwardTarget, Project,
    RemoteTarget, RequestInfo, RequestModification, RoutingRule, RoutingRuleSet, RuleAction,
    ServicePort,
};
