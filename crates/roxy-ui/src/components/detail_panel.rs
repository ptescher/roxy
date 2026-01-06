//! Detail panel component for Roxy UI
//!
//! This component renders the bottom panel that displays detailed
//! information about a selected HTTP request, including headers,
//! request body, response body, and timing information.

use gpui::prelude::*;
use gpui::*;
use roxy_core::HttpRequestRecord;
use std::sync::Arc;

use crate::theme::{colors, dimensions, font_size, spacing, Theme};

/// Available tabs in the detail panel
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DetailTab {
    /// Request and response headers
    #[default]
    Headers,
    /// Request body
    Request,
    /// Response body
    Response,
    /// Timing breakdown
    Timing,
    /// Raw span data as JSON
    Raw,
}

impl DetailTab {
    /// Get the display label for this tab
    pub fn label(&self) -> &'static str {
        match self {
            DetailTab::Headers => "Headers",
            DetailTab::Request => "Request",
            DetailTab::Response => "Response",
            DetailTab::Timing => "Timing",
            DetailTab::Raw => "Raw",
        }
    }

    /// Get all available tabs
    pub fn all() -> &'static [DetailTab] {
        &[
            DetailTab::Headers,
            DetailTab::Request,
            DetailTab::Response,
            DetailTab::Timing,
            DetailTab::Raw,
        ]
    }
}

/// Callback type for tab selection
pub type OnTabSelect = Arc<dyn Fn(DetailTab, &mut App) + Send + Sync + 'static>;

/// Properties for the DetailPanel component
#[derive(Clone)]
pub struct DetailPanelProps {
    /// The selected request to display details for
    pub selected_request: Option<HttpRequestRecord>,
    /// Currently active tab
    pub active_tab: DetailTab,
    /// Callback when a tab is clicked
    pub on_tab_select: Option<OnTabSelect>,
    /// Height of the detail panel in pixels
    pub height: f32,
    /// Scroll handle for the detail panel content
    pub scroll_handle: ScrollHandle,
}

impl Default for DetailPanelProps {
    fn default() -> Self {
        Self {
            selected_request: None,
            active_tab: DetailTab::default(),
            on_tab_select: None,
            height: 300.0,
            scroll_handle: ScrollHandle::new(),
        }
    }
}

/// Detail panel component
pub struct DetailPanel {
    props: DetailPanelProps,
    theme: Theme,
}

impl DetailPanel {
    /// Create a new detail panel with the given props
    pub fn new(props: DetailPanelProps) -> Self {
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
            .h(px(self.props.height))
            .border_t_1()
            .border_color(rgb(colors::SURFACE_0))
            .bg(rgb(colors::MANTLE))
            .child(self.render_tabs())
            .child(self.render_content())
    }

    /// Render the tab bar
    fn render_tabs(&self) -> impl IntoElement {
        let on_tab_select = self.props.on_tab_select.clone();

        div()
            .flex()
            .items_center()
            .h(dimensions::TAB_HEIGHT)
            .px(spacing::MD)
            .gap(spacing::XXS)
            .border_b_1()
            .border_color(rgb(colors::SURFACE_0))
            .children(
                DetailTab::all()
                    .iter()
                    .map(|tab| self.render_tab(*tab, on_tab_select.clone())),
            )
    }

