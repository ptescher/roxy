//! Sidebar component for Roxy UI
//!
//! This component renders the left sidebar with proxy status
//! and the list of captured hosts.

use gpui::prelude::*;
use gpui::*;
use roxy_core::HostSummary;
use std::sync::Arc;

use crate::state::ProxyStatus;
use crate::theme::{colors, dimensions, font_size, spacing, Theme};

/// Callback type for host selection
pub type OnHostSelect = Arc<dyn Fn(&str, &mut App) + Send + Sync + 'static>;

/// Properties for the Sidebar component
#[derive(Clone)]
pub struct SidebarProps {
    /// Current proxy status
    pub proxy_status: ProxyStatus,
    /// List of hosts
    pub hosts: Vec<HostSummary>,
    /// Currently selected host (if any)
    pub selected_host: Option<String>,
    /// Callback when a host is clicked
    pub on_host_select: Option<OnHostSelect>,
    /// Callback when "All Hosts" is clicked to clear filter
    pub on_clear_filter: Option<Arc<dyn Fn(&mut App) + Send + Sync + 'static>>,
    /// Width of the sidebar in pixels
    pub width: f32,
    /// Scroll handle for the hosts list
    pub scroll_handle: ScrollHandle,
}

impl Default for SidebarProps {
    fn default() -> Self {
        Self {
            proxy_status: ProxyStatus::Stopped,
            hosts: Vec::new(),
            selected_host: None,
            on_host_select: None,
            on_clear_filter: None,
            width: 220.0,
            scroll_handle: ScrollHandle::new(),
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
            .w(px(self.props.width))
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
        let hosts = self.props.hosts.clone();
        let on_clear_filter = self.props.on_clear_filter.clone();
        let selected_host = self.props.selected_host.clone();
        let on_host_select = self.props.on_host_select.clone();
        let scroll_handle = self.props.scroll_handle.clone();

        div()
            .flex()
            .flex_col()
            .flex_1()
            .p(spacing::XS)
            .child(self.render_section_header("HOSTS"))
            .child(self.render_all_hosts_item(on_clear_filter, selected_host.is_none()))
            .children(
                hosts
                    .iter()
                    .map(|host| self.render_host_item(host, on_host_select.clone())),
            )
            .when(hosts.is_empty(), |this| {
                this.child(self.render_empty_state())
            })
            .id("sidebar-hosts")
            .overflow_y_scroll()
            .track_scroll(&scroll_handle)
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

    /// Render the "All Hosts" item to clear the filter
    fn render_all_hosts_item(
        &self,
        on_clear_filter: Option<Arc<dyn Fn(&mut App) + Send + Sync + 'static>>,
        is_selected: bool,
    ) -> impl IntoElement {
        let bg_color = if is_selected {
            rgb(colors::SURFACE_0)
        } else {
            rgba(colors::TRANSPARENT)
        };

        let total_requests: u64 = self.props.hosts.iter().map(|h| h.request_count).sum();

        let mut el = div()
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
                    .child("All Hosts"),
            )
            .child(
                div()
                    .flex()
                    .gap(spacing::SM)
                    .text_size(font_size::SM)
                    .text_color(self.theme.text_muted)
                    .child(format!("{} reqs total", total_requests)),
            );

        if let Some(callback) = on_clear_filter {
            el = el.on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                callback(cx);
            });
        }

        el
    }

    /// Render a single host item
    fn render_host_item(
        &self,
        host: &HostSummary,
        on_select: Option<OnHostSelect>,
    ) -> impl IntoElement {
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

        let host_name = host.host.clone();
        let request_count = host.request_count;
        let avg_duration = host.avg_duration_ms;

        let mut el = div()
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
                    .child(host_name.clone()),
            )
            .child(
                div()
                    .flex()
                    .gap(spacing::SM)
                    .text_size(font_size::SM)
                    .text_color(self.theme.text_muted)
                    .child(format!("{} reqs", request_count))
                    .child(format!("{:.1}ms avg", avg_duration)),
            );

        if let Some(callback) = on_select {
            let host_for_callback = host_name.clone();
            el = el.on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                callback(&host_for_callback, cx);
            });
        }

        el
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
