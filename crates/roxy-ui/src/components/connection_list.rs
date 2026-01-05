//! Connections list component for Roxy UI
//!
//! This component renders the scrollable list of TCP connections
//! (PostgreSQL, Kafka, etc.) with a header and individual connection rows.

use gpui::prelude::*;
use gpui::*;
use roxy_core::TcpConnectionRow;
use std::sync::Arc;

use crate::theme::{colors, dimensions, font_size, spacing, Theme};
use gpui::MouseButton;

/// Callback type for connection selection
pub type OnConnectionSelect = Arc<dyn Fn(&TcpConnectionRow, &mut App) + Send + Sync + 'static>;

/// Properties for the ConnectionList component
#[derive(Clone)]
pub struct ConnectionListProps {
    /// List of connections to display
    pub connections: Vec<TcpConnectionRow>,
    /// Currently selected connection ID (if any)
    pub selected_connection_id: Option<String>,
    /// Callback when a connection row is clicked
    pub on_connection_select: Option<OnConnectionSelect>,
    /// Scroll handle for the connection list
    pub scroll_handle: ScrollHandle,
}

impl Default for ConnectionListProps {
    fn default() -> Self {
        Self {
            connections: Vec::new(),
            selected_connection_id: None,
            on_connection_select: None,
            scroll_handle: ScrollHandle::new(),
        }
    }
}

/// Connection list component
pub struct ConnectionList {
    props: ConnectionListProps,
    theme: Theme,
}

impl ConnectionList {
    /// Create a new connection list with the given props
    pub fn new(props: ConnectionListProps) -> Self {
        Self {
            props,
            theme: Theme::dark(),
        }
    }

    /// Render the connection list
    pub fn render(&self) -> impl IntoElement {
        let connections = &self.props.connections;

        div()
            .flex()
            .flex_col()
            .size_full()
            .overflow_hidden()
            // Header
            .child(div().flex_shrink_0().child(self.render_header()))
            // Rows
            .child(div().flex_1().overflow_hidden().child(self.render_rows()))
            .when(connections.is_empty(), |this| {
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
            // Protocol column
            .child(div().w(px(90.0)).child("Protocol"))
            // Status column
            .child(div().w(px(70.0)).child("Status"))
            // Target host column - flexible
            .child(div().flex_1().min_w(px(200.0)).child("Target"))
            // Duration column
            .child(div().w(px(80.0)).child("Duration"))
            // Bytes column
            .child(div().w(px(100.0)).child("Bytes"))
            // Messages column
            .child(div().w(px(80.0)).child("Messages"))
    }

    /// Render the scrollable list of rows
    fn render_rows(&self) -> impl IntoElement {
        let connections = self.props.connections.clone();
        let selected_id = self.props.selected_connection_id.clone();
        let on_select = self.props.on_connection_select.clone();
        let scroll_handle = self.props.scroll_handle.clone();

        div()
            .size_full()
            .bg(rgb(colors::BASE))
            .id("connection-list-rows")
            .overflow_y_scroll()
            .track_scroll(&scroll_handle)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .w_full()
                    .children(connections.iter().enumerate().map(|(index, conn)| {
                        let is_selected = selected_id
                            .as_ref()
                            .map(|id| id == &conn.id)
                            .unwrap_or(false);
                        self.render_connection_row(conn, index, is_selected, on_select.clone())
                    })),
            )
    }

    /// Render a single connection row
    fn render_connection_row(
        &self,
        conn: &TcpConnectionRow,
        index: usize,
        selected: bool,
        on_select: Option<OnConnectionSelect>,
    ) -> impl IntoElement {
        let bg = if selected {
            rgb(colors::SURFACE_0)
        } else if index % 2 == 0 {
            rgb(colors::BASE)
        } else {
            rgb(colors::MANTLE)
        };

        let protocol_color = get_protocol_color(&conn.protocol);
        let status_color = get_status_color(&conn.status);

        // Clone connection data for the callback
        let conn_for_callback = conn.clone();
        let target_display = if conn.target_host.len() > 50 {
            format!("{}...", &conn.target_host[..47])
        } else {
            conn.target_host.clone()
        };

        let mut el = div()
            .id(ElementId::Name(format!("conn-{}", conn.id).into()))
            .flex()
            .items_center()
            .h(dimensions::REQUEST_ROW_HEIGHT)
            .px(spacing::MD)
            .bg(bg)
            .hover(|style| style.bg(rgb(colors::SURFACE_0)))
            .cursor_pointer()
            .border_b_1()
            .border_color(rgb(colors::SURFACE_0))
            // Protocol column
            .child(
                div()
                    .w(px(90.0))
                    .flex_shrink_0()
                    .child(render_protocol_badge(&conn.protocol, protocol_color)),
            )
            // Status column
            .child(
                div()
                    .w(px(70.0))
                    .flex_shrink_0()
                    .child(render_status_badge(&conn.status, status_color)),
            )
            // Target host column
            .child(
                div()
                    .flex_1()
                    .min_w(px(200.0))
                    .overflow_hidden()
                    .text_size(font_size::SM)
                    .text_color(self.theme.text_primary)
                    .text_ellipsis()
                    .child(target_display),
            )
            // Duration column
            .child(
                div()
                    .w(px(80.0))
                    .flex_shrink_0()
                    .text_size(font_size::SM)
                    .text_color(self.theme.text_secondary)
                    .child(format_duration(conn.duration_ms)),
            )
            // Bytes column
            .child(
                div()
                    .w(px(100.0))
                    .flex_shrink_0()
                    .text_size(font_size::SM)
                    .text_color(self.theme.text_secondary)
                    .child(format!(
                        "↑{} ↓{}",
                        format_bytes(conn.bytes_sent),
                        format_bytes(conn.bytes_received)
                    )),
            )
            // Messages column
            .child(
                div()
                    .w(px(80.0))
                    .flex_shrink_0()
                    .text_size(font_size::SM)
                    .text_color(self.theme.text_secondary)
                    .child(format!("{}/{}", conn.client_messages, conn.server_messages)),
            );

        if let Some(callback) = on_select {
            el = el.on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                callback(&conn_for_callback, cx);
            });
        }