    /// Render a single tab
    fn render_tab(&self, tab: DetailTab, on_select: Option<OnTabSelect>) -> impl IntoElement {
        let is_active = self.props.active_tab == tab;

        let bg = if is_active {
            rgb(colors::SURFACE_0)
        } else {
            rgba(colors::TRANSPARENT)
        };

        let mut el = div()
            .px(spacing::SM)
            .py(spacing::XXS)
            .rounded(dimensions::BORDER_RADIUS)
            .bg(bg)
            .text_size(font_size::SM)
            .font_weight(FontWeight::MEDIUM)
            .text_color(if is_active {
                self.theme.text_primary
            } else {
                self.theme.text_muted
            })
            .cursor_pointer()
            .hover(|style| style.bg(rgb(colors::SURFACE_0)))
            .child(tab.label());

        if let Some(callback) = on_select {
            el = el.on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                callback(tab, cx);
            });
        }

        el
    }

    /// Render the content area based on selected request and active tab
    fn render_content(&self) -> impl IntoElement {
        let scroll_handle = self.props.scroll_handle.clone();
        match &self.props.selected_request {
            Some(request) => div()
                .flex_1()
                .p(spacing::MD)
                .id("detail-panel-content")
                .overflow_y_scroll()
                .track_scroll(&scroll_handle)
                .child(self.render_request_content(request)),
            None => div()
                .flex_1()
                .p(spacing::MD)
                .id("detail-panel-empty")
                .overflow_hidden()
                .child(self.render_empty_state()),
        }
    }

    /// Render content for a selected request
    fn render_request_content(&self, request: &HttpRequestRecord) -> impl IntoElement {
        // Clone data to avoid lifetime issues with match
        let headers_content = self.render_headers_tab(request);
        let request_content = self.render_request_tab(request);
        let response_content = self.render_response_tab(request);
        let timing_content = self.render_timing_tab(request);
        let raw_content = self.render_raw_tab(request);

        div().child(match self.props.active_tab {
            DetailTab::Headers => div().child(headers_content),
            DetailTab::Request => div().child(request_content),
            DetailTab::Response => div().child(response_content),
            DetailTab::Timing => div().child(timing_content),
            DetailTab::Raw => div().child(raw_content),
        })
    }

    /// Render the headers tab content
    fn render_headers_tab(&self, request: &HttpRequestRecord) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap(spacing::MD)
            .child(self.render_section("General", self.render_general_info(request)))
            .child(self.render_section(
                "Request Headers",
                self.render_headers(&request.request_headers),
            ))
            .child(self.render_section(
                "Response Headers",
                self.render_headers(&request.response_headers),
            ))
    }

    /// Render general request information
    fn render_general_info(&self, request: &HttpRequestRecord) -> impl IntoElement {
        let mut el = div()
            .flex()
            .flex_col()
            .gap(spacing::XXS)
            .child(self.render_info_row("Request URL", &request.url))
            .child(self.render_info_row("Request Method", &request.method))
            .child(self.render_info_row("Status Code", &request.response_status.to_string()))
            .child(self.render_info_row("Remote Address", &request.server_ip))
            .child(self.render_info_row("Protocol", &request.protocol));

        // Add client name if present
        if !request.client_name.is_empty() {
            el = el.child(self.render_info_row("Client App", &request.client_name));
        }

        el
    }

    /// Render an info row with label and value
    fn render_info_row(&self, label: &str, value: &str) -> impl IntoElement {
        div()
            .flex()
            .gap(spacing::XS)
            .child(
                div()
                    .text_size(font_size::SM)
                    .text_color(self.theme.text_muted)
                    .min_w(px(120.0))
                    .child(format!("{}:", label)),
            )
            .child(
                div()
                    .text_size(font_size::SM)
                    .text_color(self.theme.text_primary)
                    .child(value.to_string()),
            )
    }

    /// Render headers from JSON string
    fn render_headers(&self, headers_json: &str) -> impl IntoElement {
        // Try to parse headers as JSON
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(headers_json);

        match parsed {
            Ok(serde_json::Value::Object(map)) => {
                let items: Vec<_> = map
                    .iter()
                    .map(|(k, v)| {
                        let value = match v {
                            serde_json::Value::String(s) => s.clone(),
                            other => other.to_string(),
                        };
                        self.render_info_row(k, &value)
                    })
                    .collect();

                div().flex().flex_col().gap(spacing::XXS).children(items)
            }
            _ => div()
                .text_size(font_size::SM)
                .text_color(self.theme.text_muted)
                .child(if headers_json.is_empty() {
                    "(no headers)".to_string()
                } else {
                    headers_json.to_string()
                }),
        }
    }

    /// Render the request tab content (request body)
    fn render_request_tab(&self, request: &HttpRequestRecord) -> impl IntoElement {
        self.render_body_content(
            "Request Body",
            &request.request_body,
            request.request_body_size,
        )
    }

    /// Render the response tab content (response body)
    fn render_response_tab(&self, request: &HttpRequestRecord) -> impl IntoElement {
        self.render_body_content(
            "Response Body",
            &request.response_body,
            request.response_body_size,
        )
    }

    /// Render body content with size info
    fn render_body_content(&self, title: &str, body: &str, size: i64) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap(spacing::XS)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(font_size::SM)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(self.theme.text_secondary)
                            .child(title.to_string()),
                    )
                    .child(
                        div()
                            .text_size(font_size::XS)
                            .text_color(self.theme.text_muted)
                            .child(format_bytes(size)),
                    ),
            )
            .child(if body.is_empty() {
                div()
                    .p(spacing::MD)
                    .rounded(dimensions::BORDER_RADIUS)
                    .bg(rgb(colors::CRUST))
                    .text_size(font_size::SM)
                    .text_color(self.theme.text_muted)
                    .child("(empty body)")
            } else {
                div()
                    .p(spacing::SM)
                    .rounded(dimensions::BORDER_RADIUS)
                    .bg(rgb(colors::CRUST))
                    .text_size(font_size::SM)
                    .font_family("Berkeley Mono")
                    .text_color(self.theme.text_primary)
                    .overflow_hidden()
                    .child(self.format_body(body))
            })
    }

    /// Format body content (try to pretty-print JSON)
    fn format_body(&self, body: &str) -> String {
        // Check if it's base64 encoded
        if body.starts_with("base64:") {
            let encoded_preview = &body[7..body.len().min(57)];
            return format!("[Binary data, base64 encoded: {}...]", encoded_preview);
        }

        // Try to parse and pretty-print JSON
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(body) {
            serde_json::to_string_pretty(&json).unwrap_or_else(|_| body.to_string())
        } else {
            body.to_string()
        }
    }

    /// Render the raw tab content (full span as JSON)
    fn render_raw_tab(&self, request: &HttpRequestRecord) -> impl IntoElement {
        let json = serde_json::to_string_pretty(request)
            .unwrap_or_else(|e| format!("Error serializing: {}", e));

        div()
            .flex()
            .flex_col()
            .gap(spacing::XS)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(font_size::SM)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(self.theme.text_secondary)
                            .child("Raw Span Data"),
                    )
                    .child(
                        div()
                            .text_size(font_size::XS)
                            .text_color(self.theme.text_muted)
                            .child(format!("{} bytes", json.len())),
                    ),
            )
            .child(
                div()
                    .p(spacing::SM)
                    .rounded(dimensions::BORDER_RADIUS)
                    .bg(rgb(colors::CRUST))
                    .text_size(font_size::SM)
                    .font_family("Berkeley Mono")
                    .text_color(self.theme.text_primary)
                    .overflow_hidden()
                    .child(json),
            )
    }

    /// Render the timing tab content
    fn render_timing_tab(&self, request: &HttpRequestRecord) -> impl IntoElement {
        div().flex().flex_col().gap(spacing::MD).child(
            self.render_section(
                "Timing Overview",
                div()
                    .flex()
                    .flex_col()
                    .gap(spacing::XS)
                    .child(self.render_info_row(
                        "Total Duration",
                        &format!("{:.2}ms", request.duration_ms),
                    ))
                    .child(self.render_timing_bar(
                        "Total",
                        request.duration_ms,
                        request.duration_ms,
                    )),
            ),
        )
    }

    /// Render a visual timing bar
    fn render_timing_bar(
        &self,
        label: &str,
        duration_ms: f64,
        max_duration: f64,
    ) -> impl IntoElement {
        let bar_width = if max_duration > 0.0 {
            (duration_ms / max_duration * 300.0).min(300.0) as f32
        } else {
            0.0
        };

        div()
            .flex()
            .items_center()
            .gap(spacing::SM)
            .child(
                div()
                    .w(px(80.0))
                    .text_size(font_size::SM)
                    .text_color(self.theme.text_muted)
                    .child(label.to_string()),
            )
            .child(
                div()
                    .flex_1()
                    .h(px(20.0))
                    .rounded(dimensions::BORDER_RADIUS)
                    .bg(rgb(colors::SURFACE_0))
                    .child(
                        div()
                            .h(px(20.0))
                            .w(px(bar_width))
                            .rounded(dimensions::BORDER_RADIUS)
                            .bg(self.theme.info),
                    ),
            )
            .child(
                div()
                    .w(px(80.0))
                    .text_size(font_size::SM)
                    .text_color(self.theme.text_primary)
                    .child(format!("{:.2}ms", duration_ms)),
            )
    }

    /// Render a section with title and content
    fn render_section(&self, title: &str, content: impl IntoElement) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap(spacing::XS)
            .child(
                div()
                    .text_size(font_size::SM)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(self.theme.text_secondary)
                    .child(title.to_string()),
            )
            .child(content)
    }

    /// Render the empty state when no request is selected
    fn render_empty_state(&self) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .justify_center()
            .h_full()
            .text_size(font_size::MD)
            .text_color(self.theme.text_muted)
            .child("Select a request to view details")
    }
}

/// Format bytes to human-readable string
fn format_bytes(bytes: i64) -> String {
    if bytes < 0 {
        "0 B".to_string()
    } else if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

/// Convenience function to render a detail panel
pub fn detail_panel(props: DetailPanelProps) -> impl IntoElement {
    DetailPanel::new(props).render()
}
