//! The design-system widgets: buttons, checkboxes, section labels, progress
//! bars, panels, tabs, dividers. All are lightweight styled-div builders;
//! callers attach `.on_click()` / `.on_hover()` handlers.

use gpui::{
    div, prelude::*, px, relative, ElementId, Hsla,
    InteractiveElement, ParentElement, Pixels, SharedString, Stateful, Styled,
};

use super::theme::Theme;

// ---------------------------------------------------------------------------
// Button
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BtnVariant {
    /// Neutral surface button.
    Default,
    /// Accent-filled primary button ("Execute", "Apply", "Load").
    Accent,
    /// Danger-filled destructive button.
    Danger,
}

pub fn button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    variant: BtnVariant,
    theme: &Theme,
) -> Stateful<gpui::Div> {
    let label = label.into();
    let (bg, fg, hover_bg, pressed_bg, border): (Hsla, Hsla, Hsla, Hsla, Option<Hsla>) =
        match variant {
            BtnVariant::Default => (
                theme.rgb(theme.button_bg),
                theme.rgb(theme.button_fg),
                theme.rgb(theme.button_hover),
                theme.rgb(theme.button_pressed),
                Some(theme.rgba(theme.border, 0.7)),
            ),
            BtnVariant::Accent => (
                theme.rgb(theme.accent),
                if theme.dark { theme.rgb(0xFFFFFF) } else { theme.rgb(0xFFFFFF) },
                theme.lighten(theme.accent, 0.12),
                theme.darken(theme.accent, 0.12),
                None,
            ),
            BtnVariant::Danger => (
                theme.rgb(theme.danger),
                theme.rgb(0xFFFFFF),
                theme.lighten(theme.danger, 0.12),
                theme.darken(theme.danger, 0.12),
                None,
            ),
        };

    let mut btn = div()
        .id(id.into())
        .px_3()
        .h(px(28.))
        .flex()
        .items_center()
        .justify_center()
        .rounded_md()
        .text_size(px(12.5))
        .font_weight(gpui::FontWeight::MEDIUM)
        .bg(bg)
        .text_color(fg)
        .cursor_pointer()
        .hover(move |s| s.bg(hover_bg))
        .active(move |s| s.bg(pressed_bg));
    if let Some(border) = border {
        btn = btn.border_1().border_color(border);
    }
    btn.child(label)
}

pub fn icon_button(
    id: impl Into<ElementId>,
    icon: impl Into<SharedString>,
    tooltip: &'static str,
    theme: &Theme,
) -> Stateful<gpui::Div> {
    let hover = theme.rgba(theme.button_hover, 0.4);
    let pressed = theme.rgba(theme.button_hover, 0.65);
    div()
        .id(id.into())
        .size(px(30.))
        .flex()
        .items_center()
        .justify_center()
        .rounded_md()
        .text_size(px(15.))
        .bg(gpui::transparent_black())
        .text_color(theme.rgb(theme.fg_muted))
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .active(move |s| s.bg(pressed))
        .child(div().child(icon.into()).id(tooltip))
}

// ---------------------------------------------------------------------------
// Labels
// ---------------------------------------------------------------------------

pub fn section_label(text: impl Into<SharedString>, theme: &Theme) -> gpui::Div {
    div()
        .mb_1()
        .px_2()
        .text_size(px(11.0))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(theme.rgb(theme.button_highlight))
        .child(text.into())
}

pub fn caption(text: impl Into<SharedString>, theme: &Theme) -> gpui::Div {
    div()
        .text_size(px(11.0))
        .text_color(theme.rgb(theme.fg_muted))
        .child(text.into())
}

pub fn body_text(text: impl Into<SharedString>, theme: &Theme) -> gpui::Div {
    div()
        .text_size(px(12.5))
        .text_color(theme.rgb(theme.fg_text))
        .child(text.into())
}

// ---------------------------------------------------------------------------
// Checkbox
// ---------------------------------------------------------------------------

pub fn checkbox(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    checked: bool,
    theme: &Theme,
) -> Stateful<gpui::Div> {
    let label = label.into();
    let (box_bg, mark_color) = if checked {
        (theme.rgb(theme.accent), theme.rgb(0xFFFFFF))
    } else {
        (theme.rgb(theme.entry_bg), theme.rgb(theme.fg_muted))
    };
    div()
        .id(id.into())
        .flex()
        .items_center()
        .gap_2()
        .py_1()
        .px_2()
        .rounded_sm()
        .cursor_pointer()
        .hover(move |s| s.bg(theme.rgba(theme.button_hover, 0.25)))
        .child(
            div()
                .size(px(15.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(4.))
                .border_1()
                .border_color(theme.rgba(theme.border, 0.9))
                .bg(box_bg)
                .text_size(px(10.5))
                .text_color(mark_color)
                .child(if checked { "✓" } else { "" }),
        )
        .child(
            div()
                .text_size(px(12.0))
                .text_color(theme.rgb(theme.fg_text))
                .child(label),
        )
}

// ---------------------------------------------------------------------------
// Progress bar
// ---------------------------------------------------------------------------

pub fn progress_bar(
    id: impl Into<ElementId>,
    value: f32,
    text: Option<SharedString>,
    theme: &Theme,
) -> Stateful<gpui::Div> {
    let pct = value.clamp(0.0, 1.0);
    let fill = theme.rgb(theme.button_highlight);
    let text_color = theme.rgb(theme.fg_text);
    div()
        .id(id.into())
        .relative()
        .h(px(20.))
        .w_full()
        .rounded(px(10.))
        .bg(theme.rgb(theme.entry_bg))
        .border_1()
        .border_color(theme.rgba(theme.border, 0.8))
        .overflow_hidden()
        .child(
            div()
                .absolute()
                .left_0()
                .top_0()
                .bottom_0()
                .w(relative(pct))
                .rounded(px(10.))
                .bg(fill),
        )
        .when_some(text, |el, text| {
            el.child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left_0()
                    .right_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(10.5))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(text_color)
                    .child(text),
            )
        })
}

