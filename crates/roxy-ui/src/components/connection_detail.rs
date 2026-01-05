//! Connection detail panel component for Roxy UI
//!
//! This component displays detailed information about a selected TCP connection,
//! including raw bytes in hex and ASCII format for debugging unknown protocols.

use gpui::prelude::*;
use gpui::*;
use roxy_core::TcpConnectionRow;
use std::sync::Arc;

use crate::theme::{colors, dimensions, font_size, spacing, Theme};

/// Tabs available in the connection detail panel
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConnectionDetailTab {
    /// Overview of the connection
    #[default]
    Overview,
    /// Raw bytes from initial connection
    InitialBytes,
    /// Sample request data
    Request,
    /// Sample response data
    Response,
}

impl ConnectionDetailTab {
    pub fn label(&self) -> &'static str {
        match self {
            ConnectionDetailTab::Overview => "Overview",
            ConnectionDetailTab::InitialBytes => "Initial Bytes",
            ConnectionDetailTab::Request => "Request",
            ConnectionDetailTab::Response => "Response",
        }
    }

    pub fn all() -> &'static [ConnectionDetailTab] {
        &[
            ConnectionDetailTab::Overview,
            ConnectionDetailTab::InitialBytes,
            ConnectionDetailTab::Request,
            ConnectionDetailTab::Response,
        ]
    }
}

/// Callback type for tab selection
pub type OnTabSelect = Arc<dyn Fn(ConnectionDetailTab, &mut App) + Send + Sync + 'static>;

/// Properties for the ConnectionDetailPanel component
#[derive(Clone)]
pub struct ConnectionDetailPanelProps {
    /// The selected connection to display details for
    pub selected_connection: Option<TcpConnectionRow>,
    /// Currently active tab
    pub active_tab: ConnectionDetailTab,
    /// Callback when a tab is selected
    pub on_tab_select: Option<OnTabSelect>,
    /// Panel height
    pub height: f32,
    /// Scroll handle for the content
    pub scroll_handle: ScrollHandle,
}

impl Default for ConnectionDetailPanelProps {
    fn default() -> Self {
        Self {
            selected_connection: None,
            active_tab: ConnectionDetailTab::default(),
            on_tab_select: None,
            height: 300.0,
            scroll_handle: ScrollHandle::new(),
        }
    }
}

/// Connection detail panel component
pub struct ConnectionDetailPanel {
    props: ConnectionDetailPanelProps,
    theme: Theme,
}

