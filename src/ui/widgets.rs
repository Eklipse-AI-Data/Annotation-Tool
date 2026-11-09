//! The design-system widgets: buttons, checkboxes, section labels, progress
//! bars, panels, tabs, dividers. Quiet, neutral, generously spaced — one
//! accent color used sparingly.

use gpui::{
    div, prelude::*, px, Hsla, InteractiveElement, IntoElement, ParentElement, Pixels,
    SharedString, Stateful, Styled,
};

use super::icons::icon;
use super::theme::Theme;

// ---------------------------------------------------------------------------
// Button
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BtnVariant {
    /// Quiet outline button for secondary actions.
    Default,
    /// Accent-filled primary button ("Execute", "Apply", "Save & Close").
    Accent,
    /// Destructive outline button (danger text + border).
    Danger,
}

pub fn button(
    id: impl Into<gpui::ElementId>,
    label: impl Into<SharedString>,
    variant: BtnVariant,
    theme: &Theme,
) -> Stateful<gpui::Div> {
    button_with_icon(id, label, None, variant, theme)
}

/// Button with an optional leading icon (icon name from `crate::ui::icons`).
pub fn button_with_icon(
    id: impl Into<gpui::ElementId>,
    label: impl Into<SharedString>,
    leading_icon: Option<&'static str>,
    variant: BtnVariant,
    theme: &Theme,
) -> Stateful<gpui::Div> {
    let label = label.into();
    // Ghost-first design: quiet actions are borderless text on hover pills.
    let (bg, fg, hover_bg): (Hsla, Hsla, Hsla) = match variant {
        BtnVariant::Default => (
            gpui::transparent_black(),
            theme.rgba(theme.fg_text, 0.88),
            theme.rgba(theme.fg_muted, 0.13),
        ),
        BtnVariant::Accent => (
            theme.rgb(theme.accent),
            gpui::rgb(0xFFFFFF).into(),
            theme.rgb(theme.accent_hover),
        ),
        BtnVariant::Danger => (
            gpui::transparent_black(),
            theme.rgb(theme.danger),
            theme.rgba(theme.danger, 0.12),
        ),
    };

    let mut btn = div()
        .id(id.into())
        .px(px(12.))
        .h(px(32.))
        .flex()
        .items_center()
        .justify_center()
        .gap(px(7.))
        .rounded(px(7.))
        .text_size(px(12.5))
        .font_weight(gpui::FontWeight::MEDIUM)
        .bg(bg)
        .text_color(fg)
        .cursor_pointer()
        .hover(move |s| s.bg(hover_bg))
        .active(move |s| s.bg(hover_bg));
    if let Some(name) = leading_icon {
        btn = btn.child(icon(name, 14.0, fg));
    }
    btn.child(label)
}

