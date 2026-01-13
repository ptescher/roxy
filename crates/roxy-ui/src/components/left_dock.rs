//! Left Dock component for Roxy UI
//!
//! This component renders the left sidebar with tabs for Hosts, Services,
//! and Kubernetes navigation.

use gpui::prelude::*;
use gpui::*;
use roxy_core::HostSummary;
use std::sync::Arc;

use crate::state::ProxyStatus;
use crate::theme::{colors, dimensions, font_size, spacing, Theme};

/// The active tab in the left dock
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LeftDockTab {
    /// Show hosts list (current sidebar content)
    #[default]
    Hosts,
    /// Show RUM views and their connections to services
    Views,
    /// Show connected services/apps
    Services,
    /// Show Kubernetes contexts and namespaces
    Kubernetes,
}

impl LeftDockTab {
    pub fn label(&self) -> &'static str {
        match self {
            LeftDockTab::Hosts => "Hosts",
            LeftDockTab::Views => "Views",
            LeftDockTab::Services => "Services",
            LeftDockTab::Kubernetes => "K8s",
        }
    }
}

/// Summary of a connected service/app
#[derive(Debug, Clone)]
pub struct ServiceSummary {
    /// Service/app name (e.g., "frontend", "api-gateway")
    pub name: String,
    /// Number of HTTP requests from this service
    pub request_count: u64,
    /// Number of active connections
    pub connection_count: u64,
    /// Average response time in ms
    pub avg_duration_ms: f64,
}

/// Kubernetes context information
#[derive(Debug, Clone)]
pub struct KubeContext {
    /// Context name
    pub name: String,
    /// Whether this is the current context
    pub is_current: bool,
    /// Cluster name
    pub cluster: String,
}

/// Kubernetes namespace
#[derive(Debug, Clone)]
pub struct KubeNamespace {
    /// Namespace name
    pub name: String,
    /// Number of services in this namespace
    pub service_count: u32,
    /// Number of pods in this namespace
    pub pod_count: u32,
}

/// Callback type for host selection
pub type OnHostSelect = Arc<dyn Fn(&str, &mut App) + Send + Sync + 'static>;
/// Callback type for service selection
pub type OnServiceSelect = Arc<dyn Fn(&str, &mut App) + Send + Sync + 'static>;
/// Callback type for namespace selection
pub type OnNamespaceSelect = Arc<dyn Fn(Option<&str>, &mut App) + Send + Sync + 'static>;
/// Callback type for context selection
pub type OnContextSelect = Arc<dyn Fn(&str, &mut App) + Send + Sync + 'static>;
/// Callback type for tab change
pub type OnTabChange = Arc<dyn Fn(LeftDockTab, &mut App) + Send + Sync + 'static>;
/// Callback type for toggling context dropdown
pub type OnToggleContextDropdown = Arc<dyn Fn(&mut App) + Send + Sync + 'static>;