// ---------------------------------------------------------------------------
// Panels / dividers / tabs
// ---------------------------------------------------------------------------

pub fn panel(theme: &Theme) -> gpui::Div {
    div()
        .rounded_lg()
        .bg(theme.rgb(theme.bg_elevated))
        .border_1()
        .border_color(theme.rgba(theme.border, 0.8))
}

#[allow(dead_code)]
pub fn h_divider(theme: &Theme) -> gpui::Div {
    div().h(px(0.)).w_full().border_t_1().border_color(theme.rgba(theme.border, 0.6))
}

#[allow(dead_code)]
pub fn v_divider(theme: &Theme) -> gpui::Div {
    div().w(px(0.).max(px(1.))).h_full().border_l_1().border_color(theme.rgba(theme.border, 0.6))
}

/// One tab of the sidebar's tab bar. `active` selects the highlight style.
pub fn tab(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    active: bool,
    theme: &Theme,
) -> Stateful<gpui::Div> {
    let (bg, fg) = if active {
        (theme.rgba(theme.button_highlight, 0.18), theme.rgb(theme.button_highlight))
    } else {
        (gpui::transparent_black(), theme.rgb(theme.fg_muted))
    };
    let active_bg = theme.rgba(theme.button_highlight, 0.18);
    let inactive_hover = theme.rgba(theme.button_hover, 0.3);
    div()
        .id(id.into())
        .flex_1()
        .h(px(30.))
        .flex()
        .items_center()
        .justify_center()
        .rounded_md()
        .text_size(px(12.0))
        .font_weight(if active { gpui::FontWeight::SEMIBOLD } else { gpui::FontWeight::MEDIUM })
        .bg(bg)
        .text_color(fg)
        .cursor_pointer()
        .hover(move |s| s.bg(if active { active_bg } else { inactive_hover }))
        .child(label.into())
}

/// Collapsible section header. Caller wires the click; `expanded` picks the
/// chevron direction.
pub fn collapsible_header(
    id: impl Into<ElementId>,
    title: &'static str,
    expanded: bool,
    theme: &Theme,
) -> Stateful<gpui::Div> {
    let hover = theme.rgba(theme.button_hover, 0.3);
    div()
        .id(id.into())
        .flex()
        .items_center()
        .gap_1p5()
        .h(px(26.))
        .px_1()
        .rounded_sm()
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .child(
            div()
                .text_size(px(9.0))
                .text_color(theme.rgb(theme.fg_muted))
                .child(if expanded { "▼" } else { "▶" }),
        )
        .child(
            div()
                .text_size(px(11.0))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(theme.rgb(theme.fg_highlight))
                .child(title),
        )
}

/// A rounded color swatch (class color chip).
pub fn color_swatch(color: (u8, u8, u8), size: Pixels, theme: &Theme) -> gpui::Div {
    let (r, g, b) = color;
    div()
        .size(size)
        .flex_none()
        .rounded(px(3.0))
        .bg(gpui::rgb((r as u32) << 16 | (g as u32) << 8 | b as u32))
        .border_1()
        .border_color(theme.rgba(0x000000, if theme.dark { 0.5 } else { 0.25 }))
}

// ---------------------------------------------------------------------------
// Theme color helpers
// ---------------------------------------------------------------------------

impl Theme {
    /// Lighten toward white by `amount` (0.0–1.0).
    pub fn lighten(&self, color: u32, amount: f32) -> Hsla {
        let r = ((color >> 16) & 0xFF) as f32;
        let g = ((color >> 8) & 0xFF) as f32;
        let b = (color & 0xFF) as f32;
        let f = |c: f32| c + (255.0 - c) * amount;
        let mut h: Hsla = gpui::rgb(
            ((f(r) as u32) << 16) | ((f(g) as u32) << 8) | (f(b) as u32),
        )
        .into();
        h.a = 1.0;
        h
    }

    /// Darken toward black by `amount` (0.0–1.0).
    pub fn darken(&self, color: u32, amount: f32) -> Hsla {
        let r = ((color >> 16) & 0xFF) as f32;
        let g = ((color >> 8) & 0xFF) as f32;
        let b = (color & 0xFF) as f32;
        let f = |c: f32| c * (1.0 - amount);
        let mut h: Hsla = gpui::rgb(
            ((f(r) as u32) << 16) | ((f(g) as u32) << 8) | (f(b) as u32),
        )
        .into();
        h.a = 1.0;
        h
    }
}

