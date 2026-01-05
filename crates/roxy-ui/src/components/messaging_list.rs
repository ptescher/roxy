//! Messaging list component for Roxy UI
//!
//! This component renders the scrollable list of messaging operations
//! (Kafka, RabbitMQ, etc.) with a header and individual message rows.

use gpui::prelude::*;
use gpui::*;
use roxy_core::KafkaMessageRow;
use std::sync::Arc;

use crate::theme::{colors, dimensions, font_size, spacing, Theme};
use gpui::MouseButton;

/// Callback type for message selection
pub type OnMessageSelect = Arc<dyn Fn(&KafkaMessageRow, &mut App) + Send + Sync + 'static>;

/// Properties for the MessagingList component
#[derive(Clone)]
pub struct MessagingListProps {
    /// List of messages to display
    pub messages: Vec<KafkaMessageRow>,
    /// Currently selected message ID (if any)
    pub selected_message_id: Option<String>,
    /// Callback when a message row is clicked
    pub on_message_select: Option<OnMessageSelect>,
    /// Scroll handle for the message list
    pub scroll_handle: ScrollHandle,
}

impl Default for MessagingListProps {
    fn default() -> Self {
        Self {
            messages: Vec::new(),
            selected_message_id: None,
            on_message_select: None,
            scroll_handle: ScrollHandle::new(),
        }
    }
}

/// Messaging list component
pub struct MessagingList {
    props: MessagingListProps,
    theme: Theme,
}

impl MessagingList {
    /// Create a new messaging list with the given props
    pub fn new(props: MessagingListProps) -> Self {
        Self {
            props,
            theme: Theme::dark(),
        }
    }

    /// Render the messaging list
    pub fn render(&self) -> impl IntoElement {
        let messages = &self.props.messages;

        div()
            .flex()
            .flex_col()
            .size_full()
            .overflow_hidden()
            // Header
            .child(div().flex_shrink_0().child(self.render_header()))
            // Rows
            .child(div().flex_1().overflow_hidden().child(self.render_rows()))
            .when(messages.is_empty(), |this| {
                this.child(self.render_empty_state())
            })
    }

    /// Render the list header with column labels
    fn render_header(&self) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .h(dimensions::REQUEST_HEADER_HEIGHT)
            .px(spacing::MD)
            .bg(rgb(colors::MANTLE))
            .border_b_1()
            .border_color(rgb(colors::SURFACE_0))
            .text_size(font_size::SM)
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(self.theme.text_muted)
            // System column
            .child(div().w(px(60.0)).child("System"))
            // Status column
            .child(div().w(px(60.0)).child("Status"))
            // Operation column
            .child(div().w(px(80.0)).child("Operation"))
            // Topic/Destination column - flexible
            .child(div().flex_1().min_w(px(150.0)).child("Topic"))
            // Consumer Group column
            .child(div().w(px(120.0)).child("Group"))
            // Messages column
            .child(div().w(px(70.0)).child("Count"))
            // Size column
            .child(div().w(px(70.0)).child("Size"))
            // Duration column
            .child(div().w(px(80.0)).child("Duration"))
    }

    /// Render the scrollable list of rows
    fn render_rows(&self) -> impl IntoElement {
        let messages = self.props.messages.clone();
        let selected_id = self.props.selected_message_id.clone();
        let on_select = self.props.on_message_select.clone();
        let scroll_handle = self.props.scroll_handle.clone();

        div()
            .size_full()
            .bg(rgb(colors::BASE))
            .id("messaging-list-rows")
            .overflow_y_scroll()
            .track_scroll(&scroll_handle)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .w_full()
                    .children(messages.iter().enumerate().map(|(index, msg)| {
                        let is_selected = selected_id
                            .as_ref()
                            .map(|id| id == &msg.id)
                            .unwrap_or(false);
                        self.render_message_row(msg, index, is_selected, on_select.clone())
                    })),
            )
    }

    /// Render a single message row
    fn render_message_row(
        &self,
        msg: &KafkaMessageRow,
        index: usize,
        selected: bool,
        on_select: Option<OnMessageSelect>,
    ) -> impl IntoElement {
        let bg = if selected {
            rgb(colors::SURFACE_0)
        } else if index % 2 == 0 {
            rgb(colors::BASE)
        } else {
            rgb(colors::MANTLE)
        };

        let system_color = get_system_color(&msg.messaging_system);
        let status_color = if msg.success == 1 {
            hsla(142.0 / 360.0, 0.7, 0.45, 1.0) // Green
        } else {
            hsla(0.0 / 360.0, 0.7, 0.5, 1.0) // Red
        };

        // Clone message data for the callback
        let msg_for_callback = msg.clone();

        // Truncate the destination for display
        let destination_display = if msg.messaging_destination.len() > 40 {
            format!("{}...", &msg.messaging_destination[..37])
        } else {
            msg.messaging_destination.clone()
        };

        // Truncate the consumer group for display
        let group_display = if msg.messaging_consumer_group.len() > 15 {
            format!("{}...", &msg.messaging_consumer_group[..12])
        } else if msg.messaging_consumer_group.is_empty() {
            "-".to_string()
        } else {
            msg.messaging_consumer_group.clone()
        };

        let mut el = div()
            .id(ElementId::Name(format!("msg-{}", msg.id).into()))
            .flex()
            .items_center()
            .h(dimensions::REQUEST_ROW_HEIGHT)
            .px(spacing::MD)
            .bg(bg)
            .hover(|style| style.bg(rgb(colors::SURFACE_0)))
            .cursor_pointer()
            .border_b_1()
            .border_color(rgb(colors::SURFACE_0))
            // System column
            .child(
                div()
                    .w(px(60.0))
                    .flex_shrink_0()
                    .child(render_system_badge(&msg.messaging_system, system_color)),
            )
            // Status column
            .child(
                div()
                    .w(px(60.0))
                    .flex_shrink_0()
                    .child(render_status_badge(msg.success == 1, status_color)),
            )
            // Operation column
            .child(
                div()
                    .w(px(80.0))
                    .flex_shrink_0()
                    .child(render_operation_badge(&msg.operation_type)),
            )
            // Topic/Destination column
            .child(
                div()
                    .flex_1()
                    .min_w(px(150.0))
                    .overflow_hidden()
                    .text_size(font_size::SM)
                    .text_color(self.theme.text_primary)
                    .text_ellipsis()
                    .child(destination_display),
            )
            // Consumer Group column
            .child(
                div()
                    .w(px(120.0))
                    .flex_shrink_0()
                    .text_size(font_size::SM)
                    .text_color(self.theme.text_secondary)
                    .text_ellipsis()
                    .child(group_display),
            )
            // Message count column
            .child(
                div()
                    .w(px(70.0))
                    .flex_shrink_0()
                    .text_size(font_size::SM)
                    .text_color(self.theme.text_secondary)
                    .child(msg.message_count.to_string()),
            )
            // Payload size column
            .child(
                div()
                    .w(px(70.0))
                    .flex_shrink_0()
                    .text_size(font_size::SM)
                    .text_color(self.theme.text_secondary)
                    .child(format_bytes(msg.payload_size as u64)),
            )
            // Duration column
            .child(
                div()
                    .w(px(80.0))
                    .flex_shrink_0()
                    .text_size(font_size::SM)
                    .text_color(self.theme.text_secondary)
                    .child(format_duration(msg.duration_ms)),
            );

        if let Some(callback) = on_select {
            el = el.on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                callback(&msg_for_callback, cx);
            });
        }

        el
    }

    /// Render empty state when there are no messages
    fn render_empty_state(&self) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .py(spacing::XXL)
            .gap(spacing::MD)
            .child(
                div()
                    .text_size(font_size::LG)
                    .text_color(self.theme.text_muted)
                    .child("No messaging activity yet"),
            )
            .child(
                div()
                    .text_size(font_size::SM)
                    .text_color(self.theme.text_muted)
                    .child("Kafka and other messaging operations will appear here"),
            )
    }
}

