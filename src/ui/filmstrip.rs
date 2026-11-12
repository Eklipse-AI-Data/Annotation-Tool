//! Virtualized thumbnail filmstrip under the canvas: 96×54 thumbs in 102px
//! slots, horizontal wheel/drag scrolling, current-item selection ring,
//! click-to-navigate. Thumbnails decode on background threads.
//!
//! Two small state machines live here:
//! * [`Pointer`] — idle / hovering / pressed / dragging. A press becomes a
//!   click or a drag depending on how far the pointer travels.
//! * [`ThumbLoad`] — per-thumbnail decode state. Only *visible* slots ever
//!   start a decode, at most [`crate::images::thumb_budget`] at a time, so
//!   flinging through thousands of images no longer queues thousands of
//!   full-resolution decodes (the low-end-machine crash).

use std::collections::HashMap;
use std::path::PathBuf;

use gpui::{div, prelude::*, px, Context, Pixels, Point, ScrollDelta, Styled, Window, img};

use super::canvas::app_state;
use super::theme::ThemeState;

pub const THUMB_W: f32 = 96.0;
pub const THUMB_H: f32 = 54.0;
pub const SLOT_W: f32 = 102.0; // thumb + 6px padding per side
pub const STRIP_H: f32 = 66.0;
/// Pointer travel (px) that turns a press into a drag instead of a click.
const DRAG_THRESHOLD: f32 = 4.0;

/// Pointer interaction with the strip.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Pointer {
    Idle,
    Hovering { slot: usize },
    /// Left button down; still a click candidate.
    Pressed { start: Point<Pixels>, scroll_at_press: f32 },
    /// Moved past [`DRAG_THRESHOLD`] while pressed; scrolls with the pointer.
    Dragging { start: Point<Pixels>, scroll_at_press: f32 },
}

/// Decode state of one thumbnail. `Ready` thumbnails live in the shared
/// `ImageStore` cache and "not requested" is the absence of an entry, so only
/// the in-between states are stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ThumbLoad {
    Loading,
    /// Missing/undecodable file. Remembered so a broken image isn't retried
    /// on every frame (it used to be: a decode loop while it was on screen).
    Failed,
}

pub struct Filmstrip {
    scroll_x: f32,
    view_width: f32,
    pointer: Pointer,
    /// Thumbnail decode states for the current folder, keyed by filename.
    loads: HashMap<String, ThumbLoad>,
    /// Bumped by [`Filmstrip::clear`]; results from an older folder are ignored.
    epoch: u64,
    bounds_origin: Point<Pixels>,
}

impl Filmstrip {
    pub fn new(_cx: &mut Context<Self>) -> Self {
        Self {
            scroll_x: 0.0,
            view_width: 0.0,
            pointer: Pointer::Idle,
            loads: HashMap::new(),
            epoch: 0,
            bounds_origin: Point::default(),
        }
    }

    fn max_scroll(&self, count: usize) -> f32 {
        (count as f32 * SLOT_W - self.view_width).max(0.0)
    }

    fn clamp_scroll(&mut self, count: usize) {
        self.scroll_x = self.scroll_x.clamp(0.0, self.max_scroll(count));
    }

    /// Scrolls so the current item is visible (centers it when out of view).
    pub fn set_current(&mut self, index: usize, count: usize) {
        let x = index as f32 * SLOT_W;
        if x < self.scroll_x || x + SLOT_W > self.scroll_x + self.view_width {
            self.scroll_x = (x - self.view_width / 2.0 + SLOT_W / 2.0).max(0.0);
        }
        self.clamp_scroll(count);
    }

    pub fn scroll_by(&mut self, delta: f32, count: usize) {
        self.scroll_x += delta;
        self.clamp_scroll(count);
    }

    /// Resets for a new folder. In-flight decodes from the old folder are
    /// ignored when they land (epoch mismatch).
    pub fn clear(&mut self) {
        self.scroll_x = 0.0;
        self.pointer = Pointer::Idle;
        self.loads.clear();
        self.epoch += 1;
    }

    /// Content slot under a viewport-local x (accounts for scrolling);
    /// `None` left of the strip or past the last thumbnail.
    fn slot_at(&self, local_x: Pixels, count: usize) -> Option<usize> {
        let local_x = f32::from(local_x);
        if local_x < 0.0 {
            return None;
        }
        let slot = ((local_x + self.scroll_x) / SLOT_W) as usize;
        (slot < count).then_some(slot)
    }

    fn hover_at(&self, local_x: Pixels, count: usize) -> Pointer {
        match self.slot_at(local_x, count) {
            Some(slot) => Pointer::Hovering { slot },
            None => Pointer::Idle,
        }
    }

    fn loading_count(&self) -> usize {
        self.loads.values().filter(|s| **s == ThumbLoad::Loading).count()
    }

