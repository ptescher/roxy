//! Title bar component for Roxy UI
//!
//! This component renders the application title bar with the app name,
//! version, and update notification.

use gpui::prelude::*;
use gpui::*;
use roxy_core::CURRENT_VERSION;

use crate::theme::{colors, dimensions, font_size, spacing, Theme};

/// Properties for the TitleBar component
#[derive(Clone)]
pub struct TitleBarProps {
    /// Version available for update (if any)
    pub update_available: Option<String>,
}

impl Default for TitleBarProps {
    fn default() -> Self {
        Self {
            update_available: None,
        }
    }
}

/// Title bar component
pub struct TitleBar {
    props: TitleBarProps,
    theme: Theme,
}

impl TitleBar {
    /// Create a new title bar with the given props
    pub fn new(props: TitleBarProps) -> Self {
        Self {
            props,
            theme: Theme::dark(),
        }
    }

    /// Render the title bar
    pub fn render(&self) -> impl IntoElement {
        let update_available = self.props.update_available.clone();

        div()
            .flex()
            .items_center()
            .justify_between()
            .h(dimensions::TITLE_BAR_HEIGHT)
            .px(spacing::MD)
            .bg(rgb(colors::CRUST))
            .pl(dimensions::TRAFFIC_LIGHT_PADDING)
            .child(self.render_logo())
            .child(self.render_actions(update_available))
    }

    /// Render the logo and version
    fn render_logo(&self) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .gap(spacing::SM)
            .child(
                div()
                    .text_size(font_size::LG)
                    .font_weight(FontWeight::BOLD)
                    .child("Roxy"),
            )
            .child(
                div()
                    .text_size(font_size::SM)
                    .text_color(self.theme.text_muted)
                    .child(format!("v{}", CURRENT_VERSION)),
            )
    }

    /// Render the action buttons (update notification, etc.)
    fn render_actions(&self, update_available: Option<String>) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .gap(spacing::XS)
            .children(update_available.map(|version| self.render_update_badge(version)))
    }

    /// Render the update available badge
    fn render_update_badge(&self, version: String) -> impl IntoElement {
        div()
            .px(spacing::XS)
            .py(spacing::XXS)
            .rounded(dimensions::BORDER_RADIUS)
            .bg(self.theme.button_primary)
            .text_size(font_size::SM)
            .text_color(self.theme.button_primary_text)
            .cursor_pointer()
            .hover(|style| style.opacity(0.9))
            .child(format!("Update to v{}", version))
    }
}

/// Convenience function to render a title bar
pub fn title_bar(props: TitleBarProps) -> impl IntoElement {
    TitleBar::new(props).render()
}
