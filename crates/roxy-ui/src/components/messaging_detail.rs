//! Messaging detail panel component for Roxy UI
//!
//! This component displays detailed information about a selected messaging operation,
//! including Kafka-specific fields like topic, consumer group, and operation details.

use gpui::prelude::*;
use gpui::*;
use roxy_core::KafkaMessageRow;
use std::sync::Arc;

use crate::theme::{colors, dimensions, font_size, spacing, Theme};

/// Tabs available in the messaging detail panel
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MessagingDetailTab {
    /// Overview of the message
    #[default]
    Overview,
    /// Message payload/content
    Payload,
    /// Message metadata and attributes
    Metadata,
    /// Error details (if any)
    Error,
}

impl MessagingDetailTab {
    pub fn label(&self) -> &'static str {
        match self {
            MessagingDetailTab::Overview => "Overview",
            MessagingDetailTab::Payload => "Payload",
            MessagingDetailTab::Metadata => "Metadata",
            MessagingDetailTab::Error => "Error",
        }
    }

    pub fn all() -> &'static [MessagingDetailTab] {
        &[
            MessagingDetailTab::Overview,
            MessagingDetailTab::Payload,
            MessagingDetailTab::Metadata,
            MessagingDetailTab::Error,
        ]
    }
}

/// Callback type for tab selection
pub type OnTabSelect = Arc<dyn Fn(MessagingDetailTab, &mut App) + Send + Sync + 'static>;

/// Properties for the MessagingDetailPanel component
#[derive(Clone)]
pub struct MessagingDetailPanelProps {
    /// The selected message to display details for
    pub selected_message: Option<KafkaMessageRow>,
    /// Currently active tab
    pub active_tab: MessagingDetailTab,
    /// Callback when a tab is selected
    pub on_tab_select: Option<OnTabSelect>,
    /// Panel height
    pub height: f32,
    /// Scroll handle for the content
    pub scroll_handle: ScrollHandle,
}

impl Default for MessagingDetailPanelProps {
    fn default() -> Self {
        Self {
            selected_message: None,
            active_tab: MessagingDetailTab::default(),
            on_tab_select: None,
            height: 300.0,
            scroll_handle: ScrollHandle::new(),
        }
    }
}

/// Messaging detail panel component
pub struct MessagingDetailPanel {
    props: MessagingDetailPanelProps,
    theme: Theme,
}

impl MessagingDetailPanel {
    /// Create a new messaging detail panel
    pub fn new(props: MessagingDetailPanelProps) -> Self {
        Self {
            props,
            theme: Theme::dark(),
        }
    }

