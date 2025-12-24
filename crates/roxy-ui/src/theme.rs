//! Theme definitions for Roxy UI
//!
//! This module provides color constants and theming utilities
//! for consistent styling across the application.

use gpui::{rgb, rgba, Hsla};

/// Catppuccin Mocha-inspired color palette
pub mod colors {

    // Base colors
    pub const BASE: u32 = 0x1e1e2e;
    pub const MANTLE: u32 = 0x181825;
    pub const CRUST: u32 = 0x11111b;

    // Surface colors
    pub const SURFACE_0: u32 = 0x313244;
    pub const SURFACE_1: u32 = 0x45475a;
    pub const SURFACE_2: u32 = 0x585b70;

    // Overlay colors
    pub const OVERLAY_0: u32 = 0x6c7086;
    pub const OVERLAY_1: u32 = 0x7f849c;
    pub const OVERLAY_2: u32 = 0x9399b2;

    // Text colors
    pub const TEXT: u32 = 0xcdd6f4;
    pub const SUBTEXT_1: u32 = 0xbac2de;
    pub const SUBTEXT_0: u32 = 0xa6adc8;

    // Accent colors
    pub const ROSEWATER: u32 = 0xf5e0dc;
    pub const FLAMINGO: u32 = 0xf2cdcd;
    pub const PINK: u32 = 0xf5c2e7;
    pub const MAUVE: u32 = 0xcba6f7;
    pub const RED: u32 = 0xf38ba8;
    pub const MAROON: u32 = 0xeba0ac;
    pub const PEACH: u32 = 0xfab387;
    pub const YELLOW: u32 = 0xf9e2af;
    pub const GREEN: u32 = 0xa6e3a1;
    pub const TEAL: u32 = 0x94e2d5;
    pub const SKY: u32 = 0x89dceb;
    pub const SAPPHIRE: u32 = 0x74c7ec;
    pub const BLUE: u32 = 0x89b4fa;
    pub const LAVENDER: u32 = 0xb4befe;

    // Transparent
    pub const TRANSPARENT: u32 = 0x00000000;
}

/// Theme struct with pre-computed Hsla colors for gpui
#[derive(Clone)]
pub struct Theme {
    // Backgrounds
    pub background: Hsla,
    pub background_secondary: Hsla,
    pub background_tertiary: Hsla,

    // Borders
    pub border: Hsla,
    pub border_focused: Hsla,

    // Text
    pub text_primary: Hsla,
    pub text_secondary: Hsla,
    pub text_muted: Hsla,

    // Status colors
    pub success: Hsla,
    pub warning: Hsla,
    pub error: Hsla,
    pub info: Hsla,

    // HTTP method colors
    pub method_get: Hsla,
    pub method_post: Hsla,
    pub method_put: Hsla,
    pub method_delete: Hsla,
    pub method_patch: Hsla,

    // Status code colors
    pub status_2xx: Hsla,
    pub status_3xx: Hsla,
    pub status_4xx: Hsla,
    pub status_5xx: Hsla,

    // Interactive
    pub button_primary: Hsla,
    pub button_primary_text: Hsla,
    pub hover: Hsla,
}

impl Default for Theme {
    fn default() -> Self {
        Self::dark()
    }
}

