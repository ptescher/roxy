//! Database query list component for Roxy UI
//!
//! This component renders the scrollable list of database queries
//! (PostgreSQL, MySQL, etc.) with a header and individual query rows.

use gpui::prelude::*;
use gpui::*;
use roxy_core::DatabaseQueryRow;
use std::sync::Arc;

use crate::theme::{colors, dimensions, font_size, spacing, Theme};
use gpui::MouseButton;

/// Callback type for query selection
pub type OnQuerySelect = Arc<dyn Fn(&DatabaseQueryRow, &mut App) + Send + Sync + 'static>;

/// Properties for the DatabaseList component
#[derive(Clone)]
pub struct DatabaseListProps {
    /// List of queries to display
    pub queries: Vec<DatabaseQueryRow>,
    /// Currently selected query ID (if any)
    pub selected_query_id: Option<String>,
    /// Callback when a query row is clicked
    pub on_query_select: Option<OnQuerySelect>,
    /// Scroll handle for the query list
    pub scroll_handle: ScrollHandle,
}

impl Default for DatabaseListProps {
    fn default() -> Self {
        Self {
            queries: Vec::new(),
            selected_query_id: None,
            on_query_select: None,
            scroll_handle: ScrollHandle::new(),
        }
    }
}

/// Database query list component
pub struct DatabaseList {
    props: DatabaseListProps,
    theme: Theme,
}

impl DatabaseList {
    /// Create a new database list with the given props
    pub fn new(props: DatabaseListProps) -> Self {
        Self {
            props,
            theme: Theme::dark(),
        }
    }

    /// Render the database list
    pub fn render(&self) -> impl IntoElement {
        let queries = &self.props.queries;

        div()
            .flex()
            .flex_col()
            .size_full()
            .overflow_hidden()
            // Header
            .child(div().flex_shrink_0().child(self.render_header()))
            // Rows
            .child(div().flex_1().overflow_hidden().child(self.render_rows()))
            .when(queries.is_empty(), |this| {
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
            // System column
            .child(div().w(px(70.0)).child("System"))
            // Status column
            .child(div().w(px(60.0)).child("Status"))
            // Operation column
            .child(div().w(px(80.0)).child("Operation"))
            // Database column
            .child(div().w(px(100.0)).child("Database"))
            // Query column - flexible
            .child(div().flex_1().min_w(px(200.0)).child("Query"))
            // Duration column
            .child(div().w(px(80.0)).child("Duration"))
            // Rows column
            .child(div().w(px(60.0)).child("Rows"))
    }

    /// Render the scrollable list of rows
    fn render_rows(&self) -> impl IntoElement {
        let queries = self.props.queries.clone();
        let selected_id = self.props.selected_query_id.clone();
        let on_select = self.props.on_query_select.clone();
        let scroll_handle = self.props.scroll_handle.clone();

        div()
            .size_full()
            .bg(rgb(colors::BASE))
            .id("database-list-rows")
            .overflow_y_scroll()
            .track_scroll(&scroll_handle)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .w_full()
                    .children(queries.iter().enumerate().map(|(index, query)| {
                        let is_selected = selected_id
                            .as_ref()
                            .map(|id| id == &query.id)
                            .unwrap_or(false);
                        self.render_query_row(query, index, is_selected, on_select.clone())
                    })),
            )
    }

    /// Render a single query row
    fn render_query_row(
        &self,
        query: &DatabaseQueryRow,
        index: usize,
        selected: bool,
        on_select: Option<OnQuerySelect>,
    ) -> impl IntoElement {
        let bg = if selected {
            rgb(colors::SURFACE_0)
        } else if index % 2 == 0 {
            rgb(colors::BASE)
        } else {
            rgb(colors::MANTLE)
        };

        let system_color = get_system_color(&query.db_system);
        let status_color = if query.success == 1 {
            hsla(142.0 / 360.0, 0.7, 0.45, 1.0) // Green
        } else {
            hsla(0.0 / 360.0, 0.7, 0.5, 1.0) // Red
        };

        // Clone query data for the callback
        let query_for_callback = query.clone();

        // Truncate the statement for display
        let statement_display = if query.db_statement.len() > 80 {
            format!("{}...", &query.db_statement[..77])
        } else {
            query.db_statement.clone()
        };

        let mut el = div()
            .id(ElementId::Name(format!("query-{}", query.id).into()))
            .flex()
            .items_center()
            .h(dimensions::REQUEST_ROW_HEIGHT)
            .px(spacing::MD)
            .bg(bg)
            .hover(|style| style.bg(rgb(colors::SURFACE_0)))
            .cursor_pointer()
            .border_b_1()
            .border_color(rgb(colors::SURFACE_0))
            // System column
            .child(
                div()
                    .w(px(70.0))
                    .flex_shrink_0()
                    .child(render_system_badge(&query.db_system, system_color)),
            )
            // Status column
            .child(
                div()
                    .w(px(60.0))
                    .flex_shrink_0()
                    .child(render_status_badge(query.success == 1, status_color)),
            )
            // Operation column
            .child(
                div()
                    .w(px(80.0))
                    .flex_shrink_0()
                    .child(render_operation_badge(&query.db_operation)),
            )
            // Database column
            .child(
                div()
                    .w(px(100.0))
                    .flex_shrink_0()
                    .text_size(font_size::SM)
                    .text_color(self.theme.text_secondary)
                    .text_ellipsis()
                    .child(query.db_name.clone()),
            )
            // Query column
            .child(
                div()
                    .flex_1()
                    .min_w(px(200.0))
                    .overflow_hidden()
                    .text_size(font_size::SM)
                    .text_color(self.theme.text_primary)
                    .text_ellipsis()
                    .child(statement_display),
            )
            // Duration column
            .child(
                div()
                    .w(px(80.0))
                    .flex_shrink_0()
                    .text_size(font_size::SM)
                    .text_color(self.theme.text_secondary)
                    .child(format_duration(query.duration_ms)),
            )
            // Rows column
            .child(
                div()
                    .w(px(60.0))
                    .flex_shrink_0()
                    .text_size(font_size::SM)
                    .text_color(self.theme.text_secondary)
                    .child(if query.db_rows_affected >= 0 {
                        query.db_rows_affected.to_string()
                    } else {
                        "-".to_string()
                    }),
            );

        if let Some(callback) = on_select {
            el = el.on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                callback(&query_for_callback, cx);
            });
        }

