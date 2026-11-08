//! Two focused themes — Light and Dark — in restrained, neutral palettes
//! (ivory warm-gray light, near-black dark) with a single coral accent,
//! following the design language of top-tier AI desktop apps. Class box
//! colors remain generated per class id (see `classes::generate_color`) so
//! datasets look identical across tools.

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
    /// Hover/active surface for rows and quiet buttons.
    pub bg_hover: u32,

    pub fg_text: u32,
    pub fg_highlight: u32,
    pub fg_muted: u32,

    /// The single accent: focus rings, primary buttons, progress fill.
    pub accent: u32,
    /// Accent hover (10% lighter).
    pub accent_hover: u32,
    /// List-selection background (neutral, not accent).
    pub selection: u32,
    pub border: u32,

    pub button_bg: u32,
    pub button_hover: u32,

    pub entry_bg: u32,
    pub entry_fg: u32,
    pub list_bg: u32,
    pub list_hover: u32,

    // Status colors (desaturated, restrained)
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

    /// Accent-tinted selection background for list rows.
    pub fn selection_bg(&self) -> Hsla {
        self.rgba(self.accent, if self.dark { 0.16 } else { 0.12 })
    }

    pub fn rgba(&self, c: u32, alpha: f32) -> Hsla {
        let mut hsla: Hsla = gpui::rgb(c).into();
        hsla.a = alpha;
        hsla
    }
}

const fn dark_status() -> (u32, u32, u32, u32, u32, u32, u32) {
    (0x4CB782, 0xD8B04A, 0xE5484D, 0x6CB6FF, 0xE5484D, 0xFFFFFF, 0xFFFFFF)
}

const fn light_status() -> (u32, u32, u32, u32, u32, u32, u32) {
    (0x2E7D5B, 0x9A6B1F, 0xCE2B31, 0x3B7DD8, 0xCE2B31, 0x262626, 0x111111)
}

pub const THEMES: [Theme; 2] = [
    // ------------------------------------------------------------------
    // Dark — near-black neutrals, single coral accent.
    // ------------------------------------------------------------------
    Theme {
        name: "Dark",
        dark: true,
        bg_main: 0x0E0E10,
        bg_sidebar: 0x151517,
        bg_elevated: 0x1B1B1E,
        bg_hover: 0x26262A,
        fg_text: 0xF0F0F2,
        fg_highlight: 0xFFFFFF,
        fg_muted: 0xA8A8AF,
        accent: 0xE8825E,
        accent_hover: 0xF2946F,
        selection: 0x2E2E33,
        border: 0x2B2B2F,
        button_bg: 0x1F1F22,
        button_hover: 0x2A2A2E,
        entry_bg: 0x111113,
        entry_fg: 0xEDEDEF,
        list_bg: 0x121214,
        list_hover: 0x232327,
        success: dark_status().0,
        warning: dark_status().1,
        error: dark_status().2,
        info: dark_status().3,
        danger: dark_status().4,
        crosshair: dark_status().5,
        selected_box: dark_status().6,
    },
    // ------------------------------------------------------------------
    // Light — warm ivory neutrals (Claude-like), single rust accent.
    // ------------------------------------------------------------------
    Theme {
        name: "Light",
        dark: false,
        bg_main: 0xFAF9F5,
        bg_sidebar: 0xF2F0E9,
        bg_elevated: 0xFFFFFF,
        bg_hover: 0xEAE7DE,
        fg_text: 0x27261F,
        fg_highlight: 0x111110,
        fg_muted: 0x7A786F,
        accent: 0xC96442,
        accent_hover: 0xD67550,
        selection: 0xE9E5DA,
        border: 0xDFDCD2,
        button_bg: 0xFFFFFF,
        button_hover: 0xEFEEE8,
        entry_bg: 0xFFFFFF,
        entry_fg: 0x27261F,
        list_bg: 0xFFFFFF,
        list_hover: 0xF1EFE8,
        success: light_status().0,
        warning: light_status().1,
        error: light_status().2,
        info: light_status().3,
        danger: light_status().4,
        crosshair: light_status().5,
        selected_box: light_status().6,
    },
];

/// Resolves a theme name; legacy theme names from the Python tool map onto
/// the two new themes so old sessions keep a sensible appearance.
pub fn theme_by_name(name: &str) -> &'static Theme {
    match name.to_ascii_lowercase().as_str() {
        "light" | "anthropic" | "frost" => &THEMES[1],
        _ => &THEMES[0], // "dark", "midnight glass", "emerald", "ruby", "amber", unknown
    }
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
    fn legacy_names_map_onto_new_themes() {
        assert_eq!(theme_by_name("Midnight Glass").name, "Dark");
        assert_eq!(theme_by_name("Emerald").name, "Dark");
        assert_eq!(theme_by_name("Anthropic").name, "Light");
        assert_eq!(theme_by_name("Frost").name, "Light");
        assert_eq!(theme_by_name("Light").name, "Light");
        assert_eq!(theme_by_name("Dark").name, "Dark");
        assert_eq!(theme_by_name("unknown").name, "Dark");
    }
}
