//! Database query detail panel component for Roxy UI
//!
//! This component displays detailed information about a selected database query,
//! including the full SQL statement, timing, and metadata.

use gpui::prelude::*;
use gpui::*;
use roxy_core::DatabaseQueryRow;
use std::sync::Arc;

use crate::theme::{colors, dimensions, font_size, spacing, Theme};

/// Tabs available in the database detail panel
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DatabaseDetailTab {
    /// Overview of the query
    #[default]
    Overview,
    /// Full SQL statement
    Query,
    /// Error details (if any)
    Error,
}

impl DatabaseDetailTab {
    pub fn label(&self) -> &'static str {
        match self {
            DatabaseDetailTab::Overview => "Overview",
            DatabaseDetailTab::Query => "Query",
            DatabaseDetailTab::Error => "Error",
        }
    }

    pub fn all() -> &'static [DatabaseDetailTab] {
        &[
            DatabaseDetailTab::Overview,
            DatabaseDetailTab::Query,
            DatabaseDetailTab::Error,
        ]
    }
}

/// Callback type for tab selection
pub type OnTabSelect = Arc<dyn Fn(DatabaseDetailTab, &mut App) + Send + Sync + 'static>;

/// Properties for the DatabaseDetailPanel component
#[derive(Clone)]
pub struct DatabaseDetailPanelProps {
    /// The selected query to display details for
    pub selected_query: Option<DatabaseQueryRow>,
    /// Currently active tab
    pub active_tab: DatabaseDetailTab,
    /// Callback when a tab is selected
    pub on_tab_select: Option<OnTabSelect>,
    /// Panel height
    pub height: f32,
    /// Scroll handle for the content
    pub scroll_handle: ScrollHandle,
}

impl Default for DatabaseDetailPanelProps {
    fn default() -> Self {
        Self {
            selected_query: None,
            active_tab: DatabaseDetailTab::default(),
            on_tab_select: None,
            height: 300.0,
            scroll_handle: ScrollHandle::new(),
        }
    }
}

/// Database detail panel component
pub struct DatabaseDetailPanel {
    props: DatabaseDetailPanelProps,
    theme: Theme,
}

impl DatabaseDetailPanel {
    /// Create a new database detail panel
    pub fn new(props: DatabaseDetailPanelProps) -> Self {
        Self {
            props,
            theme: Theme::dark(),
        }
    }

