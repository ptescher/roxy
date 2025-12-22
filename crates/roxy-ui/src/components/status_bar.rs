//! Status bar component for Roxy UI
//!
//! This component renders the bottom status bar with information
//! about captured requests, service status, and error messages.

use gpui::prelude::*;
use gpui::*;

use crate::theme::{colors, dimensions, font_size, spacing, Theme};

/// Properties for the StatusBar component
#[derive(Clone, Default)]
pub struct StatusBarProps {
    /// Number of requests captured
    pub request_count: usize,
    /// Error message to display (if any)
    pub error_message: Option<String>,
    /// ClickHouse connection status
    pub clickhouse_connected: bool,
    /// OpenTelemetry collector status
    pub otel_connected: bool,
}

/// Status bar component
pub struct StatusBar {
    props: StatusBarProps,
    theme: Theme,
}

impl StatusBar {
    /// Create a new status bar with the given props
    pub fn new(props: StatusBarProps) -> Self {
        Self {
            props,
            theme: Theme::dark(),
        }
    }

    /// Render the status bar
    pub fn render(&self) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .justify_between()
            .h(dimensions::STATUS_BAR_HEIGHT)
            .px(spacing::MD)
            .bg(rgb(colors::CRUST))
            .text_size(font_size::XS)
            .text_color(self.theme.text_muted)
            .child(self.render_left_section())
            .child(self.render_right_section())
    }

    /// Render the left section with status information
    fn render_left_section(&self) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .gap(spacing::MD)
            .child(self.render_request_count())
            .child(self.render_service_status(
                "ClickHouse",
                "127.0.0.1:8123",
                self.props.clickhouse_connected,
            ))
            .child(self.render_service_status("OTel", "127.0.0.1:4317", self.props.otel_connected))
    }

    /// Render the request count indicator
    fn render_request_count(&self) -> impl IntoElement {
        let count = self.props.request_count;
        let text = if count == 1 {
            "1 request captured".to_string()
        } else {
            format!("{} requests captured", count)
        };

        div().child(text)
    }

    /// Render a service status indicator
    fn render_service_status(
        &self,
        name: &str,
        address: &str,
        connected: bool,
    ) -> impl IntoElement {
        let status_color = if connected {
            self.theme.success
        } else {
            self.theme.text_muted
        };

        div()
            .flex()
            .items_center()
            .gap(spacing::XXS)
            .child(div().size(px(6.0)).rounded(px(3.0)).bg(status_color))
            .child(format!("{}: {}", name, address))
    }

    /// Render the right section with error message
    fn render_right_section(&self) -> impl IntoElement {
        match &self.props.error_message {
            Some(error) => div()
                .text_color(self.theme.error)
                .max_w(px(400.0))
                .overflow_hidden()
                .child(truncate_error(error, 60)),
            None => div(),
        }
    }
}

/// Truncate an error message to a maximum length
fn truncate_error(error: &str, max_len: usize) -> String {
    if error.len() > max_len {
        format!("{}...", &error[..max_len - 3])
    } else {
        error.to_string()
    }
}

/// Convenience function to render a status bar
pub fn status_bar(props: StatusBarProps) -> impl IntoElement {
    StatusBar::new(props).render()
}
