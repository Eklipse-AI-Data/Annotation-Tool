//! The six themes, refined for GPUI. Each keeps the identity of its Tkinter
//! predecessor (hue family, light/dark class) with a modern surface
//! hierarchy: deepest canvas, elevated sidebars, translucent borders, one
//! clear accent. Class box colors remain generated per class id (see
//! `classes::generate_color`) so datasets look identical across tools.

use gpui::Hsla;

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub name: &'static str,
    pub dark: bool,

    /// Canvas / window background (deepest surface).
    pub bg_main: u32,
    /// Sidebars, filmstrip, tooltips.
    pub bg_sidebar: u32,
    /// Toolbar, popovers, dialogs (most elevated).
    pub bg_elevated: u32,

    pub fg_text: u32,
    pub fg_highlight: u32,
    pub fg_muted: u32,

    /// Focus rings, primary buttons, emphasized borders.
    pub accent: u32,
    /// List-selection background.
    pub selection: u32,
    pub border: u32,

    pub button_bg: u32,
    pub button_fg: u32,
    pub button_hover: u32,
    pub button_pressed: u32,

    pub entry_bg: u32,
    pub entry_fg: u32,
    pub list_bg: u32,
    pub list_hover: u32,

    /// Active tab, pressed button fill, progress fill, section headers.
    pub button_highlight: u32,

    // Status colors
    pub success: u32,
    pub warning: u32,
    pub error: u32,
    pub info: u32,
    pub danger: u32,

    pub crosshair: u32,
    pub selected_box: u32,
}

impl Theme {
    pub fn rgb(&self, c: u32) -> Hsla {
        gpui::rgb(c).into()
    }

    pub fn rgba(&self, c: u32, alpha: f32) -> Hsla {
        let mut hsla: Hsla = gpui::rgb(c).into();
        hsla.a = alpha;
        hsla
    }
}

const fn dark_status() -> (u32, u32, u32, u32, u32, u32, u32) {
    // success, warning, error, info, danger, crosshair, selected_box
    (0x4ADE80, 0xFFD700, 0xFF5C5C, 0x4CC9F0, 0x882222, 0xFFFFFF, 0xFFFFFF)
}

const fn light_status() -> (u32, u32, u32, u32, u32, u32, u32) {
    (0x15803D, 0xB45309, 0xDC2626, 0x0369A1, 0xB91C1C, 0x1E293B, 0x0F172A)
}