impl ConnectionDetailPanel {
    /// Create a new connection detail panel
    pub fn new(props: ConnectionDetailPanelProps) -> Self {
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
            .h(dimensions::TAB_HEIGHT)
            .px(spacing::MD)
            .gap(spacing::XS)
            .bg(rgb(colors::MANTLE))
            .border_b_1()
            .border_color(rgb(colors::SURFACE_0))
            .children(ConnectionDetailTab::all().iter().map(|&tab| {
                let is_active = tab == active_tab;
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
                    .rounded(px(4.0))
                    .bg(bg)
                    .hover(|style| style.bg(rgb(colors::SURFACE_0)))
                    .cursor_pointer()
                    .text_size(font_size::SM)
                    .text_color(text_color)
                    .child(tab.label());

                if let Some(callback) = on_select {
                    el = el.on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        callback(tab, cx);
                    });
                }

                el
            }))
    }

    /// Render the content area based on selected tab
    fn render_content(&self) -> impl IntoElement {
        let scroll_handle = self.props.scroll_handle.clone();

        let content: AnyElement = match &self.props.selected_connection {
            Some(conn) => self.render_connection_content(conn).into_any_element(),
            None => self.render_empty_state().into_any_element(),
        };

        div().flex_1().overflow_hidden().child(
            div()
                .size_full()
                .id("connection-detail-content")
                .overflow_y_scroll()
                .track_scroll(&scroll_handle)
                .p(spacing::MD)
                .child(content),
        )
    }

    /// Render content for the selected connection
    fn render_connection_content(&self, conn: &TcpConnectionRow) -> impl IntoElement {
        match self.props.active_tab {
            ConnectionDetailTab::Overview => self.render_overview(conn).into_any_element(),
            ConnectionDetailTab::InitialBytes => self
                .render_hex_view(&conn.initial_bytes_hex, &conn.initial_bytes_ascii)
                .into_any_element(),
            ConnectionDetailTab::Request => self
                .render_hex_view(&conn.sample_request_hex, "")
                .into_any_element(),
            ConnectionDetailTab::Response => self
                .render_hex_view(&conn.sample_response_hex, "")
                .into_any_element(),
        }
    }

    /// Render overview tab
    fn render_overview(&self, conn: &TcpConnectionRow) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap(spacing::MD)
            // Connection info section
            .child(
                self.render_section(
                    "Connection",
                    div()
                        .flex()
                        .flex_col()
                        .gap(spacing::XS)
                        .child(self.render_field("Target", &conn.target_host))
                        .child(self.render_field(
                            "Server",
                            &format!("{}:{}", conn.server_address, conn.server_port),
                        ))
                        .child(self.render_field(
                            "Client",
                            &format!("{}:{}", conn.client_address, conn.client_port),
                        ))
                        .child(self.render_field("Trace ID", &conn.trace_id)),
                ),
            )
            // Protocol info section
            .child(
                self.render_section(
                    "Protocol",
                    div()
                        .flex()
                        .flex_col()
                        .gap(spacing::XS)
                        .child(self.render_field("Protocol", &format_protocol(&conn.protocol)))
                        .child(self.render_field("Detection", &conn.protocol_detected))
                        .child(self.render_field(
                            "Wire Detected",
                            if conn.wire_detected == 1 { "Yes" } else { "No" },
                        )),
                ),
            )
            // Status section
            .child(
                self.render_section(
                    "Status",
                    div()
                        .flex()
                        .flex_col()
                        .gap(spacing::XS)
                        .child(self.render_field_with_color(
                            "Status",
                            &conn.status.to_uppercase(),
                            get_status_color(&conn.status),
                        ))
                        .child(self.render_field("Duration", &format_duration(conn.duration_ms)))
                        .when(!conn.error_message.is_empty(), |this| {
                            this.child(self.render_field_with_color(
                                "Error",
                                &conn.error_message,
                                hsla(0.0, 0.7, 0.5, 1.0),
                            ))
                        }),
                ),
            )
            // Traffic stats section
            .child(
                self.render_section(
                    "Traffic",
                    div()
                        .flex()
                        .flex_col()
                        .gap(spacing::XS)
                        .child(self.render_field("Bytes Sent", &format_bytes(conn.bytes_sent)))
                        .child(
                            self.render_field("Bytes Received", &format_bytes(conn.bytes_received)),
                        )
                        .child(
                            self.render_field("Client Messages", &conn.client_messages.to_string()),
                        )
                        .child(
                            self.render_field("Server Messages", &conn.server_messages.to_string()),
                        )
                        .child(self.render_field("Parse Errors", &conn.parse_errors.to_string())),
                ),
            )
    }

    /// Render a section with title and content
    fn render_section(&self, title: &str, content: impl IntoElement) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap(spacing::SM)
            .child(
                div()
                    .text_size(font_size::SM)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(colors::TEXT))
                    .child(title.to_string()),
            )
            .child(
                div()
                    .pl(spacing::MD)
                    .border_l_2()
                    .border_color(rgb(colors::SURFACE_1))
                    .child(content),
            )
    }

    /// Render a field with label and value
    fn render_field(&self, label: &str, value: &str) -> impl IntoElement {
        div()
            .flex()
            .gap(spacing::SM)
            .child(
                div()
                    .w(px(120.0))
                    .text_size(font_size::SM)
                    .text_color(self.theme.text_muted)
                    .child(format!("{}:", label)),
            )
            .child(
                div()
                    .flex_1()
                    .text_size(font_size::SM)
                    .text_color(self.theme.text_primary)
                    .child(value.to_string()),
            )
    }

    /// Render a field with custom color
    fn render_field_with_color(&self, label: &str, value: &str, color: Hsla) -> impl IntoElement {
        div()
            .flex()
            .gap(spacing::SM)
            .child(
                div()
                    .w(px(120.0))
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
                    .child(value.to_string()),
            )
    }

    /// Render hex view with both hex and ASCII representation
    fn render_hex_view(&self, hex: &str, ascii: &str) -> impl IntoElement {
        if hex.is_empty() {
            return div()
                .flex()
                .items_center()
                .justify_center()
                .py(spacing::XL)
                .child(
                    div()
                        .text_size(font_size::SM)
                        .text_color(self.theme.text_muted)
                        .child("No data captured"),
                )
                .into_any_element();
        }

        // Parse hex string into bytes for display
        let bytes: Vec<u8> = hex
            .split_whitespace()
            .filter_map(|s| u8::from_str_radix(s, 16).ok())
            .collect();

        div()
            .flex()
            .flex_col()
            .gap(spacing::MD)
            // Stats
            .child(
                div()
                    .text_size(font_size::SM)
                    .text_color(self.theme.text_muted)
                    .child(format!("{} bytes captured", bytes.len())),
            )
            // Hex dump with ASCII
            .child(self.render_hex_dump(&bytes))
            // Raw ASCII preview if available
            .when(!ascii.is_empty(), |this| {
                this.child(
                    self.render_section(
                        "ASCII Preview",
                        div()
                            .p(spacing::SM)
                            .bg(rgb(colors::BASE))
                            .rounded(px(4.0))
                            .font_family("monospace")
                            .text_size(font_size::SM)
                            .text_color(self.theme.text_primary)
                            .child(ascii.to_string()),
                    ),
                )
            })
            .into_any_element()
    }

    /// Render hex dump in classic format (offset, hex, ascii)
    fn render_hex_dump(&self, bytes: &[u8]) -> impl IntoElement {
        let mut rows: Vec<_> = Vec::new();
        let bytes_per_row = 16;

        for (i, chunk) in bytes.chunks(bytes_per_row).enumerate() {
            let offset = i * bytes_per_row;
            rows.push(self.render_hex_row(offset, chunk));
        }

        div()
            .flex()
            .flex_col()
            .p(spacing::SM)
            .bg(rgb(colors::BASE))
            .rounded(px(4.0))
            .font_family("monospace")
            .text_size(font_size::XS)
            .id("hex-dump")
            .overflow_x_scroll()
            .children(rows)
    }

    /// Render a single row of hex dump
    fn render_hex_row(&self, offset: usize, bytes: &[u8]) -> impl IntoElement {
        // Build hex string
        let mut hex_parts: Vec<String> = Vec::new();
        for (i, byte) in bytes.iter().enumerate() {
            hex_parts.push(format!("{:02x}", byte));
            if i == 7 {
                hex_parts.push(" ".to_string()); // Extra space in middle
            }
        }
        // Pad if less than 16 bytes
        while hex_parts.len() < 17 {
            hex_parts.push("  ".to_string());
        }
        let hex_str = hex_parts.join(" ");

        // Build ASCII string
        let ascii_str: String = bytes
            .iter()
            .map(|&b| {
                if b.is_ascii_graphic() || b == b' ' {
                    b as char
                } else {
                    '.'
                }
            })
            .collect();

        div()
            .flex()
            .gap(spacing::MD)
            // Offset
            .child(
                div()
                    .w(px(60.0))
                    .text_color(rgb(colors::OVERLAY_1))
                    .child(format!("{:08x}", offset)),
            )
            // Hex
            .child(
                div()
                    .w(px(400.0))
                    .text_color(rgb(colors::BLUE))
                    .child(hex_str),
            )
            // ASCII
            .child(
                div()
                    .text_color(rgb(colors::GREEN))
                    .child(format!("|{}|", ascii_str)),
            )
    }

    /// Render empty state when no connection is selected
    fn render_empty_state(&self) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .justify_center()
            .size_full()
            .child(
                div()
                    .text_size(font_size::SM)
                    .text_color(self.theme.text_muted)
                    .child("Select a connection to view details"),
            )
    }
}

/// Format protocol name for display
fn format_protocol(protocol: &str) -> String {
    match protocol {
        "postgresql" => "PostgreSQL".to_string(),
        "kafka" => "Apache Kafka".to_string(),
        "unknown" => "Unknown (TCP)".to_string(),
        _ => protocol.to_string(),
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
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

/// Standalone function to render connection detail panel
pub fn connection_detail_panel(props: ConnectionDetailPanelProps) -> impl IntoElement {
    ConnectionDetailPanel::new(props).render()
}
