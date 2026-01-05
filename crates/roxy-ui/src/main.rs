//! Roxy UI - Native network debugging interface built with GPUI
//!
//! This is the main entry point for the Roxy desktop application.
//! It provides a GUI for viewing and analyzing network traffic captured
//! by the proxy worker, which it manages as a subprocess.

mod components;
mod state;
mod theme;

use gpui::prelude::*;
use gpui::{KeyBinding, *};
use roxy_core::{ClickHouseConfig, RoxyClickHouse, CURRENT_VERSION};

#[cfg(target_os = "macos")]
use cocoa::{
    appkit::NSImage,
    base::{id, nil},
    foundation::NSData,
};
#[cfg(target_os = "macos")]
use objc::{class, msg_send, sel, sel_impl};

use roxy_proxy::{system_proxy, ProxyConfig, ProxyServer};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use components::{
    connection_detail_panel, connection_list, open_about_window, ConnectionDetailPanelProps,
    ConnectionDetailTab, ConnectionListProps, DetailPanel, DetailPanelProps, DetailTab,
    KubernetesPanel, KubernetesPanelProps, LeftDock, LeftDockProps, LeftDockTab, RequestList,
    RequestListProps, StatusBar, StatusBarProps, TitleBar, TitleBarProps,
};
use state::{AppState, ProxyStatus, UiMessage, ViewMode};
use theme::{colors, dimensions, font_size, spacing};

// Define actions for the menu bar
actions!(
    roxy,
    [
        Quit,
        About,
        ClearRequests,
        ToggleProxy,
        ShowHelp,
        Minimize,
        Zoom,
        CloseWindow
    ]
);

/// Main application view
struct RoxyApp {
    state: AppState,
}

