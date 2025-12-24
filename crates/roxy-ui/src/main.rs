//! Roxy UI - Native network debugging interface built with GPUI
//!
//! This is the main entry point for the Roxy desktop application.
//! It provides a GUI for viewing and analyzing network traffic captured
//! by the proxy worker, which it manages as a subprocess.

mod components;
mod state;
mod theme;

use gpui::prelude::*;
use gpui::*;
use roxy_core::{ClickHouseConfig, RoxyClickHouse, CURRENT_VERSION};

#[cfg(target_os = "macos")]
use cocoa::{
    appkit::{NSApp, NSImage},
    base::{id, nil},
    foundation::NSData,
};
#[cfg(target_os = "macos")]
use objc::{msg_send, sel, sel_impl};

use roxy_proxy::{ProxyConfig, ProxyServer};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use components::{
    open_about_window, DetailPanel, DetailPanelProps, DetailTab, RequestList, RequestListProps,
    Sidebar, SidebarProps, StatusBar, StatusBarProps, TitleBar, TitleBarProps, Toolbar,
    ToolbarProps,
};
use state::{AppState, ProxyStatus, UiMessage};
use theme::{colors, dimensions};

// Define actions for the menu bar
actions!(roxy, [Quit, About, ClearRequests, ToggleProxy, ShowHelp]);

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

    /// Render the sidebar component
    fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity().clone();
        let on_host_select: Arc<dyn Fn(&str, &mut App) + Send + Sync + 'static> = {
            let entity = entity.clone();
            Arc::new(move |host: &str, cx: &mut App| {
                let host = host.to_string();
                entity.update(cx, |app, cx| {
                    app.state.select_host(host);
                    cx.notify();
                });
            })
        };

        let on_clear_filter: Arc<dyn Fn(&mut App) + Send + Sync + 'static> = {
            let entity = entity.clone();
            Arc::new(move |cx: &mut App| {
                entity.update(cx, |app, cx| {
                    app.state.clear_host_filter();
                    cx.notify();
                });
            })
        };

        Sidebar::new(SidebarProps {
            proxy_status: self.state.proxy_status.clone(),
            hosts: self.state.hosts.clone(),
            selected_host: self.state.selected_host.clone(),
            on_host_select: Some(on_host_select),
            on_clear_filter: Some(on_clear_filter),
            width: self.state.sidebar_width,
            scroll_handle: self.state.sidebar_scroll_handle.clone(),
        })
        .render()
    }

    /// Render the sidebar resize handle
    fn render_sidebar_resize_handle(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
                        app.state.start_resizing_sidebar();
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

        div()
            .flex()
            .flex_col()
            .flex_1()
            .overflow_hidden()
            // Toolbar - fixed height, no shrink
            .child(div().flex_shrink_0().child(self.render_toolbar()))
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
    }

    /// Render the toolbar component
    fn render_toolbar(&self) -> impl IntoElement {
        Toolbar::new(ToolbarProps {
            request_count: self.state.request_count(),
            active_filter: Default::default(),
        })
        .render()
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
    fn render_status_bar(&self) -> impl IntoElement {
        StatusBar::new(StatusBarProps {
            request_count: self.state.request_count(),
            error_message: self.state.error_message.clone(),
            clickhouse_connected: true, // TODO: actual connection check
            otel_connected: true,       // TODO: actual connection check
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
                    .child(self.render_sidebar(cx))
                    .child(self.render_sidebar_resize_handle(cx))
                    .child(self.render_main_content(cx)),
            )
            .child(self.render_status_bar());

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
#[cfg(target_os = "macos")]
fn set_dock_icon() {
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
            unsafe {
                let ns_data: id = NSData::dataWithBytes_length_(
                    nil,
                    data.as_ptr() as *const std::ffi::c_void,
                    data.len() as u64,
                );
                if ns_data != nil {
                    let ns_image: id = NSImage::initWithData_(NSImage::alloc(nil), ns_data);
                    if ns_image != nil {
                        let app: id = NSApp();
                        let _: () = msg_send![app, setApplicationIconImage: ns_image];
                        tracing::debug!("Set dock icon from: {}", path);
                        return;
                    }
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

    // Set the dock icon for development builds (when not running as .app bundle)
    set_dock_icon();

    Application::new().run(|cx: &mut App| {
        // Set up the macOS menu bar
        cx.set_menus(vec![
            Menu {
                name: "Roxy".into(),
                items: vec![
                    MenuItem::action("About Roxy", About),
                    MenuItem::separator(),
                    MenuItem::action("Quit Roxy", Quit),
                ],
            },
            Menu {
                name: "File".into(),
                items: vec![
                    MenuItem::action("Clear Requests", ClearRequests),
                    MenuItem::separator(),
                    MenuItem::action("Toggle Proxy", ToggleProxy),
                ],
            },
            Menu {
                name: "Help".into(),
                items: vec![MenuItem::action("Roxy Help", ShowHelp)],
            },
        ]);

        // Register global action handlers
        cx.on_action(|_: &Quit, cx| {
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