/// Square toolbar icon button (30×30) with a monochrome SVG icon.
pub fn icon_button(
    id: impl Into<gpui::ElementId>,
    icon_name: &'static str,
    active: bool,
    theme: &Theme,
) -> Stateful<gpui::Div> {
    let fg = if active {
        theme.rgb(theme.fg_highlight)
    } else {
        theme.rgba(theme.fg_text, 0.6)
    };
    let hover = theme.rgba(theme.fg_muted, 0.12);
    let hover_fg = theme.rgb(theme.fg_text);
    let active_bg = theme.selection_bg();
    div()
        .id(id.into())
        .size(px(30.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(7.))
        .when(active, |el| el.bg(active_bg))
        .text_color(fg)
        .cursor_pointer()
        .hover(move |s| s.bg(hover).text_color(hover_fg))
        .active(move |s| s.bg(hover))
        .child(icon(icon_name, 16.0, fg))
}

/// Thin vertical toolbar separator.
pub fn toolbar_divider(theme: &Theme) -> gpui::Div {
    div()
        .w(px(1.))
        .h(px(18.))
        .mx(px(4.))
        .flex_none()
        .bg(theme.rgba(theme.border, 0.9))
}

// ---------------------------------------------------------------------------
// Labels
// ---------------------------------------------------------------------------

pub fn caption(text: impl Into<SharedString>, theme: &Theme) -> gpui::Div {
    div()
        .text_size(px(11.5))
        .line_height(px(16.0))
        .text_color(theme.rgb(theme.fg_muted))
        .child(text.into())
}

pub fn body_text(text: impl Into<SharedString>, theme: &Theme) -> gpui::Div {
    div()
        .text_size(px(12.5))
        .text_color(theme.rgb(theme.fg_text))
        .child(text.into())
}

/// Keyboard-key chip, e.g. for shortcut hints.
pub fn kbd(text: impl Into<SharedString>, theme: &Theme) -> gpui::Div {
    div()
        .flex_none()
        .px(px(5.))
        .py(px(1.))
        .rounded(px(4.))
        .bg(theme.rgb(theme.bg_hover))
        .border_1()
        .border_color(theme.rgba(theme.border, 0.8))
        .text_size(px(10.5))
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(theme.rgb(theme.fg_muted))
        .child(text.into())
}

// ---------------------------------------------------------------------------
// Checkbox
// ---------------------------------------------------------------------------

pub fn checkbox(
    id: impl Into<gpui::ElementId>,
    label: impl Into<SharedString>,
    checked: bool,
    theme: &Theme,
) -> Stateful<gpui::Div> {
    let label = label.into();
    let (box_bg, mark) = if checked {
        (
            theme.rgb(theme.accent),
            icon("check", 11.0, gpui::rgb(0xFFFFFF).into()).into_any_element(),
        )
    } else {
        (theme.rgb(theme.entry_bg), div().into_any_element())
    };
    let hover = theme.rgba(theme.bg_hover, 0.6);
    div()
        .id(id.into())
        .flex()
        .items_center()
        .gap(px(8.))
        .py(px(4.))
        .px(px(6.))
        .mx(px(-6.))
        .rounded(px(6.))
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .child(
            div()
                .size(px(16.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(4.))
                .border_1()
                .border_color(if checked {
                    theme.rgb(theme.accent)
                } else {
                    theme.rgba(theme.fg_muted, 0.5)
                })
                .bg(box_bg)
                .child(mark),
        )
        .child(
            div()
                .text_size(px(12.))
                .line_height(px(16.))
                .text_color(theme.rgb(theme.fg_text))
                .child(label),
        )
}

// ---------------------------------------------------------------------------
// Progress bar (slim, unlabeled — pair with a caption)
// ---------------------------------------------------------------------------

pub fn progress_bar(
    id: impl Into<gpui::ElementId>,
    value: f32,
    theme: &Theme,
) -> Stateful<gpui::Div> {
    let pct = value.clamp(0.0, 1.0);
    div()
        .id(id.into())
        .relative()
        .h(px(6.))
        .w_full()
        .rounded(px(3.))
        .bg(theme.rgba(theme.fg_muted, 0.18))
        .overflow_hidden()
        .child(
            div()
                .absolute()
                .left_0()
                .top_0()
                .bottom_0()
                .w(gpui::relative(pct))
                .rounded(px(3.))
                .bg(theme.rgb(theme.accent)),
        )
}

// ---------------------------------------------------------------------------
// Panels / dividers / tabs
// ---------------------------------------------------------------------------

pub fn panel(theme: &Theme) -> gpui::Div {
    // `flex()` is required: callers chain `.flex_col()`, which only sets the
    // direction — without a flex display, `flex_1`/`min_h_0`/scrolling
    // children never get constrained and spill out of the panel.
    div()
        .flex()
        .rounded(px(12.))
        .bg(theme.rgb(theme.bg_elevated))
        .border_1()
        .border_color(theme.rgba(theme.border, 0.9))
        .shadow_lg()
}

/// Opaque full-width 1px line between stacked sections.
pub fn h_divider(theme: &Theme) -> gpui::Div {
    div().h(px(1.)).w_full().flex_none().bg(theme.rgb(theme.divider))
}

/// Opaque 1px vertical line (status-bar segments etc.).
pub fn v_divider(theme: &Theme) -> gpui::Div {
    div().w(px(1.)).h(px(14.)).flex_none().bg(theme.rgb(theme.divider))
}

/// Header band at the top of a docked pane section: uppercase title on the
/// chrome tone with an opaque divider below, optional trailing content.
pub fn pane_header(
    id: impl Into<gpui::ElementId>,
    title: impl Into<SharedString>,
    trailing: Option<gpui::AnyElement>,
    theme: &Theme,
) -> Stateful<gpui::Div> {
    pane_header_inner(id, title.into(), None, trailing, theme)
}

/// Collapsible [`pane_header`] with a leading chevron; the whole band is the
/// click target.
pub fn pane_header_collapsible(
    id: impl Into<gpui::ElementId>,
    title: impl Into<SharedString>,
    expanded: bool,
    trailing: Option<gpui::AnyElement>,
    theme: &Theme,
) -> Stateful<gpui::Div> {
    let hover = theme.rgba(theme.fg_muted, 0.08);
    pane_header_inner(id, title.into(), Some(expanded), trailing, theme)
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
}

fn pane_header_inner(
    id: impl Into<gpui::ElementId>,
    title: SharedString,
    expanded: Option<bool>,
    trailing: Option<gpui::AnyElement>,
    theme: &Theme,
) -> Stateful<gpui::Div> {
    let muted = theme.rgb(theme.fg_muted);
    let mut left = div().flex().items_center().gap(px(6.)).min_w_0();
    if let Some(expanded) = expanded {
        left = left.child(icon(if expanded { "chevron-down" } else { "chevron-right" }, 12.0, muted));
    }
    left = left.child(
        div()
            .text_size(px(10.5))
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .text_color(theme.rgba(theme.fg_text, 0.7))
            .child(title.to_uppercase()),
    );
    div()
        .id(id.into())
        .h(px(30.))
        .w_full()
        .flex_none()
        .flex()
        .items_center()
        .justify_between()
        .px(px(12.))
        .bg(theme.rgb(theme.bg_chrome))
        .border_b_1()
        .border_color(theme.rgb(theme.divider))
        .child(left)
        .when_some(trailing, |el, t| {
            el.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_size(px(11.))
                    .text_color(muted)
                    .child(t),
            )
        })
}

/// Small borderless text button for pane-header trailing slots.
pub fn header_link(
    id: impl Into<gpui::ElementId>,
    label: impl Into<SharedString>,
    theme: &Theme,
) -> Stateful<gpui::Div> {
    let hover = theme.rgba(theme.fg_muted, 0.14);
    div()
        .id(id.into())
        .px(px(6.))
        .h(px(20.))
        .flex()
        .items_center()
        .rounded(px(4.))
        .text_size(px(11.))
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(theme.rgb(theme.fg_text))
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .child(label.into())
}

/// One tab of a dock tab strip: flush, equal width, accent underline when
/// active.
pub fn strip_tab(
    id: impl Into<gpui::ElementId>,
    label: impl Into<SharedString>,
    active: bool,
    theme: &Theme,
) -> Stateful<gpui::Div> {
    let hover = theme.rgba(theme.fg_muted, 0.08);
    div()
        .id(id.into())
        .flex_1()
        .h_full()
        .flex()
        .items_center()
        .justify_center()
        .border_b_2()
        .border_color(if active { theme.rgb(theme.accent) } else { gpui::transparent_black() })
        .text_size(px(12.))
        .font_weight(if active { gpui::FontWeight::SEMIBOLD } else { gpui::FontWeight::MEDIUM })
        .text_color(if active { theme.rgb(theme.fg_highlight) } else { theme.rgb(theme.fg_muted) })
        .cursor_pointer()
        .when(!active, move |el| el.hover(move |s| s.bg(hover)))
        .child(label.into())
}

/// One tab of a segmented control.
pub fn tab(
    id: impl Into<gpui::ElementId>,
    label: impl Into<SharedString>,
    active: bool,
    theme: &Theme,
) -> Stateful<gpui::Div> {
    let (bg, fg) = if active {
        (theme.rgb(theme.bg_elevated), theme.rgb(theme.fg_text))
    } else {
        (gpui::transparent_black(), theme.rgb(theme.fg_muted))
    };
    let active_bg = theme.rgb(theme.bg_elevated);
    let inactive_hover = theme.rgba(theme.fg_muted, 0.1);
    div()
        .id(id.into())
        .flex_1()
        .min_w_0()
        .px(px(10.))
        .h(px(28.))
        .flex()
        .items_center()
        .justify_center()
        .whitespace_nowrap()
        .rounded(px(6.))
        .text_size(px(12.))
        .font_weight(if active { gpui::FontWeight::SEMIBOLD } else { gpui::FontWeight::MEDIUM })
        .bg(bg)
        .text_color(fg)
        .cursor_pointer()
        .hover(move |s| s.bg(if active { active_bg } else { inactive_hover }))
        .child(label.into())
}

/// Inset track that contains [`tab`] buttons.
pub fn segmented_control(theme: &Theme) -> gpui::Div {
    div()
        .flex()
        .gap(px(2.))
        .p(px(2.))
        .rounded(px(8.))
        .bg(theme.rgba(theme.fg_muted, 0.1))
}

/// Rounded color swatch (class color chip).
pub fn color_swatch(color: (u8, u8, u8), size: Pixels, theme: &Theme) -> gpui::Div {
    let (r, g, b) = color;
    div()
        .size(size)
        .flex_none()
        .rounded(px(3.5))
        .bg(gpui::rgb((r as u32) << 16 | (g as u32) << 8 | b as u32))
        .border_1()
        .border_color(theme.rgba(0x000000, if theme.dark { 0.45 } else { 0.2 }))
}