impl RoxyApp {
    fn new(cx: &mut Context<Self>) -> Self {
        let mut state = AppState::new();
        let proxy_running = state.proxy_running.clone();
        let message_tx = state.message_sender();

        // Set status to Starting synchronously
        state.proxy_status = ProxyStatus::Starting;

        // Set up periodic UI refresh to process messages
        cx.spawn(async |this: WeakEntity<Self>, cx: &mut AsyncApp| loop {
            cx.background_executor()
                .timer(Duration::from_millis(500))
                .await;

            let should_continue = this
                .update(cx, |app, cx| {
                    app.state.process_messages();
                    cx.notify();
                    true
                })
                .unwrap_or(false);

            if !should_continue {
                break;
            }
        })
        .detach();

        tracing::info!("Starting embedded proxy server...");

        // Start the proxy in a background thread with its own tokio runtime
        let proxy_tx = message_tx.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
            rt.block_on(async move {
                let config = ProxyConfig {
                    start_services: true,
                    ..Default::default()
                };

                match ProxyServer::new(config).await {
                    Ok(mut server) => {
                        if let Err(e) = server.setup_services().await {
                            tracing::error!("Failed to setup services: {}", e);
                            let _ = proxy_tx.send(UiMessage::ProxyFailed(e.to_string()));
                            return;
                        }

                        proxy_running.store(true, Ordering::SeqCst);
                        tracing::info!("Proxy server started successfully");
                        let _ = proxy_tx.send(UiMessage::ProxyStarted);

                        if let Err(e) = server.run().await {
                            tracing::error!("Proxy server error: {}", e);
                        }

                        proxy_running.store(false, Ordering::SeqCst);

                        if let Err(e) = server.stop_services().await {
                            tracing::error!("Failed to stop services: {}", e);
                        }
                    }
                    Err(e) => {
                        tracing::error!("Failed to create proxy server: {}", e);
                        let _ = proxy_tx.send(UiMessage::ProxyFailed(e.to_string()));
                    }
                }
            });
        });

        // Start background polling for ClickHouse data
        let poll_tx = message_tx.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().expect("Failed to create polling runtime");
            rt.block_on(async move {
                let clickhouse = RoxyClickHouse::new(ClickHouseConfig::default());

                // Wait for services to start
                tracing::info!("[POLLER] ClickHouse poller waiting for services to start...");
                tokio::time::sleep(Duration::from_secs(5)).await;
                tracing::info!("[POLLER] Starting polling loop");

                let mut poll_count = 0u64;
                loop {
                    poll_count += 1;

                    // Poll for recent requests
                    tracing::debug!("[POLLER] Poll #{}: Querying requests...", poll_count);
                    match clickhouse.get_recent_requests(100).await {
                        Ok(requests) => {
                            tracing::info!(
                                "[POLLER] Poll #{}: SUCCESS - Got {} requests from ClickHouse",
                                poll_count,
                                requests.len()
                            );
                            for (i, req) in requests.iter().take(3).enumerate() {
                                tracing::debug!(
                                    "[POLLER]   Request {}: {} {} {}",
                                    i,
                                    req.method,
                                    req.url,
                                    req.response_status
                                );
                            }
                            if let Err(e) = poll_tx.send(UiMessage::RequestsUpdated(requests)) {
                                tracing::error!("[POLLER] Failed to send requests to UI: {}", e);
                            }
                        }
                        Err(e) => {
                            tracing::error!(
                                "[POLLER] Poll #{}: FAILED to fetch requests: {:?}",
                                poll_count,
                                e
                            );
                        }
                    }

                    // Poll for hosts
                    tracing::debug!("[POLLER] Poll #{}: Querying hosts...", poll_count);
                    match clickhouse.get_hosts().await {
                        Ok(hosts) => {
                            tracing::info!(
                                "[POLLER] Poll #{}: SUCCESS - Got {} hosts from ClickHouse",
                                poll_count,
                                hosts.len()
                            );
                            if let Err(e) = poll_tx.send(UiMessage::HostsUpdated(hosts)) {
                                tracing::error!("[POLLER] Failed to send hosts to UI: {}", e);
                            }
                        }
                        Err(e) => {
                            tracing::error!(
                                "[POLLER] Poll #{}: FAILED to fetch hosts: {:?}",
                                poll_count,
                                e
                            );
                        }
                    }

                    // Poll for TCP connections (PostgreSQL, Kafka, etc.)
                    tracing::debug!("[POLLER] Poll #{}: Querying TCP connections...", poll_count);
                    match clickhouse.get_recent_connections(100).await {
                        Ok(connections) => {
                            tracing::info!(
                                "[POLLER] Poll #{}: SUCCESS - Got {} TCP connections from ClickHouse",
                                poll_count,
                                connections.len()
                            );
                            for (i, conn) in connections.iter().take(3).enumerate() {
                                tracing::debug!(
                                    "[POLLER]   Connection {}: {} {} -> {} ({})",
                                    i,
                                    conn.protocol,
                                    conn.target_host,
                                    conn.server_address,
                                    conn.status
                                );
                            }
                            if let Err(e) = poll_tx.send(UiMessage::ConnectionsUpdated(connections)) {
                                tracing::error!("[POLLER] Failed to send connections to UI: {}", e);
                            }
                        }
                        Err(e) => {
                            tracing::error!(
                                "[POLLER] Poll #{}: FAILED to fetch TCP connections: {:?}",
                                poll_count,
                                e
                            );
                        }
                    }

                    // Poll every second
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            });
        });

        Self { state }
    }

    /// Check and update proxy status - called during render
    fn update_proxy_status(&mut self, cx: &mut Context<Self>) {
        let is_running = self.state.proxy_running.load(Ordering::SeqCst);
        let new_status = if is_running {
            ProxyStatus::Running
        } else if matches!(self.state.proxy_status, ProxyStatus::Starting) {
            // Still starting, don't change yet
            return;
        } else if matches!(self.state.proxy_status, ProxyStatus::Failed(_)) {
            // Keep failed status
            return;
        } else {
            ProxyStatus::Stopped
        };

        if self.state.proxy_status != new_status {
            self.state.proxy_status = new_status;
            cx.notify();
        }
    }

    /// Render the title bar component
    fn render_title_bar(&self) -> impl IntoElement {
        TitleBar::new(TitleBarProps {
            update_available: self.state.update_available.clone(),
        })
        .render()
    }

    /// Render the left dock component with tabs
    fn render_left_dock(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity().clone();

        // Tab change callback
        let on_tab_change: Arc<dyn Fn(LeftDockTab, &mut App) + Send + Sync + 'static> = {
            let entity = entity.clone();
            Arc::new(move |tab: LeftDockTab, cx: &mut App| {
                entity.update(cx, |app, cx| {
                    app.state.set_left_dock_tab(tab);
                    // Load K8s contexts from system kubeconfig when switching to Kubernetes tab
                    if tab == LeftDockTab::Kubernetes && app.state.kube_contexts.is_empty() {
                        app.state.load_kube_contexts();
                    }
                    cx.notify();
                });
            })
        };

        // Host selection callback - switches to Requests view
        let on_host_select: Arc<dyn Fn(&str, &mut App) + Send + Sync + 'static> = {
            let entity = entity.clone();
            Arc::new(move |host: &str, cx: &mut App| {
                let host = host.to_string();
                entity.update(cx, |app, cx| {
                    app.state.select_host(host);
                    // Switch to Requests view when a host is selected
                    app.state.set_view_mode(ViewMode::Requests);
                    cx.notify();
                });
            })
        };

        // Clear host filter callback - switches to Requests view
        let on_clear_host_filter: Arc<dyn Fn(&mut App) + Send + Sync + 'static> = {
            let entity = entity.clone();
            Arc::new(move |cx: &mut App| {
                entity.update(cx, |app, cx| {
                    app.state.clear_host_filter();
                    // Switch to Requests view
                    app.state.set_view_mode(ViewMode::Requests);
                    cx.notify();
                });
            })
        };

        // Service selection callback - switches to Requests view
        let on_service_select: Arc<dyn Fn(&str, &mut App) + Send + Sync + 'static> = {
            let entity = entity.clone();
            Arc::new(move |service: &str, cx: &mut App| {
                let service = service.to_string();
                entity.update(cx, |app, cx| {
                    app.state.select_service(service);
                    // Switch to Requests view when a service is selected
                    app.state.set_view_mode(ViewMode::Requests);
                    cx.notify();
                });
            })
        };

        // Clear service filter callback - switches to Requests view
        let on_clear_service_filter: Arc<dyn Fn(&mut App) + Send + Sync + 'static> = {
            let entity = entity.clone();
            Arc::new(move |cx: &mut App| {
                entity.update(cx, |app, cx| {
                    app.state.clear_service_filter();
                    // Switch to Requests view
                    app.state.set_view_mode(ViewMode::Requests);
                    cx.notify();
                });
            })
        };

        // Kubernetes context selection callback
        let on_context_select: Arc<dyn Fn(&str, &mut App) + Send + Sync + 'static> = {
            let entity = entity.clone();
            Arc::new(move |context: &str, cx: &mut App| {
                let context = context.to_string();
                entity.update(cx, |app, cx| {
                    app.state.select_kube_context(context);
                    cx.notify();
                });
            })
        };

        // Kubernetes namespace selection callback
        let on_namespace_select: Arc<dyn Fn(Option<&str>, &mut App) + Send + Sync + 'static> = {
            let entity = entity.clone();
            Arc::new(move |namespace: Option<&str>, cx: &mut App| {
                let namespace = namespace.map(|s| s.to_string());
                entity.update(cx, |app, cx| {
                    app.state.select_kube_namespace(namespace);
                    // Switch to Kubernetes view when a namespace is selected
                    app.state.set_view_mode(ViewMode::Kubernetes);
                    cx.notify();
                });
            })
        };

        // Toggle context dropdown callback
        let on_toggle_context_dropdown: Arc<dyn Fn(&mut App) + Send + Sync + 'static> = {
            let entity = entity.clone();
            Arc::new(move |cx: &mut App| {
                entity.update(cx, |app, cx| {
                    app.state.toggle_context_dropdown();
                    cx.notify();
                });
            })
        };

        LeftDock::new(LeftDockProps {
            active_tab: self.state.left_dock_tab,
            proxy_status: self.state.proxy_status.clone(),
            hosts: self.state.hosts.clone(),
            selected_host: self.state.selected_host.clone(),
            services: self.state.services.clone(),
            selected_service: self.state.selected_service.clone(),
            kube_contexts: self.state.kube_contexts.clone(),
            selected_context: self.state.selected_kube_context.clone(),
            kube_namespaces: self.state.kube_namespaces.clone(),
            selected_namespace: self.state.selected_kube_namespace.clone(),
            on_host_select: Some(on_host_select),
            on_clear_host_filter: Some(on_clear_host_filter),
            on_service_select: Some(on_service_select),
            on_clear_service_filter: Some(on_clear_service_filter),
            on_namespace_select: Some(on_namespace_select),
            on_context_select: Some(on_context_select),
            on_tab_change: Some(on_tab_change),
            context_dropdown_expanded: self.state.context_dropdown_expanded,
            on_toggle_context_dropdown: Some(on_toggle_context_dropdown),
            width: self.state.sidebar_width,
            hosts_scroll_handle: self.state.sidebar_scroll_handle.clone(),
            services_scroll_handle: self.state.services_scroll_handle.clone(),
            kubernetes_scroll_handle: self.state.kube_namespaces_scroll_handle.clone(),
        })
        .render()
    }

    /// Render the left dock resize handle
    fn render_left_dock_resize_handle(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity().clone();

        div()
            .w(dimensions::RESIZE_HANDLE_WIDTH)
            .h_full()
            .cursor(gpui::CursorStyle::ResizeLeftRight)
            .bg(rgb(colors::SURFACE_0))
            .hover(|style| style.bg(rgb(colors::SURFACE_1)))
            .on_mouse_down(MouseButton::Left, {
                let entity = entity.clone();
                move |_event, _window, cx| {
                    entity.update(cx, |app, cx| {
                        app.state.start_resizing_left_dock();
                        cx.notify();
                    });
                }
            })
    }

    /// Render the detail panel resize handle
    fn render_detail_panel_resize_handle(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity().clone();

        div()
            .w_full()
            .h(dimensions::RESIZE_HANDLE_HEIGHT)
            .cursor(gpui::CursorStyle::ResizeUpDown)
            .bg(rgb(colors::SURFACE_0))
            .hover(|style| style.bg(rgb(colors::SURFACE_1)))
            .on_mouse_down(MouseButton::Left, {
                let entity = entity.clone();
                move |_event, _window, cx| {
                    entity.update(cx, |app, cx| {
                        app.state.start_resizing_detail_panel();
                        cx.notify();
                    });
                }
            })
    }

    /// Render the main content area
    fn render_main_content(&self, cx: &mut Context<Self>) -> impl IntoElement {
        // The detail panel has a fixed height from state
        let detail_height = self.state.detail_panel_height;
        let is_kubernetes_mode = self.state.view_mode == ViewMode::Kubernetes;

        div()
            .flex()
            .flex_col()
            .flex_1()
            .overflow_hidden()
            // Toolbar with view switcher - only show for HTTP/Connections mode
            .when(!is_kubernetes_mode, |this| {
                this.child(div().flex_shrink_0().child(self.render_toolbar(cx)))
            })
            // Main content area - depends on view mode
            .when(self.state.view_mode == ViewMode::Requests, |this| {
                this
                    // Request list - fills remaining space
                    .child(
                        div()
                            .flex_1()
                            .min_h(px(100.0))
                            .overflow_hidden()
                            .child(self.render_request_list(cx)),
                    )
                    // Resize handle - fixed height, no shrink
                    .child(
                        div()
                            .flex_shrink_0()
                            .child(self.render_detail_panel_resize_handle(cx)),
                    )
                    // Detail panel - fixed height from state, no shrink
                    .child(
                        div()
                            .flex_shrink_0()
                            .h(px(detail_height))
                            .child(self.render_detail_panel(cx)),
                    )
            })
            .when(self.state.view_mode == ViewMode::Connections, |this| {
                this
                    // Connection list - fills remaining space
                    .child(
                        div()
                            .flex_1()
                            .min_h(px(100.0))
                            .overflow_hidden()
                            .child(self.render_connection_list(cx)),
                    )
                    // Resize handle - fixed height, no shrink
                    .child(
                        div()
                            .flex_shrink_0()
                            .child(self.render_detail_panel_resize_handle(cx)),
                    )
                    // Connection detail panel - fixed height from state, no shrink
                    .child(
                        div()
                            .flex_shrink_0()
                            .h(px(detail_height))
                            .child(self.render_connection_detail_panel(cx)),
                    )
            })
            .when(self.state.view_mode == ViewMode::Kubernetes, |this| {
                this.child(self.render_kubernetes_panel(cx))
            })
    }

    /// Render the toolbar component with view switcher (HTTP/Connections only)
    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let _entity = cx.entity().clone();
        let view_mode = self.state.view_mode;

        div()
            .flex()
            .items_center()
            .justify_between()
            .h(dimensions::TOOLBAR_HEIGHT)
            .px(spacing::MD)
            .border_b_1()
            .border_color(rgb(colors::SURFACE_0))
            .bg(rgb(colors::BASE))
            // Left side - view mode tabs (only HTTP and Connections)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(spacing::XXS)
                    .child(self.render_view_tab(ViewMode::Requests, view_mode, cx))
                    .child(self.render_view_tab(ViewMode::Connections, view_mode, cx)),
            )
            // Right side - request count or connection count
            .child(
                div()
                    .text_size(font_size::SM)
                    .text_color(rgb(colors::SUBTEXT_0))
                    .child(match view_mode {
                        ViewMode::Requests => format!("{} requests", self.state.request_count()),
                        ViewMode::Connections => {
                            format!("{} connections", self.state.tcp_connections.len())
                        }
                        ViewMode::Kubernetes => String::new(), // Not shown in toolbar mode
                    }),
            )
    }

    /// Render a view mode tab button
    fn render_view_tab(
        &self,
        mode: ViewMode,
        current: ViewMode,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let entity = cx.entity().clone();
        let is_active = mode == current;

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

        div()
            .px(spacing::SM)
            .py(spacing::XS)
            .rounded(dimensions::BORDER_RADIUS)
            .bg(bg)
            .text_size(font_size::SM)
            .font_weight(if is_active {
                FontWeight::MEDIUM
            } else {
                FontWeight::NORMAL
            })
            .text_color(text_color)
            .cursor_pointer()
            .hover(|style| style.bg(rgb(colors::SURFACE_0)))
            .child(mode.label().to_string())
            .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                entity.update(cx, |app, cx| {
                    app.state.set_view_mode(mode);
                    cx.notify();
                });
            })
    }

    /// Render the Kubernetes overview panel
    fn render_kubernetes_panel(&self, _cx: &mut Context<Self>) -> impl IntoElement {
        let props = KubernetesPanelProps {
            gateways: self.state.k8s_gateways.clone(),
            ingresses: self.state.k8s_ingresses.clone(),
            http_routes: self.state.k8s_http_routes.clone(),
            services: self.state.k8s_services.clone(),
            port_forwards: self.state.k8s_port_forwards.clone(),
            selected_namespace: self.state.selected_kube_namespace.clone(),
            width: 800.0, // Will be overridden by flex
            height: 600.0,
            scroll_handle: self.state.kubernetes_scroll_handle.clone(),
        };

        KubernetesPanel::new(props).render()
    }

    /// Render the request list component
    fn render_request_list(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity().clone();
        let on_request_select: Arc<
            dyn Fn(&roxy_core::HttpRequestRecord, &mut App) + Send + Sync + 'static,
        > = Arc::new(
            move |request: &roxy_core::HttpRequestRecord, cx: &mut App| {
                let request = request.clone();
                entity.update(cx, |app, cx| {
                    app.state.select_request(request);
                    cx.notify();
                });
            },
        );

        // Use filtered_requests when a host is selected
        let requests = if self.state.selected_host.is_some() {
            self.state
                .filtered_requests()
                .into_iter()
                .cloned()
                .collect()
        } else {
            self.state.requests.clone()
        };

        RequestList::new(RequestListProps {
            requests,
            selected_request_id: self.state.selected_request.as_ref().map(|r| r.id.clone()),
            on_request_select: Some(on_request_select),
            scroll_handle: self.state.request_list_scroll_handle.clone(),
        })
        .render()
    }

    /// Render the connection detail panel
    fn render_connection_detail_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity().clone();
        let on_tab_select: Arc<dyn Fn(ConnectionDetailTab, &mut App) + Send + Sync + 'static> =
            Arc::new(move |tab: ConnectionDetailTab, cx: &mut App| {
                entity.update(cx, |app, cx| {
                    app.state.active_connection_detail_tab = tab;
                    cx.notify();
                });
            });

        connection_detail_panel(ConnectionDetailPanelProps {
            selected_connection: self.state.selected_connection.clone(),
            active_tab: self.state.active_connection_detail_tab,
            on_tab_select: Some(on_tab_select),
            height: self.state.detail_panel_height,
            scroll_handle: self.state.connection_detail_scroll_handle.clone(),
        })
    }

    /// Render the connection list component
    fn render_connection_list(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity().clone();
        let on_connection_select: Arc<
            dyn Fn(&roxy_core::TcpConnectionRow, &mut App) + Send + Sync + 'static,
        > = Arc::new(
            move |connection: &roxy_core::TcpConnectionRow, cx: &mut App| {
                let connection = connection.clone();
                entity.update(cx, |app, cx| {
                    app.state.selected_connection = Some(connection);
                    cx.notify();
                });
            },
        );

        connection_list(ConnectionListProps {
            connections: self.state.tcp_connections.clone(),
            selected_connection_id: self
                .state
                .selected_connection
                .as_ref()
                .map(|c| c.id.clone()),
            on_connection_select: Some(on_connection_select),
            scroll_handle: self.state.connections_scroll_handle.clone(),
        })
    }

    /// Render the detail panel component
    fn render_detail_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity().clone();
        let on_tab_select: Arc<dyn Fn(DetailTab, &mut App) + Send + Sync + 'static> =
            Arc::new(move |tab: DetailTab, cx: &mut App| {
                entity.update(cx, |app, cx| {
                    app.state.set_detail_tab(tab);
                    cx.notify();
                });
            });

        DetailPanel::new(DetailPanelProps {
            selected_request: self.state.selected_request.clone(),
            active_tab: self.state.active_detail_tab,
            on_tab_select: Some(on_tab_select),
            height: self.state.detail_panel_height,
            scroll_handle: self.state.detail_panel_scroll_handle.clone(),
        })
        .render()
    }

    /// Render the status bar component
    fn render_status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity().clone();
        let on_system_proxy_toggle: std::sync::Arc<dyn Fn(bool, &mut App) + Send + Sync + 'static> =
            std::sync::Arc::new(move |enabled: bool, cx: &mut App| {
                entity.update(cx, |app, cx| {
                    if enabled {
                        if let Err(e) = system_proxy::set_system_proxy(8080) {
                            tracing::error!("Failed to enable system proxy: {}", e);
                            app.state.error_message =
                                Some(format!("Failed to enable system proxy: {}", e));
                        } else {
                            tracing::info!(
                                "System proxy enabled - all macOS traffic now routes through Roxy"
                            );
                            app.state.set_system_proxy_enabled(true);
                        }
                    } else {
                        if let Err(e) = system_proxy::clear_system_proxy() {
                            tracing::error!("Failed to disable system proxy: {}", e);
                            app.state.error_message =
                                Some(format!("Failed to disable system proxy: {}", e));
                        } else {
                            tracing::info!("System proxy disabled");
                            app.state.set_system_proxy_enabled(false);
                        }
                    }
                    cx.notify();
                });
            });

        StatusBar::new(StatusBarProps {
            request_count: self.state.request_count(),
            error_message: self.state.error_message.clone(),
            clickhouse_connected: true, // TODO: actual connection check
            otel_connected: true,       // TODO: actual connection check
            system_proxy_enabled: self.state.system_proxy_enabled,
            on_system_proxy_toggle: Some(on_system_proxy_toggle),
        })
        .render()
    }
}