/// Properties for the LeftDock component
#[derive(Clone)]
pub struct LeftDockProps {
    /// Current active tab
    pub active_tab: LeftDockTab,
    /// Current proxy status
    pub proxy_status: ProxyStatus,
    /// List of hosts
    pub hosts: Vec<HostSummary>,
    /// Currently selected host (if any)
    pub selected_host: Option<String>,
    /// List of RUM views
    pub rum_views: Vec<roxy_core::DatadogRumViewRecord>,
    /// Currently selected RUM view (None = "All Views")
    pub selected_rum_view: Option<String>,
    /// List of RUM resources (API calls)
    pub rum_resources: Vec<roxy_core::DatadogRumResourceRecord>,
    /// Scroll handle for RUM views list
    pub rum_views_scroll_handle: ScrollHandle,
    /// List of connected services
    pub services: Vec<ServiceSummary>,
    /// Currently selected service (if any)
    pub selected_service: Option<String>,
    /// Available Kubernetes contexts
    pub kube_contexts: Vec<KubeContext>,
    /// Currently selected Kubernetes context
    pub selected_context: Option<String>,
    /// Kubernetes namespaces for the selected context
    pub kube_namespaces: Vec<KubeNamespace>,
    /// Currently selected namespace (None = all namespaces)
    pub selected_namespace: Option<String>,
    /// Callback when a host is clicked
    pub on_host_select: Option<OnHostSelect>,
    /// Callback when "All Hosts" is clicked to clear filter
    pub on_clear_host_filter: Option<Arc<dyn Fn(&mut App) + Send + Sync + 'static>>,
    /// Callback when a view is clicked
    pub on_view_select: Option<Arc<dyn Fn(String, &mut App) + Send + Sync + 'static>>,
    /// Callback when "All Views" is clicked to clear filter
    pub on_clear_view_filter: Option<Arc<dyn Fn(&mut App) + Send + Sync + 'static>>,
    /// Callback when a service is clicked
    pub on_service_select: Option<OnServiceSelect>,
    /// Callback when "All Services" is clicked to clear filter
    pub on_clear_service_filter: Option<Arc<dyn Fn(&mut App) + Send + Sync + 'static>>,
    /// Callback when a namespace is selected
    pub on_namespace_select: Option<OnNamespaceSelect>,
    /// Callback when a context is selected
    pub on_context_select: Option<OnContextSelect>,
    /// Callback when tab is changed
    pub on_tab_change: Option<OnTabChange>,
    /// Whether the context dropdown is expanded
    pub context_dropdown_expanded: bool,
    /// Callback to toggle context dropdown
    pub on_toggle_context_dropdown: Option<OnToggleContextDropdown>,
    /// Width of the dock in pixels
    pub width: f32,
    /// Scroll handle for the hosts list
    pub hosts_scroll_handle: ScrollHandle,
    /// Scroll handle for the services list
    pub services_scroll_handle: ScrollHandle,
    /// Scroll handle for the kubernetes list
    pub kubernetes_scroll_handle: ScrollHandle,
}

impl Default for LeftDockProps {
    fn default() -> Self {
        Self {
            active_tab: LeftDockTab::Hosts,
            proxy_status: ProxyStatus::Stopped,
            hosts: Vec::new(),
            selected_host: None,
            rum_views: Vec::new(),
            selected_rum_view: None,
            rum_resources: Vec::new(),
            rum_views_scroll_handle: ScrollHandle::new(),
            services: Vec::new(),
            selected_service: None,
            kube_contexts: Vec::new(),
            selected_context: None,
            kube_namespaces: Vec::new(),
            selected_namespace: None,
            on_host_select: None,
            on_clear_host_filter: None,
            on_view_select: None,
            on_clear_view_filter: None,
            on_service_select: None,
            on_clear_service_filter: None,
            on_namespace_select: None,
            on_context_select: None,
            on_tab_change: None,
            context_dropdown_expanded: false,
            on_toggle_context_dropdown: None,
            width: 220.0,
            hosts_scroll_handle: ScrollHandle::new(),
            services_scroll_handle: ScrollHandle::new(),
            kubernetes_scroll_handle: ScrollHandle::new(),
        }
    }
}

/// Left Dock component with tabs
pub struct LeftDock {
    props: LeftDockProps,
    theme: Theme,
}

impl LeftDock {
    /// Create a new left dock with the given props
    pub fn new(props: LeftDockProps) -> Self {
        Self {
            props,
            theme: Theme::dark(),
        }
    }

    /// Render the left dock
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
            .child(self.render_tabs())
            .child(self.render_tab_content())
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

