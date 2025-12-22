//! Request list component for Roxy UI
//!
//! This component renders the scrollable list of HTTP requests
//! with a header and individual request rows.

use gpui::prelude::*;
use gpui::*;
use roxy_core::HttpRequestRecord;

use crate::components::request_row::{format_bytes, RequestRowProps};
use crate::theme::{colors, dimensions, font_size, spacing, Theme};

/// Properties for the RequestList component
#[derive(Clone, Default)]
pub struct RequestListProps {
    /// List of requests to display
    pub requests: Vec<HttpRequestRecord>,
    /// Currently selected request ID (if any)
    pub selected_request_id: Option<String>,
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

        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(200.0))
            .overflow_hidden()
            .child(self.render_header())
            .child(self.render_rows())
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
        let requests = &self.props.requests;
        let selected_id = &self.props.selected_request_id;

        div().flex().flex_col().flex_1().overflow_hidden().children(
            requests.iter().enumerate().map(|(index, request)| {
                let is_selected = selected_id
                    .as_ref()
                    .map(|id| id == &request.id)
                    .unwrap_or(false);
                self.render_request_row(request, index, is_selected)
            }),
        )
    }

    /// Render a single request row
    fn render_request_row(
        &self,
        request: &HttpRequestRecord,
        index: usize,
        selected: bool,
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

        div()
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
                    .child(request.method.clone()),
            )
            .child(
                div()
                    .w(px(60.0))
                    .text_color(status_color)
                    .child(request.response_status.to_string()),
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
                    .child(format!("{:.1}ms", request.duration_ms)),
            )
            .child(
                div()
                    .w(px(80.0))
                    .text_color(self.theme.text_muted)
                    .child(format_bytes(request.response_body_size)),
            )
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