impl Render for RoxyApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Process any pending messages from background tasks
        self.state.process_messages();

        // Check for proxy status updates on each render
        self.update_proxy_status(cx);

        let entity = cx.entity().clone();
        let window_height: f32 = window.viewport_size().height.into();

        let mut container = div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(colors::BASE))
            .text_color(rgb(colors::TEXT))
            .font_family("Berkeley Mono")
            .child(self.render_title_bar())
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .overflow_hidden()
                    .child(self.render_left_dock(cx))
                    .child(self.render_left_dock_resize_handle(cx))
                    .child(self.render_main_content(cx)),
            )
            .child(self.render_status_bar(cx));

        // Always add global mouse handlers for resizing
        container = container
            .on_mouse_move({
                let entity = entity.clone();
                move |event, _window, cx| {
                    entity.update(cx, |app, cx| {
                        let mut changed = false;
                        if app.state.is_resizing_sidebar {
                            let new_width: f32 = event.position.x.into();
                            if new_width > 0.0 {
                                app.state.set_sidebar_width(new_width);
                                changed = true;
                            }
                        }
                        if app.state.is_resizing_detail_panel {
                            // Calculate height from bottom of window
                            let pos_y: f32 = event.position.y.into();
                            let new_height = window_height - pos_y;
                            if new_height > 0.0 {
                                app.state.set_detail_panel_height(new_height);
                                changed = true;
                            }
                        }
                        if changed {
                            cx.notify();
                        }
                    });
                }
            })
            .on_mouse_up(MouseButton::Left, {
                let entity = entity.clone();
                move |_event, _window, cx| {
                    entity.update(cx, |app, cx| {
                        if app.state.is_resizing_sidebar || app.state.is_resizing_detail_panel {
                            app.state.stop_resizing_sidebar();
                            app.state.stop_resizing_detail_panel();
                            cx.notify();
                        }
                    });
                }
            });

        container
    }
}