    /// Render the detail panel
    pub fn render(&self) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(colors::MANTLE))
            .border_t_1()
            .border_color(rgb(colors::SURFACE_0))
            .child(self.render_tabs())
            .child(self.render_content())
    }

    /// Render the tab bar
    fn render_tabs(&self) -> impl IntoElement {
        let active_tab = self.props.active_tab;
        let on_tab_select = self.props.on_tab_select.clone();

        div()
            .flex()
            .items_center()
            .h(dimensions::DETAIL_PANEL_TABS_HEIGHT)
            .px(spacing::MD)
            .bg(rgb(colors::MANTLE))
            .border_b_1()
            .border_color(rgb(colors::SURFACE_0))
            .gap(spacing::XS)
            .children(DatabaseDetailTab::all().iter().map(|tab| {
                let is_active = *tab == active_tab;
                let tab_value = *tab;
                let on_select = on_tab_select.clone();

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

                let mut el = div()
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
                    .child(tab.label().to_string());

                if let Some(callback) = on_select {
                    el = el.on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                        callback(tab_value, cx);
                    });
                }

                el
            }))
    }

    /// Render the content area based on active tab
    fn render_content(&self) -> impl IntoElement {
        let scroll_handle = self.props.scroll_handle.clone();

        div()
            .flex_1()
            .overflow_hidden()
            .id("database-detail-content")
            .overflow_y_scroll()
            .track_scroll(&scroll_handle)
            .child(self.render_query_content())
    }

    /// Render content for the selected query
    fn render_query_content(&self) -> impl IntoElement {
        match &self.props.selected_query {
            Some(query) => self.render_query_details(query).into_any_element(),
            None => self.render_empty_state().into_any_element(),
        }
    }

    /// Render the details for a query
    fn render_query_details(&self, query: &DatabaseQueryRow) -> impl IntoElement {
        match self.props.active_tab {
            DatabaseDetailTab::Overview => self.render_overview(query).into_any_element(),
            DatabaseDetailTab::Query => self.render_query_tab(query).into_any_element(),
            DatabaseDetailTab::Error => self.render_error_tab(query).into_any_element(),
        }
    }

    /// Render the overview tab
    fn render_overview(&self, query: &DatabaseQueryRow) -> impl IntoElement {
        let status_color = if query.success == 1 {
            hsla(142.0 / 360.0, 0.7, 0.45, 1.0) // Green
        } else {
            hsla(0.0 / 360.0, 0.7, 0.5, 1.0) // Red
        };

        div()
            .flex()
            .flex_col()
            .p(spacing::MD)
            .gap(spacing::MD)
            // Connection Info section
            .child(
                self.render_section(
                    "Connection",
                    div()
                        .flex()
                        .flex_col()
                        .gap(spacing::SM)
                        .child(self.render_field("System", format_system(&query.db_system)))
                        .child(self.render_field("Database", query.db_name.clone()))
                        .child(self.render_field("User", query.db_user.clone()))
                        .child(self.render_field(
                            "Server",
                            format!("{}:{}", query.server_address, query.server_port),
                        ))
                        .child(self.render_field("Client", query.client_address.clone()))
                        .child(self.render_field("Application", query.application_name.clone())),
                ),
            )
            // Query Info section
            .child(
                self.render_section(
                    "Query",
                    div()
                        .flex()
                        .flex_col()
                        .gap(spacing::SM)
                        .child(self.render_field("Operation", query.db_operation.to_uppercase()))
                        .child(self.render_field_with_color(
                            "Status",
                            if query.success == 1 {
                                "Success"
                            } else {
                                "Failed"
                            },
                            status_color,
                        ))
                        .child(self.render_field("Duration", format_duration(query.duration_ms)))
                        .child(self.render_field(
                            "Rows Affected",
                            if query.db_rows_affected >= 0 {
                                query.db_rows_affected.to_string()
                            } else {
                                "-".to_string()
                            },
                        )),
                ),
            )
            // Tracing section
            .child(
                self.render_section(
                    "Tracing",
                    div()
                        .flex()
                        .flex_col()
                        .gap(spacing::SM)
                        .child(self.render_field("Trace ID", query.trace_id.clone()))
                        .child(self.render_field("Span ID", query.span_id.clone()))
                        .child(self.render_field(
                            "Parent Span",
                            if query.parent_span_id.is_empty() {
                                "-".to_string()
                            } else {
                                query.parent_span_id.clone()
                            },
                        )),
                ),
            )
    }

    /// Render the query tab (full SQL statement)
    fn render_query_tab(&self, query: &DatabaseQueryRow) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .p(spacing::MD)
            .gap(spacing::MD)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(spacing::SM)
                    // Operation badge
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(spacing::SM)
                            .child(
                                div()
                                    .text_size(font_size::SM)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(self.theme.text_muted)
                                    .child("Operation:"),
                            )
                            .child(render_operation_badge(&query.db_operation)),
                    )
                    // SQL statement in a code block
                    .child(
                        div()
                            .mt(spacing::SM)
                            .p(spacing::MD)
                            .bg(rgb(colors::BASE))
                            .rounded(dimensions::BORDER_RADIUS)
                            .border_1()
                            .border_color(rgb(colors::SURFACE_0))
                            .child(
                                div()
                                    .text_size(font_size::SM)
                                    .font_family("monospace")
                                    .text_color(self.theme.text_primary)
                                    .child(query.db_statement.clone()),
                            ),
                    ),
            )
    }

    /// Render the error tab
    fn render_error_tab(&self, query: &DatabaseQueryRow) -> impl IntoElement {
        if query.success == 1 {
            return div()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .py(spacing::XXL)
                .child(
                    div()
                        .text_size(font_size::MD)
                        .text_color(self.theme.text_muted)
                        .child("No errors - query completed successfully"),
                )
                .into_any_element();
        }

        div()
            .flex()
            .flex_col()
            .p(spacing::MD)
            .gap(spacing::MD)
            .child(
                self.render_section(
                    "Error Details",
                    div()
                        .flex()
                        .flex_col()
                        .gap(spacing::SM)
                        .child(self.render_field_with_color(
                            "Status",
                            "Failed",
                            hsla(0.0 / 360.0, 0.7, 0.5, 1.0),
                        ))
                        .child(self.render_field(
                            "Error Code",
                            if query.error_code.is_empty() {
                                "-".to_string()
                            } else {
                                query.error_code.clone()
                            },
                        ))
                        .child(
                            div()
                                .mt(spacing::SM)
                                .p(spacing::MD)
                                .bg(hsla(0.0 / 360.0, 0.3, 0.15, 1.0))
                                .rounded(dimensions::BORDER_RADIUS)
                                .border_1()
                                .border_color(hsla(0.0 / 360.0, 0.5, 0.3, 1.0))
                                .child(
                                    div()
                                        .text_size(font_size::SM)
                                        .text_color(hsla(0.0 / 360.0, 0.7, 0.7, 1.0))
                                        .child(if query.error_message.is_empty() {
                                            "No error message available".to_string()
                                        } else {
                                            query.error_message.clone()
                                        }),
                                ),
                        ),
                ),
            )
            .into_any_element()
    }

    /// Render a section with title
    fn render_section(&self, title: &str, content: impl IntoElement) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap(spacing::SM)
            .child(
                div()
                    .text_size(font_size::SM)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(self.theme.text_muted)
                    .child(title.to_string()),
            )
            .child(
                div()
                    .pl(spacing::SM)
                    .border_l_2()
                    .border_color(rgb(colors::SURFACE_1))
                    .child(content),
            )
    }

    /// Render a field with label and value
    fn render_field(&self, label: &str, value: impl Into<SharedString>) -> impl IntoElement {
        div()
            .flex()
            .items_baseline()
            .gap(spacing::SM)
            .child(
                div()
                    .w(px(100.0))
                    .flex_shrink_0()
                    .text_size(font_size::SM)
                    .text_color(self.theme.text_muted)
                    .child(format!("{}:", label)),
            )
            .child(
                div()
                    .flex_1()
                    .text_size(font_size::SM)
                    .text_color(self.theme.text_primary)
                    .child(value.into()),
            )
    }

    /// Render a field with colored value
    fn render_field_with_color(
        &self,
        label: &str,
        value: impl Into<SharedString>,
        color: Hsla,
    ) -> impl IntoElement {
        div()
            .flex()
            .items_baseline()
            .gap(spacing::SM)
            .child(
                div()
                    .w(px(100.0))
                    .flex_shrink_0()
                    .text_size(font_size::SM)
                    .text_color(self.theme.text_muted)
                    .child(format!("{}:", label)),
            )
            .child(
                div()
                    .flex_1()
                    .text_size(font_size::SM)
                    .text_color(color)
                    .font_weight(FontWeight::MEDIUM)
                    .child(value.into()),
            )
    }

    /// Render empty state when no query is selected
    fn render_empty_state(&self) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .size_full()
            .py(spacing::XXL)
            .child(
                div()
                    .text_size(font_size::MD)
                    .text_color(self.theme.text_muted)
                    .child("Select a query to view details"),
            )
    }
}