        el
    }

    /// Render empty state when there are no connections
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
                    .child("No connections yet"),
            )
            .child(
                div()
                    .text_size(font_size::SM)
                    .text_color(self.theme.text_muted)
                    .child("TCP connections (PostgreSQL, Kafka, etc.) will appear here"),
            )
    }
}

/// Render protocol badge
fn render_protocol_badge(protocol: &str, color: Hsla) -> impl IntoElement {
    let label = match protocol {
        "postgresql" => "PG",
        "kafka" => "Kafka",
        "unknown" => "TCP",
        _ => protocol,
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
fn render_status_badge(status: &str, color: Hsla) -> impl IntoElement {
    div()
        .px(spacing::SM)
        .py(px(2.0))
        .bg(color.opacity(0.2))
        .rounded(px(4.0))
        .text_size(font_size::XS)
        .text_color(color)
        .font_weight(FontWeight::MEDIUM)
        .child(status.to_uppercase())
}

/// Get color for protocol type
fn get_protocol_color(protocol: &str) -> Hsla {
    match protocol {
        "postgresql" => hsla(210.0 / 360.0, 0.7, 0.6, 1.0), // Blue
        "kafka" => hsla(30.0 / 360.0, 0.8, 0.5, 1.0),       // Orange
        _ => hsla(0.0, 0.0, 0.5, 1.0),                      // Gray
    }
}

/// Get color for status
fn get_status_color(status: &str) -> Hsla {
    match status {
        "ok" => hsla(142.0 / 360.0, 0.7, 0.45, 1.0),    // Green
        "error" => hsla(0.0 / 360.0, 0.7, 0.5, 1.0),    // Red
        "timeout" => hsla(45.0 / 360.0, 0.8, 0.5, 1.0), // Yellow
        _ => hsla(0.0, 0.0, 0.5, 1.0),                  // Gray
    }
}

/// Format duration in ms to human readable
fn format_duration(ms: f64) -> String {
    if ms < 1.0 {
        format!("{:.0}µs", ms * 1000.0)
    } else if ms < 1000.0 {
        format!("{:.0}ms", ms)
    } else if ms < 60000.0 {
        format!("{:.1}s", ms / 1000.0)
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

/// Standalone function to render connection list
pub fn connection_list(props: ConnectionListProps) -> impl IntoElement {
    ConnectionList::new(props).render()
}