/// Set the dock icon programmatically for development builds
/// This is needed when running outside of an app bundle
/// Must be called AFTER GPUI Application is initialized
#[cfg(target_os = "macos")]
fn set_dock_icon() {
    use std::panic;

    // Try to load the icon from the resources directory
    let icon_paths = [
        // When running from project root
        "crates/roxy-ui/resources/app-icon@2x.png",
        "crates/roxy-ui/resources/app-icon.png",
        // When running from crates/roxy-ui
        "resources/app-icon@2x.png",
        "resources/app-icon.png",
    ];

    for path in &icon_paths {
        if let Ok(data) = std::fs::read(path) {
            // Use catch_unwind to handle any objc runtime issues gracefully
            let result = panic::catch_unwind(|| unsafe {
                let ns_data: id = NSData::dataWithBytes_length_(
                    nil,
                    data.as_ptr() as *const std::ffi::c_void,
                    data.len() as u64,
                );
                if ns_data == nil {
                    return false;
                }
                let ns_image: id = NSImage::initWithData_(NSImage::alloc(nil), ns_data);
                if ns_image == nil {
                    return false;
                }
                // Use sharedApplication instead of NSApp() for better compatibility
                let app: id = msg_send![class!(NSApplication), sharedApplication];
                if app == nil {
                    return false;
                }
                let _: () = msg_send![app, setApplicationIconImage: ns_image];
                true
            });

            match result {
                Ok(true) => {
                    tracing::debug!("Set dock icon from: {}", path);
                    return;
                }
                Ok(false) => continue,
                Err(_) => {
                    tracing::debug!(
                        "Failed to set dock icon (objc error), continuing without icon"
                    );
                    return;
                }
            }
        }
    }

    tracing::debug!("No icon file found, using default icon");
}