pub const THEMES: [Theme; 6] = [
    // ------------------------------------------------------------------
    // Midnight Glass — default. Deep indigo glass with an electric accent.
    // ------------------------------------------------------------------
    Theme {
        name: "Midnight Glass",
        dark: true,
        bg_main: 0x0B0A1F,
        bg_sidebar: 0x141330,
        bg_elevated: 0x1B1A3E,
        fg_text: 0xE2E2F0,
        fg_highlight: 0xFFFFFF,
        fg_muted: 0x9C9CB8,
        accent: 0x4361EE,
        selection: 0x4834D4,
        border: 0x2B2954,
        button_bg: 0x272450,
        button_fg: 0xE8E8F5,
        button_hover: 0x322F63,
        button_pressed: 0x3A3674,
        entry_bg: 0x101430,
        entry_fg: 0xFFFFFF,
        list_bg: 0x101430,
        list_hover: 0x1D1C44,
        button_highlight: 0x4CC9F0,
        success: dark_status().0,
        warning: dark_status().1,
        error: dark_status().2,
        info: dark_status().3,
        danger: dark_status().4,
        crosshair: dark_status().5,
        selected_box: dark_status().6,
    },
    // ------------------------------------------------------------------
    // Anthropic — warm paper light theme with a rust accent.
    // ------------------------------------------------------------------
    Theme {
        name: "Anthropic",
        dark: false,
        bg_main: 0xDCD9CE,
        bg_sidebar: 0xF0EEE6,
        bg_elevated: 0xF7F6F1,
        fg_text: 0x141413,
        fg_highlight: 0x000000,
        fg_muted: 0x6B6862,
        accent: 0xC15F3C,
        selection: 0xE7C4B5,
        border: 0xD6D3D1,
        button_bg: 0xE9E5DB,
        button_fg: 0x141413,
        button_hover: 0xE0DBCF,
        button_pressed: 0xD6D0C2,
        entry_bg: 0xFAF9F5,
        entry_fg: 0x141413,
        list_bg: 0xFAF9F5,
        list_hover: 0xEFEBE0,
        button_highlight: 0xD97757,
        success: light_status().0,
        warning: light_status().1,
        error: light_status().2,
        info: light_status().3,
        danger: light_status().4,
        crosshair: light_status().5,
        selected_box: light_status().6,
    },
    // ------------------------------------------------------------------
    // Emerald
    // ------------------------------------------------------------------
    Theme {
        name: "Emerald",
        dark: true,
        bg_main: 0x04120F,
        bg_sidebar: 0x082019,
        bg_elevated: 0x0C2B22,
        fg_text: 0xDCFCE7,
        fg_highlight: 0xFFFFFF,
        fg_muted: 0x84AF9C,
        accent: 0x059669,
        selection: 0x10B981,
        border: 0x123B2E,
        button_bg: 0x0C2A21,
        button_fg: 0xECFDF5,
        button_hover: 0x113A2D,
        button_pressed: 0x155039,
        entry_bg: 0x052B1F,
        entry_fg: 0xFFFFFF,
        list_bg: 0x052B1F,
        list_hover: 0x0B3829,
        button_highlight: 0x34D399,
        success: dark_status().0,
        warning: dark_status().1,
        error: dark_status().2,
        info: dark_status().3,
        danger: dark_status().4,
        crosshair: dark_status().5,
        selected_box: dark_status().6,
    },
    // ------------------------------------------------------------------
    // Ruby
    // ------------------------------------------------------------------
    Theme {
        name: "Ruby",
        dark: true,
        bg_main: 0x140404,
        bg_sidebar: 0x260808,
        bg_elevated: 0x330C0C,
        fg_text: 0xFEE2E2,
        fg_highlight: 0xFFFFFF,
        fg_muted: 0xB08484,
        accent: 0xDC2626,
        selection: 0xEF4444,
        border: 0x4A1010,
        button_bg: 0x3B0D0D,
        button_fg: 0xFEE2E2,
        button_hover: 0x4A1414,
        button_pressed: 0x5C1A1A,
        entry_bg: 0x3A0A0A,
        entry_fg: 0xFFFFFF,
        list_bg: 0x3A0A0A,
        list_hover: 0x471010,
        button_highlight: 0xF87171,
        success: dark_status().0,
        warning: dark_status().1,
        error: dark_status().2,
        info: dark_status().3,
        danger: dark_status().4,
        crosshair: dark_status().5,
        selected_box: dark_status().6,
    },
    // ------------------------------------------------------------------
    // Amber
    // ------------------------------------------------------------------
    Theme {
        name: "Amber",
        dark: true,
        bg_main: 0x140E02,
        bg_sidebar: 0x241705,
        bg_elevated: 0x2E1E07,
        fg_text: 0xFEF3C7,
        fg_highlight: 0xFFFFFF,
        fg_muted: 0xAE9A6B,
        accent: 0xD97706,
        selection: 0xF59E0B,
        border: 0x4A3308,
        button_bg: 0x3B2A06,
        button_fg: 0xFEF3C7,
        button_hover: 0x4A3509,
        button_pressed: 0x5B420C,
        entry_bg: 0x3D2A05,
        entry_fg: 0xFFFFFF,
        list_bg: 0x3D2A05,
        list_hover: 0x4A3409,
        button_highlight: 0xFBBF24,
        success: dark_status().0,
        warning: dark_status().1,
        error: dark_status().2,
        info: dark_status().3,
        danger: dark_status().4,
        crosshair: dark_status().5,
        selected_box: dark_status().6,
    },
    // ------------------------------------------------------------------
    // Frost — light, cool blue.
    // ------------------------------------------------------------------
    Theme {
        name: "Frost",
        dark: false,
        bg_main: 0xE8EDF5,
        bg_sidebar: 0xF1F5F9,
        bg_elevated: 0xF8FAFC,
        fg_text: 0x1E293B,
        fg_highlight: 0x0F172A,
        fg_muted: 0x64748B,
        accent: 0x2563EB,
        selection: 0xBFD7FE,
        border: 0xCBD5E1,
        button_bg: 0xE2E8F0,
        button_fg: 0x1E293B,
        button_hover: 0xD8E0EC,
        button_pressed: 0xC9D4E4,
        entry_bg: 0xFFFFFF,
        entry_fg: 0x1E293B,
        list_bg: 0xFFFFFF,
        list_hover: 0xEDF2FA,
        button_highlight: 0x2563EB,
        success: light_status().0,
        warning: light_status().1,
        error: light_status().2,
        info: light_status().3,
        danger: light_status().4,
        crosshair: light_status().5,
        selected_box: light_status().6,
    },
];

pub fn theme_by_name(name: &str) -> &'static Theme {
    THEMES
        .iter()
        .find(|t| t.name.eq_ignore_ascii_case(name))
        .unwrap_or(&THEMES[0])
}

/// GPUI global wrapper so every view can reach the active theme.
pub struct ThemeState {
    pub theme: &'static Theme,
}

impl gpui::Global for ThemeState {}

impl ThemeState {
    pub fn init(cx: &mut gpui::App, name: &str) {
        cx.set_global(ThemeState {
            theme: theme_by_name(name),
        });
    }

    pub fn set(cx: &mut gpui::App, name: &str) {
        cx.set_global(ThemeState {
            theme: theme_by_name(name),
        });
    }

    pub fn current(cx: &gpui::App) -> &'static Theme {
        cx.try_global::<ThemeState>()
            .map(|s| s.theme)
            .unwrap_or(&THEMES[0])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn six_themes_resolve() {
        assert_eq!(theme_by_name("Midnight Glass").name, "Midnight Glass");
        assert_eq!(theme_by_name("anthropic").name, "Anthropic");
        assert_eq!(theme_by_name("nope").name, "Midnight Glass"); // fallback
        assert_eq!(THEMES.len(), 6);
    }
}