/// Format database system name
fn format_system(system: &str) -> String {
    match system {
        "postgresql" => "PostgreSQL".to_string(),
        "mysql" => "MySQL".to_string(),
        "mariadb" => "MariaDB".to_string(),
        "sqlite" => "SQLite".to_string(),
        _ => system.to_string(),
    }
}

/// Render operation badge
fn render_operation_badge(operation: &str) -> impl IntoElement {
    let color = get_operation_color(operation);
    let label = operation.to_uppercase();

    div()
        .px(spacing::SM)
        .py(px(2.0))
        .bg(color.opacity(0.2))
        .rounded(px(4.0))
        .text_size(font_size::XS)
        .text_color(color)
        .font_weight(FontWeight::MEDIUM)
        .child(label)
}

/// Get color for operation type
fn get_operation_color(operation: &str) -> Hsla {
    match operation.to_uppercase().as_str() {
        "SELECT" => hsla(210.0 / 360.0, 0.7, 0.6, 1.0), // Blue - read
        "INSERT" => hsla(142.0 / 360.0, 0.7, 0.45, 1.0), // Green - create
        "UPDATE" => hsla(45.0 / 360.0, 0.8, 0.5, 1.0),  // Yellow - modify
        "DELETE" => hsla(0.0 / 360.0, 0.7, 0.5, 1.0),   // Red - delete
        _ => hsla(270.0 / 360.0, 0.5, 0.6, 1.0),        // Purple - other
    }
}

/// Format duration in ms to human readable
fn format_duration(ms: f64) -> String {
    if ms < 1.0 {
        format!("{:.0}µs", ms * 1000.0)
    } else if ms < 1000.0 {
        format!("{:.2}ms", ms)
    } else if ms < 60000.0 {
        format!("{:.3}s", ms / 1000.0)
    } else {
        format!("{:.1}m", ms / 60000.0)
    }
}

/// Standalone function to render database detail panel
pub fn database_detail_panel(props: DatabaseDetailPanelProps) -> impl IntoElement {
    DatabaseDetailPanel::new(props).render()
}
