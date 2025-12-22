//! Sidebar component for Roxy UI
//!
//! This component renders the left sidebar with proxy status
//! and the list of captured hosts.

use gpui::prelude::*;
use gpui::*;
use roxy_core::HostSummary;

use crate::state::ProxyStatus;
use crate::theme::{colors, dimensions, font_size, spacing, Theme};

/// Properties for the Sidebar component
#[derive(Clone)]
pub struct SidebarProps {
    /// Current proxy status
    pub proxy_status: ProxyStatus,
    /// List of hosts
    pub hosts: Vec<HostSummary>,
    /// Currently selected host (if any)
    pub selected_host: Option<String>,
}

impl Default for SidebarProps {
    fn default() -> Self {
        Self {
            proxy_status: ProxyStatus::Stopped,
            hosts: Vec::new(),
            selected_host: None,
        }
    }
}

/// Sidebar component
pub struct Sidebar {
    props: SidebarProps,
    theme: Theme,
}

impl Sidebar {
    /// Create a new sidebar with the given props
    pub fn new(props: SidebarProps) -> Self {
        Self {
            props,
            theme: Theme::dark(),
        }
    }

    /// Render the sidebar
    pub fn render(&self) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .w(dimensions::SIDEBAR_WIDTH)
            .h_full()
            .bg(rgb(colors::MANTLE))
            .border_r_1()
            .border_color(rgb(colors::SURFACE_0))
            .child(self.render_header())
            .child(self.render_hosts_section())
    }

    /// Render the header with proxy status
    fn render_header(&self) -> impl IntoElement {
        let (status_color, status_text) = self.get_status_display();

        div()
            .flex()
            .items_center()
            .justify_between()
            .p(spacing::MD)
            .border_b_1()
            .border_color(rgb(colors::SURFACE_0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(spacing::XS)
                    .child(
                        div()
                            .size(dimensions::STATUS_INDICATOR_SIZE)
                            .rounded(px(5.0))
                            .bg(status_color),
                    )
                    .child(
                        div()
                            .text_size(font_size::MD)
                            .font_weight(FontWeight::MEDIUM)
                            .child(status_text),
                    ),
            )
            .child(
                div()
                    .text_size(font_size::SM)
                    .text_color(self.theme.text_muted)
                    .child(":8080"),
            )
    }

    /// Get the status color and text based on proxy status
    fn get_status_display(&self) -> (Hsla, &'static str) {
        match &self.props.proxy_status {
            ProxyStatus::Running => (self.theme.success, "Running"),
            ProxyStatus::Starting => (self.theme.warning, "Starting..."),
            ProxyStatus::Stopped => (self.theme.text_muted, "Stopped"),
            ProxyStatus::Failed(_) => (self.theme.error, "Failed"),
        }
    }

    /// Render the hosts section
    fn render_hosts_section(&self) -> impl IntoElement {
        let hosts = &self.props.hosts;

        div()
            .flex()
            .flex_col()
            .flex_1()
            .overflow_hidden()
            .p(spacing::XS)
            .child(self.render_section_header("HOSTS"))
            .children(hosts.iter().map(|host| self.render_host_item(host)))
            .when(hosts.is_empty(), |this| {
                this.child(self.render_empty_state())
            })
    }

    /// Render a section header
    fn render_section_header(&self, title: &str) -> impl IntoElement {
        div()
            .text_size(font_size::XS)
            .text_color(self.theme.text_muted)
            .font_weight(FontWeight::SEMIBOLD)
            .px(spacing::XS)
            .py(spacing::XXS)
            .child(title.to_string())
    }

    /// Render a single host item
    fn render_host_item(&self, host: &HostSummary) -> impl IntoElement {
        let is_selected = self
            .props
            .selected_host
            .as_ref()
            .map(|h| h == &host.host)
            .unwrap_or(false);

        let bg_color = if is_selected {
            rgb(colors::SURFACE_0)
        } else {
            rgba(colors::TRANSPARENT)
        };

        div()
            .flex()
            .flex_col()
            .gap(spacing::XXS)
            .p(spacing::XS)
            .rounded(dimensions::BORDER_RADIUS)
            .bg(bg_color)
            .hover(|style| style.bg(rgb(colors::SURFACE_0)))
            .cursor_pointer()
            .child(
                div()
                    .text_size(font_size::MD)
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(self.theme.text_primary)
                    .child(host.host.clone()),
            )
            .child(
                div()
                    .flex()
                    .gap(spacing::SM)
                    .text_size(font_size::SM)
                    .text_color(self.theme.text_muted)
                    .child(format!("{} reqs", host.request_count))
                    .child(format!("{:.1}ms avg", host.avg_duration_ms)),
            )
    }

    /// Render the empty state when no hosts are available
    fn render_empty_state(&self) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .justify_center()
            .h(px(100.0))
            .text_size(font_size::SM)
            .text_color(self.theme.text_muted)
            .child("No hosts yet")
    }
}

/// Convenience function to render a sidebar
pub fn sidebar(props: SidebarProps) -> impl IntoElement {
    Sidebar::new(props).render()
}