impl Theme {
    /// Create the default dark theme
    pub fn dark() -> Self {
        Self {
            // Backgrounds
            background: rgb(colors::BASE).into(),
            background_secondary: rgb(colors::MANTLE).into(),
            background_tertiary: rgb(colors::CRUST).into(),

            // Borders
            border: rgb(colors::SURFACE_0).into(),
            border_focused: rgb(colors::BLUE).into(),

            // Text
            text_primary: rgb(colors::TEXT).into(),
            text_secondary: rgb(colors::SUBTEXT_0).into(),
            text_muted: rgb(colors::OVERLAY_0).into(),

            // Status colors
            success: rgb(colors::GREEN).into(),
            warning: rgb(colors::YELLOW).into(),
            error: rgb(colors::RED).into(),
            info: rgb(colors::BLUE).into(),

            // HTTP method colors
            method_get: rgb(colors::GREEN).into(),
            method_post: rgb(colors::BLUE).into(),
            method_put: rgb(colors::YELLOW).into(),
            method_delete: rgb(colors::RED).into(),
            method_patch: rgb(colors::MAUVE).into(),

            // Status code colors
            status_2xx: rgb(colors::GREEN).into(),
            status_3xx: rgb(colors::BLUE).into(),
            status_4xx: rgb(colors::YELLOW).into(),
            status_5xx: rgb(colors::RED).into(),

            // Interactive
            button_primary: rgb(colors::BLUE).into(),
            button_primary_text: rgb(colors::BASE).into(),
            hover: rgb(colors::SURFACE_0).into(),
        }
    }

    /// Get color for HTTP method
    pub fn method_color(&self, method: &str) -> Hsla {
        match method {
            "GET" => self.method_get,
            "POST" => self.method_post,
            "PUT" => self.method_put,
            "DELETE" => self.method_delete,
            "PATCH" => self.method_patch,
            _ => self.text_muted,
        }
    }

    /// Get color for HTTP status code
    pub fn status_color(&self, status: u16) -> Hsla {
        match status {
            200..=299 => self.status_2xx,
            300..=399 => self.status_3xx,
            400..=499 => self.status_4xx,
            500..=599 => self.status_5xx,
            _ => self.text_muted,
        }
    }

    /// Get transparent color
    pub fn transparent() -> Hsla {
        rgba(colors::TRANSPARENT).into()
    }
}

/// Spacing constants for consistent layout
pub mod spacing {
    use gpui::px;
    use gpui::Pixels;

    pub const XXXS: Pixels = px(2.0);
    pub const XXS: Pixels = px(4.0);
    pub const XS: Pixels = px(8.0);
    pub const SM: Pixels = px(12.0);
    pub const MD: Pixels = px(16.0);
    pub const LG: Pixels = px(24.0);
    pub const XL: Pixels = px(32.0);
    pub const XXL: Pixels = px(48.0);
}

/// Font size constants
pub mod font_size {
    use gpui::px;
    use gpui::Pixels;

    pub const XS: Pixels = px(10.0);
    pub const SM: Pixels = px(11.0);
    pub const MD: Pixels = px(13.0);
    pub const LG: Pixels = px(14.0);
    pub const XL: Pixels = px(16.0);
    pub const XXL: Pixels = px(20.0);
}

/// Component dimensions
pub mod dimensions {
    use gpui::px;
    use gpui::Pixels;

    pub const TITLE_BAR_HEIGHT: Pixels = px(38.0);
    pub const TOOLBAR_HEIGHT: Pixels = px(48.0);
    pub const STATUS_BAR_HEIGHT: Pixels = px(24.0);
    pub const SIDEBAR_WIDTH: Pixels = px(280.0);
    pub const REQUEST_ROW_HEIGHT: Pixels = px(36.0);
    pub const REQUEST_HEADER_HEIGHT: Pixels = px(32.0);
    pub const DETAIL_PANEL_HEIGHT: Pixels = px(300.0);
    pub const TAB_HEIGHT: Pixels = px(36.0);

    pub const TRAFFIC_LIGHT_X: Pixels = px(10.0);
    pub const TRAFFIC_LIGHT_Y: Pixels = px(10.0);
    pub const TRAFFIC_LIGHT_PADDING: Pixels = px(78.0);

    pub const STATUS_INDICATOR_SIZE: Pixels = px(10.0);
    pub const BORDER_RADIUS: Pixels = px(4.0);

    /// Width of the resize handle for the sidebar
    pub const RESIZE_HANDLE_WIDTH: Pixels = px(4.0);
    /// Height of the resize handle for the detail panel
    pub const RESIZE_HANDLE_HEIGHT: Pixels = px(4.0);
}