    // ------------------------------------------------------------------
    // Pointer transitions
    // ------------------------------------------------------------------

    fn on_wheel(&mut self, event: &gpui::ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
        let dy = match event.delta {
            ScrollDelta::Lines(l) => l.y * SLOT_W,
            ScrollDelta::Pixels(p) => f32::from(p.y),
        };
        let count = app_state(cx).image_list.len();
        self.scroll_by(dy, count);
        cx.notify();
    }

    fn on_down(&mut self, event: &gpui::MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if event.button == gpui::MouseButton::Left {
            self.pointer = Pointer::Pressed { start: event.position, scroll_at_press: self.scroll_x };
        }
        cx.notify();
    }

    fn on_move(&mut self, event: &gpui::MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        let count = app_state(cx).image_list.len();
        let local = event.position - self.bounds_origin;
        let left_down = event.pressed_button == Some(gpui::MouseButton::Left);
        self.pointer = match self.pointer {
            // Released somewhere we never saw (e.g. outside the window): drop
            // back to hovering instead of scrolling with no button held.
            Pointer::Pressed { .. } | Pointer::Dragging { .. } if !left_down => {
                self.hover_at(local.x, count)
            }
            Pointer::Pressed { start, scroll_at_press }
                if f32::from(event.position.x - start.x).abs() > DRAG_THRESHOLD =>
            {
                Pointer::Dragging { start, scroll_at_press }
            }
            held @ (Pointer::Pressed { .. } | Pointer::Dragging { .. }) => held,
            Pointer::Idle | Pointer::Hovering { .. } => self.hover_at(local.x, count),
        };
        if let Pointer::Dragging { start, scroll_at_press } = self.pointer {
            self.scroll_x = scroll_at_press - f32::from(event.position.x - start.x);
            self.clamp_scroll(count);
        }
        cx.notify();
    }

    fn on_up(&mut self, event: &gpui::MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        if event.button != gpui::MouseButton::Left {
            return;
        }
        let count = app_state(cx).image_list.len();
        let local = event.position - self.bounds_origin;
        if let Pointer::Pressed { .. } = self.pointer {
            if let Some(index) = self.slot_at(local.x, count) {
                cx.emit(FilmstripClick(index));
            }
        }
        self.pointer = self.hover_at(local.x, count);
        cx.notify();
    }