    /// Render the tab bar
    fn render_tabs(&self) -> impl IntoElement {
        let tabs = [
            LeftDockTab::Hosts,
            LeftDockTab::Views,
            LeftDockTab::Services,
            LeftDockTab::Kubernetes,
        ];
        let on_tab_change = self.props.on_tab_change.clone();
        let active_tab = self.props.active_tab;

        div()
            .flex()
            .flex_row()
            .border_b_1()
            .border_color(rgb(colors::SURFACE_0))
            .children(tabs.into_iter().map(move |tab| {
                let is_active = tab == active_tab;
                let callback = on_tab_change.clone();

                let mut el = div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .py(spacing::XS)
                    .text_size(font_size::SM)
                    .cursor_pointer()
                    .when(is_active, |this| {
                        this.border_b_2()
                            .border_color(rgb(colors::BLUE))
                            .text_color(rgb(colors::TEXT))
                            .font_weight(FontWeight::MEDIUM)
                    })
                    .when(!is_active, |this| {
                        this.text_color(rgb(colors::SUBTEXT_0))
                            .hover(|style| style.text_color(rgb(colors::TEXT)))
                    })
                    .child(tab.label());

                if let Some(callback) = callback {
                    el = el.on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                        callback(tab, cx);
                    });
                }

                el
            }))
    }

    /// Render the content for the active tab
    fn render_tab_content(&self) -> impl IntoElement {
        match self.props.active_tab {
            LeftDockTab::Hosts => self.render_hosts_tab().into_any_element(),
            LeftDockTab::Views => self.render_views_tab().into_any_element(),
            LeftDockTab::Services => self.render_services_tab().into_any_element(),
            LeftDockTab::Kubernetes => self.render_kubernetes_tab().into_any_element(),
        }
    }

    // =========================================================================
    // Hosts Tab
    // =========================================================================

    /// Render the hosts tab content
    fn render_hosts_tab(&self) -> impl IntoElement {
        let hosts = self.props.hosts.clone();
        let on_clear_filter = self.props.on_clear_host_filter.clone();
        let selected_host = self.props.selected_host.clone();
        let on_host_select = self.props.on_host_select.clone();
        let scroll_handle = self.props.hosts_scroll_handle.clone();

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
                this.child(self.render_empty_state("No hosts yet"))
            })
            .id("left-dock-hosts")
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
            .is_some_and(|h| h == &host.host);

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

    // =========================================================================
    // Views Tab
    // =========================================================================

    /// Render the views tab content
    fn render_views_tab(&self) -> impl IntoElement {
        let rum_views = self.props.rum_views.clone();
        let selected_view = self.props.selected_rum_view.clone();
        let on_view_select = self.props.on_view_select.clone();
        let on_clear_view_filter = self.props.on_clear_view_filter.clone();
        let scroll_handle = self.props.rum_views_scroll_handle.clone();

        // Get unique view names with counts
        let mut view_counts: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        for view in &rum_views {
            *view_counts.entry(view.view_name.clone()).or_insert(0) += 1;
        }

        let mut views: Vec<_> = view_counts.into_iter().collect();
        // Stable sort: first by count descending, then by name ascending for ties
        views.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

        div()
            .flex()
            .flex_col()
            .flex_1()
            .p(spacing::XS)
            .child(self.render_section_header("VIEWS"))
            .child(self.render_all_views_item(
                on_clear_view_filter,
                selected_view.is_none(),
                rum_views.len(),
            ))
            .children(views.iter().map(|(view_name, count)| {
                self.render_view_item(view_name, *count, on_view_select.clone())
            }))
            .when(rum_views.is_empty(), |this| {
                this.child(self.render_empty_state("No RUM views yet"))
            })
            .id("left-dock-views")
            .overflow_y_scroll()
            .track_scroll(&scroll_handle)
    }

    /// Render the "All Views" item to clear the view filter
    fn render_all_views_item(
        &self,
        on_clear_filter: Option<Arc<dyn Fn(&mut App) + Send + Sync + 'static>>,
        is_selected: bool,
        total_count: usize,
    ) -> impl IntoElement {
        let bg_color = if is_selected {
            rgb(colors::SURFACE_0)
        } else {
            rgb(colors::MANTLE)
        };

        let mut el = div()
            .px(spacing::SM)
            .py(spacing::XS)
            .mx(spacing::XS)
            .my(spacing::XXS)
            .rounded(dimensions::BORDER_RADIUS)
            .bg(bg_color)
            .hover(|style| style.bg(rgb(colors::SURFACE_0)))
            .cursor_pointer()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(font_size::SM)
                            .text_color(rgb(colors::TEXT))
                            .child("All Views"),
                    )
                    .child(
                        div()
                            .text_size(font_size::XS)
                            .text_color(rgb(colors::SUBTEXT_0))
                            .child(total_count.to_string()),
                    ),
            );

        if let Some(callback) = on_clear_filter {
            el = el.on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                callback(cx);
            });
        }

        el
    }

    /// Render a single view item
    fn render_view_item(
        &self,
        view_name: &str,
        count: usize,
        on_view_select: Option<Arc<dyn Fn(String, &mut App) + Send + Sync + 'static>>,
    ) -> impl IntoElement {
        let is_selected = self.props.selected_rum_view.as_ref() == Some(&view_name.to_string());
        let bg_color = if is_selected {
            rgb(colors::SURFACE_0)
        } else {
            rgb(colors::MANTLE)
        };

        let view_name_owned = view_name.to_string();
        let view_name_for_click = view_name_owned.clone();

        let mut el = div()
            .px(spacing::SM)
            .py(spacing::XS)
            .mx(spacing::XS)
            .my(spacing::XXS)
            .rounded(dimensions::BORDER_RADIUS)
            .bg(bg_color)
            .hover(|style| style.bg(rgb(colors::SURFACE_0)))
            .cursor_pointer()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(spacing::XXS)
                    .child(
                        div()
                            .text_size(font_size::SM)
                            .text_color(rgb(colors::TEXT))
                            .child(view_name_owned),
                    )
                    .child(
                        div()
                            .text_size(font_size::XS)
                            .text_color(rgb(colors::SUBTEXT_0))
                            .child(format!("{} sessions", count)),
                    ),
            );

        if let Some(callback) = on_view_select {
            el = el.on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                callback(view_name_for_click.clone(), cx);
            });
        }

        el
    }

    // =========================================================================
    // Services Tab
    // =========================================================================

    /// Render the services tab content
    fn render_services_tab(&self) -> impl IntoElement {
        let services = self.props.services.clone();
        let on_clear_filter = self.props.on_clear_service_filter.clone();
        let selected_service = self.props.selected_service.clone();
        let on_service_select = self.props.on_service_select.clone();
        let scroll_handle = self.props.services_scroll_handle.clone();

        div()
            .flex()
            .flex_col()
            .flex_1()
            .p(spacing::XS)
            .child(self.render_section_header("CONNECTED SERVICES"))
            .child(self.render_all_services_item(on_clear_filter, selected_service.is_none()))
            .children(
                services
                    .iter()
                    .map(|service| self.render_service_item(service, on_service_select.clone())),
            )
            .when(services.is_empty(), |this| {
                this.child(self.render_empty_state("No services connected"))
            })
            .id("left-dock-services")
            .overflow_y_scroll()
            .track_scroll(&scroll_handle)
    }

    /// Render the "All Services" item to clear the filter
    fn render_all_services_item(
        &self,
        on_clear_filter: Option<Arc<dyn Fn(&mut App) + Send + Sync + 'static>>,
        is_selected: bool,
    ) -> impl IntoElement {
        let bg_color = if is_selected {
            rgb(colors::SURFACE_0)
        } else {
            rgba(colors::TRANSPARENT)
        };

        let total_requests: u64 = self.props.services.iter().map(|s| s.request_count).sum();
        let total_connections: u64 = self.props.services.iter().map(|s| s.connection_count).sum();

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
                    .child("All Services"),
            )
            .child(
                div()
                    .flex()
                    .gap(spacing::SM)
                    .text_size(font_size::SM)
                    .text_color(self.theme.text_muted)
                    .child(format!("{} reqs", total_requests))
                    .child(format!("{} conns", total_connections)),
            );

        if let Some(callback) = on_clear_filter {
            el = el.on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                callback(cx);
            });
        }

        el
    }

    /// Render a single service item
    fn render_service_item(
        &self,
        service: &ServiceSummary,
        on_select: Option<OnServiceSelect>,
    ) -> impl IntoElement {
        let is_selected = self
            .props
            .selected_service
            .as_ref()
            .is_some_and(|s| s == &service.name);

        let bg_color = if is_selected {
            rgb(colors::SURFACE_0)
        } else {
            rgba(colors::TRANSPARENT)
        };

        let service_name = service.name.clone();
        let request_count = service.request_count;
        let connection_count = service.connection_count;
        let avg_duration = service.avg_duration_ms;

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
                    .flex()
                    .items_center()
                    .gap(spacing::XS)
                    .child(
                        // Service icon/indicator
                        div().size(px(8.0)).rounded(px(4.0)).bg(self.theme.info),
                    )
                    .child(
                        div()
                            .text_size(font_size::MD)
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(self.theme.text_primary)
                            .child(service_name.clone()),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap(spacing::SM)
                    .text_size(font_size::SM)
                    .text_color(self.theme.text_muted)
                    .child(format!("{} reqs", request_count))
                    .child(format!("{} conns", connection_count))
                    .child(format!("{:.1}ms", avg_duration)),
            );

        if let Some(callback) = on_select {
            let name_for_callback = service_name.clone();
            el = el.on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                callback(&name_for_callback, cx);
            });
        }

        el
    }

    // =========================================================================
    // Kubernetes Tab
    // =========================================================================

    /// Render the kubernetes tab content
    fn render_kubernetes_tab(&self) -> impl IntoElement {
        let scroll_handle = self.props.kubernetes_scroll_handle.clone();

        div()
            .flex()
            .flex_col()
            .flex_1()
            .p(spacing::XS)
            .child(self.render_context_selector())
            .when(self.props.selected_context.is_some(), |this| {
                this.child(self.render_namespace_list())
            })
            .when(self.props.selected_context.is_none(), |this| {
                this.child(self.render_empty_state("Select a context"))
            })
            .id("left-dock-kubernetes")
            .overflow_y_scroll()
            .track_scroll(&scroll_handle)
    }

    /// Render the context selector dropdown
    fn render_context_selector(&self) -> impl IntoElement {
        let contexts = self.props.kube_contexts.clone();
        let selected = self.props.selected_context.clone();
        let on_context_select = self.props.on_context_select.clone();
        let on_toggle = self.props.on_toggle_context_dropdown.clone();
        let is_expanded = self.props.context_dropdown_expanded;

        // Find the selected context to show its details
        let selected_ctx = selected
            .as_ref()
            .and_then(|name| contexts.iter().find(|c| &c.name == name));

        let display_name = selected_ctx
            .map(|c| c.name.as_str())
            .unwrap_or("Select context...");

        let cluster_name = selected_ctx.map(|c| c.cluster.as_str()).unwrap_or("");

        let is_current = selected_ctx.map(|c| c.is_current).unwrap_or(false);

        let chevron = if is_expanded { "▲" } else { "▼" };

        let mut header = div()
            .flex()
            .flex_col()
            .gap(spacing::XXS)
            .p(spacing::XS)
            .rounded(dimensions::BORDER_RADIUS)
            .bg(rgb(colors::SURFACE_0))
            .cursor_pointer()
            .hover(|style| style.bg(rgb(colors::SURFACE_1)))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(spacing::XS)
                            .child(
                                // Current context indicator
                                div()
                                    .size(px(8.0))
                                    .rounded(px(4.0))
                                    .when(is_current, |this| this.bg(self.theme.success))
                                    .when(!is_current && selected.is_some(), |this| {
                                        this.bg(self.theme.text_muted)
                                    })
                                    .when(selected.is_none(), |this| {
                                        this.bg(rgba(colors::TRANSPARENT))
                                    }),
                            )
                            .child(
                                div()
                                    .text_size(font_size::MD)
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(self.theme.text_primary)
                                    .child(display_name.to_string()),
                            ),
                    )
                    .child(
                        div()
                            .text_size(font_size::SM)
                            .text_color(self.theme.text_muted)
                            .child(chevron),
                    ),
            )
            .when(!cluster_name.is_empty(), |this| {
                this.child(
                    div()
                        .text_size(font_size::XS)
                        .text_color(self.theme.text_muted)
                        .pl(px(16.0)) // Indent to align with name
                        .child(cluster_name.to_string()),
                )
            });

        if let Some(toggle_callback) = on_toggle.clone() {
            header = header.on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                toggle_callback(cx);
            });
        }

        div()
            .flex()
            .flex_col()
            .gap(spacing::XS)
            .mb(spacing::SM)
            .child(self.render_section_header("CONTEXT"))
            .child(header)
            // Show available contexts only when expanded
            .when(is_expanded && !contexts.is_empty(), |this| {
                this.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(spacing::XXS)
                        .mt(spacing::XXS)
                        .children(contexts.iter().map(|ctx| {
                            let is_selected = selected.as_ref().is_some_and(|s| s == &ctx.name);
                            let callback = on_context_select.clone();
                            let toggle_callback = on_toggle.clone();
                            let context_name = ctx.name.clone();

                            let mut el = div()
                                .flex()
                                .items_center()
                                .gap(spacing::XS)
                                .p(spacing::XS)
                                .rounded(dimensions::BORDER_RADIUS)
                                .cursor_pointer()
                                .hover(|style| style.bg(rgb(colors::SURFACE_0)))
                                .when(is_selected, |this| this.bg(rgb(colors::SURFACE_0)))
                                .child(
                                    div()
                                        .size(px(8.0))
                                        .rounded(px(4.0))
                                        .when(ctx.is_current, |this| this.bg(self.theme.success))
                                        .when(!ctx.is_current, |this| {
                                            this.bg(self.theme.text_muted)
                                        }),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .child(
                                            div()
                                                .text_size(font_size::SM)
                                                .text_color(self.theme.text_primary)
                                                .child(ctx.name.clone()),
                                        )
                                        .child(
                                            div()
                                                .text_size(font_size::XS)
                                                .text_color(self.theme.text_muted)
                                                .child(ctx.cluster.clone()),
                                        ),
                                );

                            if let Some(callback) = callback {
                                let name = context_name.clone();
                                let toggle = toggle_callback.clone();
                                el = el.on_mouse_down(
                                    MouseButton::Left,
                                    move |_event, _window, cx| {
                                        callback(&name, cx);
                                        // Collapse dropdown after selection
                                        if let Some(ref toggle) = toggle {
                                            toggle(cx);
                                        }
                                    },
                                );
                            }

                            el
                        })),
                )
            })
    }

    /// Render the namespace list
    fn render_namespace_list(&self) -> impl IntoElement {
        let namespaces = self.props.kube_namespaces.clone();
        let selected = self.props.selected_namespace.clone();
        let on_namespace_select = self.props.on_namespace_select.clone();

        div()
            .flex()
            .flex_col()
            .gap(spacing::XXS)
            .child(self.render_section_header("NAMESPACES"))
            .child(self.render_all_namespaces_item(on_namespace_select.clone(), selected.is_none()))
            .children(
                namespaces
                    .iter()
                    .map(|ns| self.render_namespace_item(ns, on_namespace_select.clone())),
            )
            .when(namespaces.is_empty(), |this| {
                this.child(self.render_empty_state("No namespaces found"))
            })
    }

    /// Render the "All Namespaces" item
    fn render_all_namespaces_item(
        &self,
        on_select: Option<OnNamespaceSelect>,
        is_selected: bool,
    ) -> impl IntoElement {
        let bg_color = if is_selected {
            rgb(colors::SURFACE_0)
        } else {
            rgba(colors::TRANSPARENT)
        };

        let total_services: u32 = self
            .props
            .kube_namespaces
            .iter()
            .map(|ns| ns.service_count)
            .sum();
        let total_pods: u32 = self
            .props
            .kube_namespaces
            .iter()
            .map(|ns| ns.pod_count)
            .sum();

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
                div().flex().items_center().gap(spacing::XS).child(
                    div()
                        .text_size(font_size::MD)
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(self.theme.text_primary)
                        .child("All Namespaces"),
                ),
            )
            .child(
                div()
                    .flex()
                    .gap(spacing::SM)
                    .text_size(font_size::SM)
                    .text_color(self.theme.text_muted)
                    .child(format!("{} services", total_services))
                    .child(format!("{} pods", total_pods)),
            );

        if let Some(callback) = on_select {
            el = el.on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                callback(None, cx);
            });
        }

        el
    }

    /// Render a single namespace item
    fn render_namespace_item(
        &self,
        namespace: &KubeNamespace,
        on_select: Option<OnNamespaceSelect>,
    ) -> impl IntoElement {
        let is_selected = self
            .props
            .selected_namespace
            .as_ref()
            .is_some_and(|s| s == &namespace.name);

        let bg_color = if is_selected {
            rgb(colors::SURFACE_0)
        } else {
            rgba(colors::TRANSPARENT)
        };

        let ns_name = namespace.name.clone();
        let service_count = namespace.service_count;
        let pod_count = namespace.pod_count;

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
                    .flex()
                    .items_center()
                    .gap(spacing::XS)
                    .child(
                        // Namespace icon
                        div().size(px(8.0)).rounded(px(2.0)).bg(self.theme.info),
                    )
                    .child(
                        div()
                            .text_size(font_size::MD)
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(self.theme.text_primary)
                            .child(ns_name.clone()),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap(spacing::SM)
                    .text_size(font_size::SM)
                    .text_color(self.theme.text_muted)
                    .child(format!("{} svcs", service_count))
                    .child(format!("{} pods", pod_count)),
            );

        if let Some(callback) = on_select {
            let name_for_callback = ns_name.clone();
            el = el.on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                callback(Some(&name_for_callback), cx);
            });
        }

        el
    }

    /// Render the empty state message
    fn render_empty_state(&self, message: &str) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .justify_center()
            .h(px(100.0))
            .text_size(font_size::SM)
            .text_color(self.theme.text_muted)
            .child(message.to_string())
    }
}

/// Convenience function to render a left dock
pub fn left_dock(props: LeftDockProps) -> impl IntoElement {
    LeftDock::new(props).render()
}
