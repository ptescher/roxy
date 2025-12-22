//! Toolbar component for Roxy UI
//!
//! This component renders the main toolbar with request count
//! and filter buttons for the network traffic view.

use gpui::prelude::*;
use gpui::*;

use crate::theme::{colors, dimensions, font_size, spacing, Theme};

/// Filter type for requests
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RequestFilter {
    /// Show all requests
    #[default]
    All,
    /// Show only XHR/Fetch requests
    Xhr,
    /// Show only WebSocket connections
    WebSocket,
    /// Show only error responses (4xx, 5xx)
    Errors,
}

impl RequestFilter {
    /// Get the display label for this filter
    pub fn label(&self) -> &'static str {
        match self {
            RequestFilter::All => "All",
            RequestFilter::Xhr => "XHR",
            RequestFilter::WebSocket => "WS",
            RequestFilter::Errors => "Errors",
        }
    }

    /// Get all available filters
    pub fn all() -> &'static [RequestFilter] {
        &[
            RequestFilter::All,
            RequestFilter::Xhr,
            RequestFilter::WebSocket,
            RequestFilter::Errors,
        ]
    }
}

/// Properties for the Toolbar component
#[derive(Clone)]
pub struct ToolbarProps {
    /// Total number of requests
    pub request_count: usize,
    /// Currently active filter
    pub active_filter: RequestFilter,
}

impl Default for ToolbarProps {
    fn default() -> Self {
        Self {
            request_count: 0,
            active_filter: RequestFilter::All,
        }
    }
}

/// Toolbar component
pub struct Toolbar {
    props: ToolbarProps,
    theme: Theme,
}

impl Toolbar {
    /// Create a new toolbar with the given props
    pub fn new(props: ToolbarProps) -> Self {
        Self {
            props,
            theme: Theme::dark(),
        }
    }

    /// Render the toolbar
    pub fn render(&self) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .justify_between()
            .h(dimensions::TOOLBAR_HEIGHT)
            .px(spacing::MD)
            .border_b_1()
            .border_color(rgb(colors::SURFACE_0))
            .child(self.render_left_section())
            .child(self.render_filters())
    }

    /// Render the left section with title and request count
    fn render_left_section(&self) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .gap(spacing::MD)
            .child(
                div()
                    .text_size(font_size::MD)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(self.theme.text_primary)
                    .child("Network Traffic"),
            )
            .child(self.render_request_count())
    }

    /// Render the request count badge
    fn render_request_count(&self) -> impl IntoElement {
        div()
            .px(spacing::XS)
            .py(spacing::XXS)
            .rounded(dimensions::BORDER_RADIUS)
            .bg(rgb(colors::SURFACE_0))
            .text_size(font_size::SM)
            .text_color(self.theme.text_muted)
            .child(format!("{} requests", self.props.request_count))
    }

    /// Render the filter buttons
    fn render_filters(&self) -> impl IntoElement {
        div().flex().items_center().gap(spacing::XS).children(
            RequestFilter::all()
                .iter()
                .map(|filter| self.render_filter_button(*filter)),
        )
    }

    /// Render a single filter button
    fn render_filter_button(&self, filter: RequestFilter) -> impl IntoElement {
        let is_active = self.props.active_filter == filter;

        let (bg, text_color) = if is_active {
            (self.theme.button_primary, self.theme.button_primary_text)
        } else {
            (rgb(colors::SURFACE_0).into(), self.theme.text_primary)
        };

        div()
            .px(spacing::SM)
            .py(spacing::XXS)
            .rounded(dimensions::BORDER_RADIUS)
            .bg(bg)
            .text_size(font_size::SM)
            .font_weight(FontWeight::MEDIUM)
            .text_color(text_color)
            .cursor_pointer()
            .hover(|style| style.opacity(0.8))
            .child(filter.label())
    }
}

/// Convenience function to render a toolbar
pub fn toolbar(props: ToolbarProps) -> impl IntoElement {
    Toolbar::new(props).render()
}