/// Render messaging system badge
fn render_system_badge(system: &str, color: Hsla) -> impl IntoElement {
    let label = match system.to_lowercase().as_str() {
        "kafka" => "Kafka",
        "rabbitmq" => "RMQ",
        "redis" => "Redis",
        "nats" => "NATS",
        _ => system,
    };

    div()
        .px(spacing::SM)
        .py(px(2.0))
        .bg(color.opacity(0.2))
        .rounded(px(4.0))
        .text_size(font_size::XS)
        .text_color(color)
        .font_weight(FontWeight::MEDIUM)
        .child(label.to_string())
}

/// Render status badge
fn render_status_badge(success: bool, color: Hsla) -> impl IntoElement {
    let label = if success { "OK" } else { "ERR" };

    div()
        .px(spacing::SM)
        .py(px(2.0))
        .bg(color.opacity(0.2))
        .rounded(px(4.0))
        .text_size(font_size::XS)
        .text_color(color)
        .font_weight(FontWeight::MEDIUM)
        .child(label.to_string())
}

/// Render operation badge
fn render_operation_badge(operation: &str) -> impl IntoElement {
    let color = get_operation_color(operation);
    let label = match operation.to_lowercase().as_str() {
        "publish" => "PUB",
        "receive" => "SUB",
        "process" => "PROC",
        _ => operation,
    };

    div()
        .px(spacing::SM)
        .py(px(2.0))
        .bg(color.opacity(0.2))
        .rounded(px(4.0))
        .text_size(font_size::XS)
        .text_color(color)
        .font_weight(FontWeight::MEDIUM)
        .child(label.to_string())
}

/// Get color for messaging system type
fn get_system_color(system: &str) -> Hsla {
    match system.to_lowercase().as_str() {
        "kafka" => hsla(30.0 / 360.0, 0.8, 0.5, 1.0), // Orange
        "rabbitmq" => hsla(30.0 / 360.0, 0.9, 0.55, 1.0), // Orange (RabbitMQ brand)
        "redis" => hsla(0.0 / 360.0, 0.7, 0.5, 1.0),  // Red
        "nats" => hsla(200.0 / 360.0, 0.7, 0.5, 1.0), // Blue
        _ => hsla(0.0, 0.0, 0.5, 1.0),                // Gray
    }
}

/// Get color for operation type
fn get_operation_color(operation: &str) -> Hsla {
    match operation.to_lowercase().as_str() {
        "publish" => hsla(142.0 / 360.0, 0.7, 0.45, 1.0), // Green - produce
        "receive" => hsla(210.0 / 360.0, 0.7, 0.6, 1.0),  // Blue - consume
        "process" => hsla(270.0 / 360.0, 0.5, 0.6, 1.0),  // Purple - process
        _ => hsla(0.0, 0.0, 0.5, 1.0),                    // Gray
    }
}

/// Format duration in ms to human readable
fn format_duration(ms: f64) -> String {
    if ms < 1.0 {
        format!("{:.0}µs", ms * 1000.0)
    } else if ms < 1000.0 {
        format!("{:.1}ms", ms)
    } else if ms < 60000.0 {
        format!("{:.2}s", ms / 1000.0)
    } else {
        format!("{:.1}m", ms / 60000.0)
    }
}

/// Format bytes to human readable
fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{}B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1}K", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1}M", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1}G", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

/// Standalone function to render messaging list
pub fn messaging_list(props: MessagingListProps) -> impl IntoElement {
    MessagingList::new(props).render()
}