    /// Release outside the strip ends a press/drag (never a click).
    fn on_up_out(&mut self, _: &gpui::MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.pointer, Pointer::Pressed { .. } | Pointer::Dragging { .. }) {
            self.pointer = Pointer::Idle;
            cx.notify();
        }
    }

    // ------------------------------------------------------------------
    // Thumbnail loading
    // ------------------------------------------------------------------

    /// Starts decodes for visible thumbnails that have none yet — nearest to
    /// the current item first — while under the decode budget. Called from
    /// render, so slots that scrolled away are simply never started; each
    /// completion re-renders and pulls the next visible ones.
    fn start_visible_loads(
        &mut self,
        mut wanted: Vec<(usize, String, PathBuf)>,
        current: Option<usize>,
        cx: &mut Context<Self>,
    ) {
        let anchor = current
            .filter(|&c| wanted.iter().any(|(idx, _, _)| *idx == c))
            .unwrap_or_else(|| wanted.first().map(|(idx, _, _)| *idx).unwrap_or(0));
        wanted.sort_by_key(|(idx, _, _)| idx.abs_diff(anchor));
        let budget = crate::images::thumb_budget();
        for (_, name, path) in wanted {
            if self.loading_count() >= budget {
                break;
            }
            self.loads.insert(name.clone(), ThumbLoad::Loading);
            let epoch = self.epoch;
            let entity = cx.entity();
            crate::images::load_thumb(cx, path.clone(), entity, move |strip, result, cx| {
                if strip.epoch != epoch {
                    return; // folder changed while decoding
                }
                match result {
                    Ok(image) => {
                        strip.loads.remove(&name);
                        crate::images::ImageStore::cache_thumb(cx, &path, image);
                    }
                    Err(_) => {
                        strip.loads.insert(name.clone(), ThumbLoad::Failed);
                    }
                }
                cx.notify();
            });
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct FilmstripClick(pub usize);

impl gpui::EventEmitter<FilmstripClick> for Filmstrip {}

impl Render for Filmstrip {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = ThemeState::current(cx);
        let state = app_state(cx);
        let count = state.image_list.len();
        let current_index = usize::try_from(state.current_image_index).ok();
        let image_dir = PathBuf::from(&state.image_dir);
        let scroll_x = self.scroll_x;
        let hovered = match self.pointer {
            Pointer::Hovering { slot } => Some(slot),
            _ => None,
        };

        // Visible slots only — the full list is never cloned per frame.
        let first = ((scroll_x / SLOT_W) as i64 - 1).max(0) as usize;
        let visible_w = self.view_width.max(1.0);
        let last = ((((scroll_x + visible_w) / SLOT_W) as i64 + 1) as usize).min(count);
        let visible: Vec<String> = state
            .image_list
            .get(first..last.max(first))
            .map(|names| names.to_vec())
            .unwrap_or_default();

        let mut slots: Vec<gpui::AnyElement> = Vec::with_capacity(visible.len());
        let mut wanted = Vec::new();
        for (offset, name) in visible.into_iter().enumerate() {
            let idx = first + offset;
            let path = image_dir.join(&name);
            let is_current = current_index == Some(idx);
            let x = idx as f32 * SLOT_W - scroll_x;

            let thumb = crate::images::ImageStore::thumb_cached(cx, &path);
            let load = self.loads.get(&name).copied();
            let ai_pending = super::autolabel_ui::review_status(cx, &name)
                == Some(crate::autolabel::review::Status::Pending);
            let border = if is_current {
                theme.rgb(theme.accent)
            } else if hovered == Some(idx) {
                theme.rgba(theme.fg_text, 0.55)
            } else {
                theme.rgb(theme.divider)
            };

            let content = match (thumb, load) {
                (Some(image), _) => img(image).size(px(THUMB_W)).h(px(THUMB_H)).into_any_element(),
                (None, Some(ThumbLoad::Failed)) => div()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(theme.rgba(theme.fg_muted, 0.1))
                    .text_size(px(10.0))
                    .text_color(theme.rgb(theme.fg_muted))
                    .child("?")
                    .into_any_element(),
                (None, load) => {
                    if load.is_none() {
                        wanted.push((idx, name, path));
                    }
                    div().size_full().bg(theme.rgba(theme.fg_muted, 0.1)).into_any_element()
                }
            };

            slots.push(
                div()
                    .id(("slot", idx))
                    .absolute()
                    .left(px(x + 3.0))
                    .top(px(6.0))
                    .size(px(THUMB_W))
                    .h(px(THUMB_H))
                    .overflow_hidden()
                    .rounded(px(5.0))
                    .when(is_current, |el| el.border_2())
                    .when(!is_current, |el| el.border_1())
                    .border_color(border)
                    .child(content)
                    .when(ai_pending, |el| el.child(ai_badge(theme)))
                    .into_any_element(),
            );
        }
        if !wanted.is_empty() {
            self.start_visible_loads(wanted, current_index, cx);
        }

        div()
            .id("filmstrip")
            .relative()
            .w_full()
            .h(px(STRIP_H))
            .flex_none()
            .overflow_hidden()
            .bg(theme.rgb(theme.bg_pane))
            .border_t_1()
            .border_color(theme.rgb(theme.divider))
            .on_scroll_wheel(cx.listener(Self::on_wheel))
            .on_mouse_down(gpui::MouseButton::Left, cx.listener(Self::on_down))
            .on_mouse_move(cx.listener(Self::on_move))
            .on_mouse_up(gpui::MouseButton::Left, cx.listener(Self::on_up))
            .on_mouse_up_out(gpui::MouseButton::Left, cx.listener(Self::on_up_out))
            .on_hover(cx.listener(|strip: &mut Filmstrip, hovered: &bool, _w, cx| {
                if !*hovered && matches!(strip.pointer, Pointer::Hovering { .. }) {
                    strip.pointer = Pointer::Idle;
                    cx.notify();
                }
            }))
            .children(slots)
            .when(count == 0, |el| {
                el.child(
                    div()
                        .absolute()
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(11.0))
                        .text_color(theme.rgb(theme.fg_muted))
                        .child("No images"),
                )
            })
            .child(
                // Invisible sizer that records the strip's viewport bounds.
                gpui::canvas(
                    move |bounds, _window, _cx| bounds,
                    {
                        let entity = cx.entity();
                        move |bounds, _state, _window, cx| {
                            let width = f32::from(bounds.size.width);
                            entity.update(cx, |strip, cx| {
                                if strip.view_width != width {
                                    strip.view_width = width;
                                    strip.clamp_scroll(count);
                                    cx.notify();
                                }
                                strip.bounds_origin = bounds.origin;
                            });
                        }
                    },
                )
                .absolute()
                .size_full(),
            )
    }
}

/// Small "AI" pill marking images whose labels came from Auto Label and
/// haven't been reviewed yet.
pub fn ai_badge(theme: &super::theme::Theme) -> gpui::Div {
    div()
        .absolute()
        .top(px(3.0))
        .left(px(3.0))
        .px(px(4.0))
        .h(px(13.0))
        .flex()
        .items_center()
        .rounded(px(3.0))
        .bg(theme.rgb(theme.info))
        .text_size(px(9.0))
        .font_weight(gpui::FontWeight::BOLD)
        .text_color(gpui::white())
        .child("AI")
}
