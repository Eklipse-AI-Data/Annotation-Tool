//! Monochrome line-icon set (Lucide paths, 24×24, stroke 2) served through an
//! in-memory GPUI AssetSource. Icons render tinted by `text_color`, so they
//! adapt to the theme automatically.

use std::borrow::Cow;
use std::sync::OnceLock;

use gpui::{svg, SharedString, Styled};

fn icon_bytes(inner: &str) -> &'static [u8] {
    let svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="#ffffff" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">{inner}</svg>"##
    )
    .into_bytes();
    Box::leak(svg.into_boxed_slice())
}

macro_rules! icons {
    ($($name:literal => $inner:expr),+ $(,)?) => {
        {{
            let mut map = std::collections::HashMap::<&'static str, &'static [u8]>::new();
            $( map.insert($name, icon_bytes($inner)); )+
            map
        }}
    };
}

fn table() -> &'static std::collections::HashMap<&'static str, &'static [u8]> {
    static TABLE: OnceLock<std::collections::HashMap<&'static str, &'static [u8]>> = OnceLock::new();
    TABLE.get_or_init(|| {
        icons! {
            "panel-left" => r#"<rect width="18" height="18" x="3" y="3" rx="2"/><path d="M9 3v18"/>"#,
            "undo" => r#"<path d="M9 14 4 9l5-5"/><path d="M4 9h10.5a5.5 5.5 0 0 1 5.5 5.5 5.5 5.5 0 0 1-5.5 5.5H11"/>"#,
            "redo" => r#"<path d="m15 14 5-5-5-5"/><path d="M20 9H9.5A5.5 5.5 0 0 0 4 14.5 5.5 5.5 0 0 0 9.5 20H13"/>"#,
            "check-circle" => r#"<path d="M21.801 10A10 10 0 1 1 17 3.335"/><path d="m9 11 3 3L22 4"/>"#,
            "keyboard" => r#"<rect width="20" height="16" x="2" y="4" rx="2"/><path d="M6 8h.01"/><path d="M10 8h.01"/><path d="M14 8h.01"/><path d="M18 8h.01"/><path d="M8 12h.01"/><path d="M12 12h.01"/><path d="M16 12h.01"/><path d="M7 16h10"/>"#,
            "film" => r#"<rect width="18" height="18" x="3" y="3" rx="2"/><path d="M7 3v18"/><path d="M3 7.5h4"/><path d="M3 12h18"/><path d="M3 16.5h4"/><path d="M17 3v18"/><path d="M17 7.5h4"/><path d="M17 16.5h4"/>"#,
            "list" => r#"<path d="M3 6h.01"/><path d="M8 6h13"/><path d="M3 12h.01"/><path d="M8 12h13"/><path d="M3 18h.01"/><path d="M8 18h13"/>"#,
            "chevron-down" => r#"<path d="m6 9 6 6 6-6"/>"#,
            "chevron-right" => r#"<path d="m9 18 6-6-6-6"/>"#,
            "folder-open" => r#"<path d="m6 14 1.5-2.9A2 2 0 0 1 9.24 10H20a2 2 0 0 1 1.94 2.5l-1.54 6a2 2 0 0 1-1.95 1.5H4a2 2 0 0 1-2-2V5c0-1.1.9-2 2-2h3.93a2 2 0 0 1 1.66.9l.82 1.2a2 2 0 0 0 1.66.9H18a2 2 0 0 1 2 2v2"/>"#,
            "database" => r#"<ellipse cx="12" cy="5" rx="9" ry="3"/><path d="M3 5v14a9 3 0 0 0 18 0V5"/><path d="M3 12a9 3 0 0 0 18 0"/>"#,
            "history" => r#"<path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8"/><path d="M3 3v5h5"/><path d="M12 7v5l4 2"/>"#,
            "sliders" => r#"<path d="M21 4h-7"/><path d="M10 4H3"/><path d="M21 12h-9"/><path d="M8 12H3"/><path d="M21 20h-5"/><path d="M12 20H3"/><path d="M14 2v4"/><path d="M8 10v4"/><path d="M16 18v4"/>"#,
            "x" => r#"<path d="M18 6 6 18"/><path d="m6 6 12 12"/>"#,
            "image" => r#"<rect width="18" height="18" x="3" y="3" rx="2"/><circle cx="9" cy="9" r="2"/><path d="m21 15-3.1-3.1a2 2 0 0 0-2.8 0L6 21"/>"#,
            "check" => r#"<path d="M20 6 9 17l-5-5"/>"#,
            "search" => r#"<circle cx="11" cy="11" r="8"/><path d="m21 21-4.3-4.3"/>"#,
            "filter" => r#"<polygon points="22 3 2 3 10 12.46 10 19 14 21 14 12.46 22 3"/>"#,
            "eye" => r#"<path d="M2.06 12.35a1 1 0 0 1 0-.7 10.75 10.75 0 0 1 19.88 0 1 1 0 0 1 0 .7 10.75 10.75 0 0 1-19.88 0"/><circle cx="12" cy="12" r="3"/>"#,
            "copy" => r#"<rect width="14" height="14" x="8" y="8" rx="2"/><path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"/>"#,
            "clipboard" => r#"<rect width="8" height="4" x="8" y="2" rx="1"/><path d="M16 4h2a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h2"/>"#,
            "layers" => r#"<path d="m12.83 2.18a2 2 0 0 0-1.66 0L2.6 6.08a1 1 0 0 0 0 1.83l8.58 3.91a2 2 0 0 0 1.66 0l8.58-3.9a1 1 0 0 0 0-1.83Z"/><path d="m22 17.65-9.17 4.16a2 2 0 0 1-1.66 0L2 17.65"/><path d="m22 12.65-9.17 4.16a2 2 0 0 1-1.66 0L2 12.65"/>"#,
            "trash" => r#"<path d="M3 6h18"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6"/><path d="M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/>"#,
            "warning" => r#"<path d="m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3"/><path d="M12 9v4"/><path d="M12 17h.01"/>"#,
            "info" => r#"<circle cx="12" cy="12" r="10"/><path d="M12 16v-4"/><path d="M12 8h.01"/>"#,
            "error" => r#"<circle cx="12" cy="12" r="10"/><path d="m15 9-6 6"/><path d="m9 9 6 6"/>"#,
        }
    })
}

/// GPUI asset source serving the embedded icon table.
pub struct IconAssets;

impl gpui::AssetSource for IconAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>, anyhow::Error> {
        Ok(table().get(path).map(|bytes| Cow::Borrowed(*bytes)))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>, anyhow::Error> {
        Ok(table()
            .keys()
            .filter(|k| k.starts_with(path))
            .map(|k| SharedString::from(k.to_string()))
            .collect())
    }
}

/// Renders an icon by name at the given pixel size, tinted by `color`.
pub fn icon(name: &'static str, size: f32, color: gpui::Hsla) -> gpui::Svg {
    svg()
        .path(name)
        .size(gpui::px(size))
        .text_color(color)
}
