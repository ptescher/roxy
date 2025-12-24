//! About Dialog component for Roxy
//!
//! Displays application information including version, description,
//! and credits in a modal dialog.

use gpui::prelude::*;
use gpui::*;
use roxy_core::CURRENT_VERSION;

use crate::theme::{colors, font_size, spacing};

/// About dialog component
pub struct AboutDialog;

impl AboutDialog {
    pub fn new() -> Self {
        Self
    }

    fn render_tech_badge(&self, name: &str) -> impl IntoElement {
        div()
            .px(spacing::XS)
            .py(spacing::XXS)
            .rounded(px(4.0))
            .bg(rgb(colors::SURFACE_0))
            .text_size(font_size::XS)
            .text_color(rgb(colors::SUBTEXT_1))
            .child(name.to_string())
    }

    fn render_link(&self, label: &str) -> impl IntoElement {
        div()
            .text_size(font_size::XS)
            .text_color(rgb(colors::BLUE))
            .cursor_pointer()
            .hover(|style| style.text_color(rgb(colors::SAPPHIRE)))
            .child(label.to_string())
    }
}

impl Default for AboutDialog {
    fn default() -> Self {
        Self::new()
    }
}

impl Render for AboutDialog {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .w(px(360.0))
            .bg(rgb(colors::BASE))
            .rounded(px(12.0))
            .overflow_hidden()
            .child(
                // Content
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .p(spacing::XL)
                    .gap(spacing::MD)
                    // App icon placeholder (a styled "R")
                    .child(
                        div()
                            .size(px(80.0))
                            .rounded(px(16.0))
                            .bg(rgb(colors::MAUVE))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                div()
                                    .text_size(px(48.0))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(rgb(colors::BASE))
                                    .child("R"),
                            ),
                    )
                    // App name
                    .child(
                        div()
                            .text_size(font_size::XXL)
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(colors::TEXT))
                            .child("Roxy"),
                    )
                    // Version
                    .child(
                        div()
                            .text_size(font_size::MD)
                            .text_color(rgb(colors::SUBTEXT_0))
                            .child(format!("Version {}", CURRENT_VERSION)),
                    )
                    // Description
                    .child(
                        div()
                            .text_size(font_size::SM)
                            .text_color(rgb(colors::OVERLAY_1))
                            .max_w(px(300.0))
                            .child("A native network debugging proxy with powerful routing and inspection capabilities."),
                    )
                    // Spacer
                    .child(div().h(spacing::SM))
                    // Built with
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap(spacing::XXS)
                            .child(
                                div()
                                    .text_size(font_size::XS)
                                    .text_color(rgb(colors::OVERLAY_0))
                                    .child("Built with"),
                            )
                            .child(
                                div()
                                    .flex()
                                    .gap(spacing::SM)
                                    .child(self.render_tech_badge("Rust"))
                                    .child(self.render_tech_badge("GPUI"))
                                    .child(self.render_tech_badge("ClickHouse")),
                            ),
                    )
                    // Spacer
                    .child(div().h(spacing::SM))
                    // Copyright
                    .child(
                        div()
                            .text_size(font_size::XS)
                            .text_color(rgb(colors::OVERLAY_0))
                            .child("© 2024 Roxy Contributors"),
                    )
                    // Links
                    .child(
                        div()
                            .flex()
                            .gap(spacing::MD)
                            .child(self.render_link("GitHub"))
                            .child(self.render_link("MIT License")),
                    ),
            )
    }
}

/// Open the About dialog in a new window
pub fn open_about_window(cx: &mut App) {
    let window_options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin: point(px(200.0), px(200.0)),
            size: size(px(360.0), px(420.0)),
        })),
        titlebar: Some(TitlebarOptions {
            title: Some("About Roxy".into()),
            appears_transparent: false,
            traffic_light_position: None,
        }),
        focus: true,
        show: true,
        kind: WindowKind::PopUp,
        is_movable: true,
        app_id: Some("dev.roxy.about".to_string()),
        window_background: WindowBackgroundAppearance::Opaque,
        ..Default::default()
    };

    cx.open_window(window_options, |_window, cx| {
        cx.new(|_cx| AboutDialog::new())
    })
    .ok();
}
