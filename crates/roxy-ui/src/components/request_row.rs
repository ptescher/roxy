//! Request row component for Roxy UI
//!
//! This component renders a single row in the request list,
//! displaying request information like method, status, URL,
//! duration, and response size.

use gpui::prelude::*;
use gpui::*;
use roxy_core::HttpRequestRecord;

use crate::theme::{colors, dimensions, font_size, spacing, Theme};

/// Properties for the RequestRow component
#[derive(Clone)]
pub struct RequestRowProps {
    /// The request record to display
    pub request: HttpRequestRecord,
    /// Row index for alternating backgrounds
    pub index: usize,
    /// Whether this row is selected
    pub selected: bool,
}

/// Request row component
pub struct RequestRow {
    props: RequestRowProps,
    theme: Theme,
}

impl RequestRow {
    /// Create a new request row with the given props
    pub fn new(props: RequestRowProps) -> Self {
        Self {
            props,
            theme: Theme::dark(),
        }
    }

    /// Render the request row
    pub fn render(&self) -> impl IntoElement {
        let bg = self.get_background_color();

        div()
            .flex()
            .items_center()
            .h(dimensions::REQUEST_ROW_HEIGHT)
            .px(spacing::MD)
            .bg(bg)
            .hover(|style| style.bg(rgb(colors::SURFACE_0)))
            .cursor_pointer()
            .text_size(font_size::SM)
            .child(self.render_method())
            .child(self.render_status())
            .child(self.render_url())
            .child(self.render_duration())
            .child(self.render_size())
    }

    /// Get the background color based on selection and index
    fn get_background_color(&self) -> Hsla {
        if self.props.selected {
            rgb(colors::SURFACE_0).into()
        } else if self.props.index % 2 == 0 {
            rgb(colors::BASE).into()
        } else {
            rgb(colors::MANTLE).into()
        }
    }

    /// Render the HTTP method column
    fn render_method(&self) -> impl IntoElement {
        let method_color = self.theme.method_color(&self.props.request.method);

        div()
            .w(px(60.0))
            .font_weight(FontWeight::BOLD)
            .text_color(method_color)
            .child(self.props.request.method.clone())
    }

    /// Render the status code column
    fn render_status(&self) -> impl IntoElement {
        let status = self.props.request.response_status;
        let status_color = self.theme.status_color(status);

        div()
            .w(px(60.0))
            .text_color(status_color)
            .child(status.to_string())
    }

    /// Render the URL column
    fn render_url(&self) -> impl IntoElement {
        let url = &self.props.request.url;
        let url_display = if url.len() > 60 {
            format!("{}...", &url[..57])
        } else {
            url.clone()
        };

        div()
            .flex_1()
            .overflow_hidden()
            .text_color(self.theme.text_primary)
            .child(url_display)
    }

    /// Render the duration column
    fn render_duration(&self) -> impl IntoElement {
        let duration_ms = self.props.request.duration_ms;

        div()
            .w(px(80.0))
            .text_color(self.theme.text_muted)
            .child(format!("{:.1}ms", duration_ms))
    }

    /// Render the response size column
    fn render_size(&self) -> impl IntoElement {
        let size = self.props.request.response_body_size;

        div()
            .w(px(80.0))
            .text_color(self.theme.text_muted)
            .child(format_bytes(size))
    }
}

/// Format bytes to human-readable string
pub fn format_bytes(bytes: i64) -> String {
    if bytes < 0 {
        return "0 B".to_string();
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

/// Convenience function to render a request row
pub fn request_row(props: RequestRowProps) -> impl IntoElement {
    RequestRow::new(props).render()
}