#[cfg(not(target_os = "macos"))]
fn set_dock_icon() {
    // No-op on non-macOS platforms
}

fn main() {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("roxy_ui=debug".parse().unwrap())
                .add_directive("roxy_core=info".parse().unwrap())
                .add_directive("roxy_proxy=debug".parse().unwrap())
                .add_directive("gpui=warn".parse().unwrap()),
        )
        .init();

    tracing::info!("Starting Roxy UI v{}", CURRENT_VERSION);

    Application::new().run(|cx: &mut App| {
        // Set the dock icon for development builds (when not running as .app bundle)
        // Must be called after GPUI initializes NSApplication
        set_dock_icon();

        // Bind keyboard shortcuts
        cx.bind_keys([
            // Application menu
            KeyBinding::new("cmd-q", Quit, None),
            KeyBinding::new("cmd-,", About, None), // Preferences (shows About for now)
            // File menu
            KeyBinding::new("cmd-k", ClearRequests, None),
            // Window
            KeyBinding::new("cmd-w", CloseWindow, None),
            KeyBinding::new("cmd-m", Minimize, None),
            // Help
            KeyBinding::new("cmd-shift-/", ShowHelp, None),
        ]);

        // Set up the macOS menu bar
        cx.set_menus(vec![
            Menu {
                name: "Roxy".into(),
                items: vec![
                    MenuItem::action("About Roxy", About),
                    MenuItem::separator(),
                    MenuItem::action("Quit Roxy", Quit), // cmd-q
                ],
            },
            Menu {
                name: "File".into(),
                items: vec![
                    MenuItem::action("Clear Requests", ClearRequests), // cmd-k
                    MenuItem::separator(),
                    MenuItem::action("Toggle Proxy", ToggleProxy),
                ],
            },
            Menu {
                name: "Window".into(),
                items: vec![
                    MenuItem::action("Minimize", Minimize),
                    MenuItem::action("Zoom", Zoom),
                    MenuItem::separator(),
                    MenuItem::action("Close Window", CloseWindow),
                ],
            },
            Menu {
                name: "Help".into(),
                items: vec![MenuItem::action("Roxy Help", ShowHelp)], // cmd-?
            },
        ]);

        // Register global action handlers
        cx.on_action(|_: &Quit, cx| {
            // Clear system proxy before quitting to restore network settings
            if let Err(e) = system_proxy::clear_system_proxy() {
                tracing::error!("Failed to clear system proxy on quit: {}", e);
            } else {
                tracing::info!("System proxy cleared on quit");
            }
            cx.quit();
        });

        cx.on_action(|_: &About, cx| {
            open_about_window(cx);
        });

        cx.on_action(|_: &ClearRequests, _cx| {
            tracing::info!("Clear requests action triggered");
        });

        cx.on_action(|_: &ToggleProxy, _cx| {
            tracing::info!("Toggle proxy action triggered");
        });

        cx.on_action(|_: &ShowHelp, _cx| {
            tracing::info!("Show help action triggered");
        });

        cx.on_action(|_: &Minimize, _cx| {
            tracing::info!("Minimize action triggered");
            // GPUI handles window minimize automatically through the menu
        });

        cx.on_action(|_: &Zoom, _cx| {
            tracing::info!("Zoom action triggered");
            // GPUI handles window zoom automatically through the menu
        });

        cx.on_action(|_: &CloseWindow, cx| {
            // For a single-window app, closing the window quits
            cx.quit();
        });

        // Configure window options
        let window_options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds {
                origin: point(px(100.0), px(100.0)),
                size: size(px(1400.0), px(900.0)),
            })),
            titlebar: Some(TitlebarOptions {
                title: Some("Roxy - Network Debugger".into()),
                appears_transparent: true,
                traffic_light_position: Some(point(px(10.0), px(10.0))),
            }),
            focus: true,
            show: true,
            kind: WindowKind::Normal,
            is_movable: true,
            app_id: Some("dev.roxy.app".to_string()),
            window_background: WindowBackgroundAppearance::Opaque,
            ..Default::default()
        };

        cx.open_window(window_options, |_, cx| cx.new(RoxyApp::new))
            .expect("Failed to open window");
    });
}