        el
    }

    /// Render empty state when there are no queries
    fn render_empty_state(&self) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .py(spacing::XXL)
            .gap(spacing::MD)
            .child(
                div()
                    .text_size(font_size::LG)
                    .text_color(self.theme.text_muted)
                    .child("No database queries yet"),
            )
            .child(
                div()
                    .text_size(font_size::SM)
                    .text_color(self.theme.text_muted)
                    .child("PostgreSQL and MySQL queries will appear here"),
            )
    }
}

/// Render database system badge
fn render_system_badge(system: &str, color: Hsla) -> impl IntoElement {
    let label = match system {
        "postgresql" => "PG",
        "mysql" => "MySQL",
        "mariadb" => "Maria",
        "sqlite" => "SQLite",
        _ => system,
    };

    div()
        .px(spacing::SM)
        .py(px(2.0))
        .bg(color.opacity(0.2))
        .rounded(px(4.0))
        .text_size(font_size::XS)
        .text_color(color)
        .font_weight(FontWeight::MEDIUM)
        .child(label.to_string())
}

/// Render status badge
fn render_status_badge(success: bool, color: Hsla) -> impl IntoElement {
    let label = if success { "OK" } else { "ERR" };

    div()
        .px(spacing::SM)
        .py(px(2.0))
        .bg(color.opacity(0.2))
        .rounded(px(4.0))
        .text_size(font_size::XS)
        .text_color(color)
        .font_weight(FontWeight::MEDIUM)
        .child(label.to_string())
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

/// Get color for database system type
fn get_system_color(system: &str) -> Hsla {
    match system {
        "postgresql" => hsla(210.0 / 360.0, 0.7, 0.6, 1.0), // Blue
        "mysql" => hsla(30.0 / 360.0, 0.8, 0.5, 1.0),       // Orange
        "mariadb" => hsla(200.0 / 360.0, 0.6, 0.5, 1.0),    // Light blue
        "sqlite" => hsla(180.0 / 360.0, 0.5, 0.5, 1.0),     // Teal
        _ => hsla(0.0, 0.0, 0.5, 1.0),                      // Gray
    }
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
        format!("{:.1}ms", ms)
    } else if ms < 60000.0 {
        format!("{:.2}s", ms / 1000.0)
    } else {
        format!("{:.1}m", ms / 60000.0)
    }
}

/// Standalone function to render database list
pub fn database_list(props: DatabaseListProps) -> impl IntoElement {
    DatabaseList::new(props).render()
}