    /// Render the detail panel
    pub fn render(&self) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(colors::MANTLE))
            .border_t_1()
            .border_color(rgb(colors::SURFACE_0))
            .child(self.render_tabs())
            .child(self.render_content())
    }

    /// Render the tab bar
    fn render_tabs(&self) -> impl IntoElement {
        let active_tab = self.props.active_tab;
        let on_tab_select = self.props.on_tab_select.clone();

        div()
            .flex()
            .items_center()
            .h(dimensions::DETAIL_PANEL_TABS_HEIGHT)
            .px(spacing::MD)
            .bg(rgb(colors::MANTLE))
            .border_b_1()
            .border_color(rgb(colors::SURFACE_0))
            .gap(spacing::XS)
            .children(MessagingDetailTab::all().iter().map(|tab| {
                let is_active = *tab == active_tab;
                let tab_value = *tab;
                let on_select = on_tab_select.clone();

                let bg = if is_active {
                    rgb(colors::SURFACE_0)
                } else {
                    rgba(colors::TRANSPARENT)
                };
                let text_color = if is_active {
                    rgb(colors::TEXT)
                } else {
                    rgb(colors::SUBTEXT_0)
                };

                let mut el = div()
                    .px(spacing::SM)
                    .py(spacing::XS)
                    .rounded(dimensions::BORDER_RADIUS)
                    .bg(bg)
                    .text_size(font_size::SM)
                    .font_weight(if is_active {
                        FontWeight::MEDIUM
                    } else {
                        FontWeight::NORMAL
                    })
                    .text_color(text_color)
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(colors::SURFACE_0)))
                    .child(tab.label().to_string());

                if let Some(callback) = on_select {
                    el = el.on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                        callback(tab_value, cx);
                    });
                }

                el
            }))
    }

    /// Render the content area based on active tab
    fn render_content(&self) -> impl IntoElement {
        let scroll_handle = self.props.scroll_handle.clone();

        div()
            .flex_1()
            .overflow_hidden()
            .id("messaging-detail-content")
            .overflow_y_scroll()
            .track_scroll(&scroll_handle)
            .child(self.render_message_content())
    }

    /// Render content for the selected message
    fn render_message_content(&self) -> impl IntoElement {
        match &self.props.selected_message {
            Some(msg) => self.render_message_details(msg).into_any_element(),
            None => self.render_empty_state().into_any_element(),
        }
    }

    /// Render the details for a message
    fn render_message_details(&self, msg: &KafkaMessageRow) -> impl IntoElement {
        match self.props.active_tab {
            MessagingDetailTab::Overview => self.render_overview(msg).into_any_element(),
            MessagingDetailTab::Payload => self.render_payload_tab(msg).into_any_element(),
            MessagingDetailTab::Metadata => self.render_metadata_tab(msg).into_any_element(),
            MessagingDetailTab::Error => self.render_error_tab(msg).into_any_element(),
        }
    }

    /// Render the overview tab
    fn render_overview(&self, msg: &KafkaMessageRow) -> impl IntoElement {
        let status_color = if msg.success == 1 {
            hsla(142.0 / 360.0, 0.7, 0.45, 1.0) // Green
        } else {
            hsla(0.0 / 360.0, 0.7, 0.5, 1.0) // Red
        };

        div()
            .flex()
            .flex_col()
            .p(spacing::MD)
            .gap(spacing::MD)
            // Connection Info section
            .child(
                self.render_section(
                    "Connection",
                    div()
                        .flex()
                        .flex_col()
                        .gap(spacing::SM)
                        .child(self.render_field("System", format_system(&msg.messaging_system)))
                        .child(self.render_field(
                            "Server",
                            format!("{}:{}", msg.server_address, msg.server_port),
                        ))
                        .child(self.render_field("Client", msg.client_address.clone()))
                        .child(self.render_field(
                            "Client ID",
                            if msg.messaging_client_id.is_empty() {
                                "-".to_string()
                            } else {
                                msg.messaging_client_id.clone()
                            },
                        )),
                ),
            )
            // Operation Info section
            .child(
                self.render_section(
                    "Operation",
                    div()
                        .flex()
                        .flex_col()
                        .gap(spacing::SM)
                        .child(
                            self.render_field("Type", format_operation_type(&msg.operation_type)),
                        )
                        .child(self.render_field("Operation", msg.messaging_operation.clone()))
                        .child(self.render_field_with_color(
                            "Status",
                            if msg.success == 1 {
                                "Success"
                            } else {
                                "Failed"
                            },
                            status_color,
                        ))
                        .child(self.render_field("Duration", format_duration(msg.duration_ms))),
                ),
            )
            // Topic/Destination section
            .child(
                self.render_section(
                    "Destination",
                    div()
                        .flex()
                        .flex_col()
                        .gap(spacing::SM)
                        .child(self.render_field("Topic", msg.messaging_destination.clone()))
                        .child(self.render_field(
                            "Consumer Group",
                            if msg.messaging_consumer_group.is_empty() {
                                "-".to_string()
                            } else {
                                msg.messaging_consumer_group.clone()
                            },
                        ))
                        .child(self.render_field("Message Count", msg.message_count.to_string()))
                        .child(
                            self.render_field(
                                "Payload Size",
                                format_bytes(msg.payload_size as u64),
                            ),
                        ),
                ),
            )
            // Tracing section
            .child(
                self.render_section(
                    "Tracing",
                    div()
                        .flex()
                        .flex_col()
                        .gap(spacing::SM)
                        .child(self.render_field("Trace ID", msg.trace_id.clone()))
                        .child(self.render_field("Span ID", msg.span_id.clone()))
                        .child(self.render_field(
                            "Parent Span",
                            if msg.parent_span_id.is_empty() {
                                "-".to_string()
                            } else {
                                msg.parent_span_id.clone()
                            },
                        )),
                ),
            )
    }

    /// Render the payload tab (message content)
    fn render_payload_tab(&self, msg: &KafkaMessageRow) -> impl IntoElement {
        // Try to parse and display the attributes as the payload
        // In Kafka, the actual message content is typically stored in attributes
        let payload_content = if !msg.attributes.is_empty() && msg.attributes != "{}" {
            format_json(&msg.attributes)
        } else {
            String::new()
        };

        div()
            .flex()
            .flex_col()
            .p(spacing::MD)
            .gap(spacing::MD)
            // Message Info section
            .child(
                self.render_section(
                    "Message Info",
                    div()
                        .flex()
                        .flex_col()
                        .gap(spacing::SM)
                        .child(self.render_field("Topic", msg.messaging_destination.clone()))
                        .child(self.render_field("Operation", msg.messaging_operation.clone()))
                        .child(self.render_field("Message Count", msg.message_count.to_string()))
                        .child(
                            self.render_field(
                                "Payload Size",
                                format_bytes(msg.payload_size as u64),
                            ),
                        ),
                ),
            )
            // Payload Content section
            .child(self.render_section(
                "Payload Content",
                if payload_content.is_empty() {
                    div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .py(spacing::LG)
                        .child(
                            div()
                                .text_size(font_size::SM)
                                .text_color(self.theme.text_muted)
                                .child("No payload data captured"),
                        )
                        .into_any_element()
                } else {
                    div()
                        .p(spacing::MD)
                        .bg(rgb(colors::BASE))
                        .rounded(dimensions::BORDER_RADIUS)
                        .border_1()
                        .border_color(rgb(colors::SURFACE_0))
                        .overflow_hidden()
                        .child(
                            div()
                                .text_size(font_size::SM)
                                .font_family("monospace")
                                .text_color(self.theme.text_primary)
                                .child(payload_content),
                        )
                        .into_any_element()
                },
            ))
    }

    /// Render the metadata tab (Kafka-specific details)
    fn render_metadata_tab(&self, msg: &KafkaMessageRow) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .p(spacing::MD)
            .gap(spacing::MD)
            // Kafka Protocol section
            .child(
                self.render_section(
                    "Kafka Protocol",
                    div()
                        .flex()
                        .flex_col()
                        .gap(spacing::SM)
                        .child(self.render_field(
                            "API Key",
                            format!(
                                "{} ({})",
                                msg.kafka_api_key,
                                api_key_name(msg.kafka_api_key)
                            ),
                        ))
                        .child(self.render_field("API Version", msg.kafka_api_version.to_string()))
                        .child(
                            self.render_field(
                                "Correlation ID",
                                msg.kafka_correlation_id.to_string(),
                            ),
                        ),
                ),
            )
            // Message Stats section
            .child(
                self.render_section(
                    "Message Statistics",
                    div()
                        .flex()
                        .flex_col()
                        .gap(spacing::SM)
                        .child(self.render_field("Message Count", msg.message_count.to_string()))
                        .child(
                            self.render_field(
                                "Payload Size",
                                format_bytes(msg.payload_size as u64),
                            ),
                        )
                        .child(self.render_field("Duration", format_duration(msg.duration_ms))),
                ),
            )
            // Attributes section (if any)
            .when(
                !msg.attributes.is_empty() && msg.attributes != "{}",
                |this| {
                    this.child(
                        self.render_section(
                            "Attributes",
                            div()
                                .p(spacing::MD)
                                .bg(rgb(colors::BASE))
                                .rounded(dimensions::BORDER_RADIUS)
                                .border_1()
                                .border_color(rgb(colors::SURFACE_0))
                                .child(
                                    div()
                                        .text_size(font_size::SM)
                                        .font_family("monospace")
                                        .text_color(self.theme.text_primary)
                                        .child(format_json(&msg.attributes)),
                                ),
                        ),
                    )
                },
            )
    }

    /// Render the error tab
    fn render_error_tab(&self, msg: &KafkaMessageRow) -> impl IntoElement {
        if msg.success == 1 {
            return div()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .py(spacing::XXL)
                .child(
                    div()
                        .text_size(font_size::MD)
                        .text_color(self.theme.text_muted)
                        .child("No errors - operation completed successfully"),
                )
                .into_any_element();
        }

        div()
            .flex()
            .flex_col()
            .p(spacing::MD)
            .gap(spacing::MD)
            .child(
                self.render_section(
                    "Error Details",
                    div()
                        .flex()
                        .flex_col()
                        .gap(spacing::SM)
                        .child(self.render_field_with_color(
                            "Status",
                            "Failed",
                            hsla(0.0 / 360.0, 0.7, 0.5, 1.0),
                        ))
                        .child(self.render_field(
                            "Error Code",
                            if msg.error_code == 0 {
                                "-".to_string()
                            } else {
                                format!("{} ({})", msg.error_code, kafka_error_name(msg.error_code))
                            },
                        ))
                        .child(
                            div()
                                .mt(spacing::SM)
                                .p(spacing::MD)
                                .bg(hsla(0.0 / 360.0, 0.3, 0.15, 1.0))
                                .rounded(dimensions::BORDER_RADIUS)
                                .border_1()
                                .border_color(hsla(0.0 / 360.0, 0.5, 0.3, 1.0))
                                .child(
                                    div()
                                        .text_size(font_size::SM)
                                        .text_color(hsla(0.0 / 360.0, 0.7, 0.7, 1.0))
                                        .child(if msg.error_message.is_empty() {
                                            "No error message available".to_string()
                                        } else {
                                            msg.error_message.clone()
                                        }),
                                ),
                        ),
                ),
            )
            .into_any_element()
    }

    /// Render a section with title
    fn render_section(&self, title: &str, content: impl IntoElement) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap(spacing::SM)
            .child(
                div()
                    .text_size(font_size::SM)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(self.theme.text_muted)
                    .child(title.to_string()),
            )
            .child(
                div()
                    .pl(spacing::SM)
                    .border_l_2()
                    .border_color(rgb(colors::SURFACE_1))
                    .child(content),
            )
    }

    /// Render a field with label and value
    fn render_field(&self, label: &str, value: impl Into<SharedString>) -> impl IntoElement {
        div()
            .flex()
            .items_baseline()
            .gap(spacing::SM)
            .child(
                div()
                    .w(px(120.0))
                    .flex_shrink_0()
                    .text_size(font_size::SM)
                    .text_color(self.theme.text_muted)
                    .child(format!("{}:", label)),
            )
            .child(
                div()
                    .flex_1()
                    .text_size(font_size::SM)
                    .text_color(self.theme.text_primary)
                    .child(value.into()),
            )
    }

    /// Render a field with colored value
    fn render_field_with_color(
        &self,
        label: &str,
        value: impl Into<SharedString>,
        color: Hsla,
    ) -> impl IntoElement {
        div()
            .flex()
            .items_baseline()
            .gap(spacing::SM)
            .child(
                div()
                    .w(px(120.0))
                    .flex_shrink_0()
                    .text_size(font_size::SM)
                    .text_color(self.theme.text_muted)
                    .child(format!("{}:", label)),
            )
            .child(
                div()
                    .flex_1()
                    .text_size(font_size::SM)
                    .text_color(color)
                    .font_weight(FontWeight::MEDIUM)
                    .child(value.into()),
            )
    }

    /// Render empty state when no message is selected
    fn render_empty_state(&self) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .size_full()
            .py(spacing::XXL)
            .child(
                div()
                    .text_size(font_size::MD)
                    .text_color(self.theme.text_muted)
                    .child("Select a message to view details"),
            )
    }
}

