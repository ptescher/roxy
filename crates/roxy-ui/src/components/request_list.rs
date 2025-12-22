//! Request list component for Roxy UI
//!
//! This component renders the scrollable list of HTTP requests
//! with a header and individual request rows.

use gpui::prelude::*;
use gpui::*;
use roxy_core::HttpRequestRecord;
use std::sync::Arc;

use crate::components::request_row::format_bytes;
use crate::theme::{colors, dimensions, font_size, spacing, Theme};

/// Callback type for request selection
pub type OnRequestSelect = Arc<dyn Fn(&HttpRequestRecord, &mut App) + Send + Sync + 'static>;

/// Properties for the RequestList component
#[derive(Clone)]
pub struct RequestListProps {
    /// List of requests to display
    pub requests: Vec<HttpRequestRecord>,
    /// Currently selected request ID (if any)
    pub selected_request_id: Option<String>,
    /// Callback when a request row is clicked
    pub on_request_select: Option<OnRequestSelect>,
    /// Scroll handle for the request list
    pub scroll_handle: ScrollHandle,
}

impl Default for RequestListProps {
    fn default() -> Self {
        Self {
            requests: Vec::new(),
            selected_request_id: None,
            on_request_select: None,
            scroll_handle: ScrollHandle::new(),
        }
    }
}

/// Request list component
pub struct RequestList {
    props: RequestListProps,
    theme: Theme,
}

impl RequestList {
    /// Create a new request list with the given props
    pub fn new(props: RequestListProps) -> Self {
        Self {
            props,
            theme: Theme::dark(),
        }
    }

    /// Render the request list
    pub fn render(&self) -> impl IntoElement {
        let requests = &self.props.requests;

        // Fill the container completely - parent controls the size
        div()
            .flex()
            .flex_col()
            .size_full()
            .overflow_hidden()
            // Header - fixed height, no shrink
            .child(div().flex_shrink_0().child(self.render_header()))
            // Rows - fill remaining space
            .child(div().flex_1().overflow_hidden().child(self.render_rows()))
            .when(requests.is_empty(), |this| {
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
            .child(div().w(px(60.0)).child("Method"))
            .child(div().w(px(60.0)).child("Status"))
            .child(div().flex_1().child("URL"))
            .child(div().w(px(80.0)).child("Duration"))
            .child(div().w(px(80.0)).child("Size"))
    }

    /// Render all request rows
    fn render_rows(&self) -> impl IntoElement {
        let requests = self.props.requests.clone();
        let selected_id = self.props.selected_request_id.clone();
        let on_select = self.props.on_request_select.clone();
        let scroll_handle = self.props.scroll_handle.clone();

        div()
            .size_full()
            .bg(rgb(colors::BASE))
            .id("request-list-rows")
            .overflow_y_scroll()
            .track_scroll(&scroll_handle)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .w_full()
                    .children(requests.iter().enumerate().map(|(index, request)| {
                        let is_selected = selected_id
                            .as_ref()
                            .map(|id| id == &request.id)
                            .unwrap_or(false);
                        self.render_request_row(request, index, is_selected, on_select.clone())
                    })),
            )
    }

    /// Render a single request row
    fn render_request_row(
        &self,
        request: &HttpRequestRecord,
        index: usize,
        selected: bool,
        on_select: Option<OnRequestSelect>,
    ) -> impl IntoElement {
        let bg = if selected {
            rgb(colors::SURFACE_0)
        } else if index % 2 == 0 {
            rgb(colors::BASE)
        } else {
            rgb(colors::MANTLE)
        };

        let method_color = self.theme.method_color(&request.method);
        let status_color = self.theme.status_color(request.response_status);

        let url_display = if request.url.len() > 60 {
            format!("{}...", &request.url[..57])
        } else {
            request.url.clone()
        };

        // Clone request data for the callback
        let request_for_callback = request.clone();
        let method = request.method.clone();
        let status = request.response_status;
        let duration = request.duration_ms;
        let size = request.response_body_size;

        let mut el = div()
            .flex()
            .items_center()
            .h(dimensions::REQUEST_ROW_HEIGHT)
            .px(spacing::MD)
            .bg(bg)
            .hover(|style| style.bg(rgb(colors::SURFACE_0)))
            .cursor_pointer()
            .text_size(font_size::SM)
            .child(
                div()
                    .w(px(60.0))
                    .font_weight(FontWeight::BOLD)
                    .text_color(method_color)
                    .child(method),
            )
            .child(
                div()
                    .w(px(60.0))
                    .text_color(status_color)
                    .child(status.to_string()),
            )
            .child(
                div()
                    .flex_1()
                    .overflow_hidden()
                    .text_color(self.theme.text_primary)
                    .child(url_display),
            )
            .child(
                div()
                    .w(px(80.0))
                    .text_color(self.theme.text_muted)
                    .child(format!("{:.1}ms", duration)),
            )
            .child(
                div()
                    .w(px(80.0))
                    .text_color(self.theme.text_muted)
                    .child(format_bytes(size)),
            );

        if let Some(callback) = on_select {
            el = el.on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                callback(&request_for_callback, cx);
            });
        }

        el
    }

    /// Render the empty state when no requests are available
    fn render_empty_state(&self) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .h(px(200.0))
            .gap(spacing::XS)
            .child(
                div()
                    .text_size(font_size::MD)
                    .text_color(self.theme.text_muted)
                    .child("No requests captured yet"),
            )
            .child(
                div()
                    .text_size(font_size::SM)
                    .text_color(self.theme.text_muted)
                    .child("Configure your browser or app to use the proxy at 127.0.0.1:8080"),
            )
    }
}

/// Convenience function to render a request list
pub fn request_list(props: RequestListProps) -> impl IntoElement {
    RequestList::new(props).render()
}