/// Format messaging system name
fn format_system(system: &str) -> String {
    match system.to_lowercase().as_str() {
        "kafka" => "Apache Kafka".to_string(),
        "rabbitmq" => "RabbitMQ".to_string(),
        "redis" => "Redis".to_string(),
        "nats" => "NATS".to_string(),
        _ => system.to_string(),
    }
}

/// Format operation type
fn format_operation_type(op: &str) -> String {
    match op.to_lowercase().as_str() {
        "publish" => "Publish (Producer)".to_string(),
        "receive" => "Receive (Consumer)".to_string(),
        "process" => "Process".to_string(),
        _ => op.to_string(),
    }
}

/// Get human-readable Kafka API key name
fn api_key_name(api_key: i16) -> &'static str {
    match api_key {
        0 => "Produce",
        1 => "Fetch",
        2 => "ListOffsets",
        3 => "Metadata",
        8 => "OffsetCommit",
        9 => "OffsetFetch",
        10 => "FindCoordinator",
        11 => "JoinGroup",
        12 => "Heartbeat",
        13 => "LeaveGroup",
        14 => "SyncGroup",
        15 => "DescribeGroups",
        16 => "ListGroups",
        18 => "ApiVersions",
        19 => "CreateTopics",
        20 => "DeleteTopics",
        _ => "Unknown",
    }
}

/// Get human-readable Kafka error name
fn kafka_error_name(error_code: i16) -> &'static str {
    match error_code {
        -1 => "UNKNOWN_SERVER_ERROR",
        0 => "NONE",
        1 => "OFFSET_OUT_OF_RANGE",
        2 => "CORRUPT_MESSAGE",
        3 => "UNKNOWN_TOPIC_OR_PARTITION",
        4 => "INVALID_FETCH_SIZE",
        5 => "LEADER_NOT_AVAILABLE",
        6 => "NOT_LEADER_OR_FOLLOWER",
        7 => "REQUEST_TIMED_OUT",
        8 => "BROKER_NOT_AVAILABLE",
        9 => "REPLICA_NOT_AVAILABLE",
        10 => "MESSAGE_TOO_LARGE",
        25 => "UNKNOWN_TOPIC_ID",
        _ => "UNKNOWN",
    }
}

/// Format JSON for display (pretty print if valid)
fn format_json(json: &str) -> String {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(json) {
        serde_json::to_string_pretty(&value).unwrap_or_else(|_| json.to_string())
    } else {
        json.to_string()
    }
}

/// Format duration in ms to human readable
fn format_duration(ms: f64) -> String {
    if ms < 1.0 {
        format!("{:.0}µs", ms * 1000.0)
    } else if ms < 1000.0 {
        format!("{:.2}ms", ms)
    } else if ms < 60000.0 {
        format!("{:.3}s", ms / 1000.0)
    } else {
        format!("{:.1}m", ms / 60000.0)
    }
}

/// Format bytes to human readable
fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

/// Standalone function to render messaging detail panel
pub fn messaging_detail_panel(props: MessagingDetailPanelProps) -> impl IntoElement {
    MessagingDetailPanel::new(props).render()
}
