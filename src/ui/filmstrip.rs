//! Virtualized thumbnail filmstrip under the canvas: 96×54 thumbs in 102px
//! slots, horizontal wheel/drag scrolling, current-item selection ring,
//! click-to-navigate. Thumbnails decode on background threads.

use std::collections::HashSet;
use std::path::PathBuf;

use gpui::{div, prelude::*, px, Context, Pixels, Point,
    ScrollDelta, SharedString, Styled, Window, img};

use super::canvas::app_state;
use super::theme::ThemeState;

pub const THUMB_W: f32 = 96.0;
pub const THUMB_H: f32 = 54.0;
pub const SLOT_W: f32 = 102.0; // thumb + 6px padding per side
pub const STRIP_H: f32 = 66.0;

pub struct Filmstrip {
    scroll_x: f32,
    view_width: f32,
    /// Filenames with an in-flight thumbnail request.
    pending: HashSet<String>,
    drag_start: Option<(Point<Pixels>, f32)>,
    hover: Option<(usize, Point<Pixels>)>,
    bounds_origin: Point<Pixels>,
}

impl Filmstrip {
    pub fn new(_cx: &mut Context<Self>) -> Self {
        Self {
            scroll_x: 0.0,
            view_width: 0.0,
            pending: HashSet::new(),
            drag_start: None,
            hover: None,
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

    fn slot_at(&self, local_x: f32) -> Option<usize> {
        if local_x < 0.0 {
            return None;
        }
        Some((local_x / SLOT_W) as usize)
    }

    fn on_wheel(&mut self, event: &gpui::ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
        let dy = match event.delta {
            ScrollDelta::Lines(l) => l.y * 102.0,
            ScrollDelta::Pixels(p) => f32::from(p.y),
            };
        let count = app_state(cx).image_list.len();
        self.scroll_by(dy, count);
        cx.notify();
    }

    fn on_down(&mut self, event: &gpui::MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if event.button == gpui::MouseButton::Left {
            self.drag_start = Some((event.position, self.scroll_x));
        }
        cx.notify();
    }

    fn on_move(&mut self, event: &gpui::MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        let local = event.position - self.bounds_origin;
        self.hover = self.slot_at(f32::from(local.x)).map(|i| (i, local));
        if let Some((start, start_scroll)) = self.drag_start {
            let count = app_state(cx).image_list.len();
            self.scroll_x = start_scroll - f32::from(event.position.x - start.x);
            self.clamp_scroll(count);
        }
        cx.notify();
    }

    fn on_up(&mut self, event: &gpui::MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        if event.button != gpui::MouseButton::Left {
            return;
        }
        let drag = self.drag_start.take();
        let moved_far = drag
            .map(|(start, _)| f32::from((event.position - start).x).abs() > 4.0)
            .unwrap_or(true);
        if moved_far {
            cx.notify();
            return;
        }
        let local = event.position - self.bounds_origin;
        if let Some(index) = self.slot_at(f32::from(local.x)) {
            cx.emit(FilmstripClick(index));
        }
        cx.notify();
    }
}

#[derive(Debug, Clone, Copy)]
pub struct FilmstripClick(pub usize);


impl gpui::EventEmitter<FilmstripClick> for Filmstrip {}

impl Render for Filmstrip {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = ThemeState::current(cx);
        let state = app_state(cx);
        let filenames: Vec<String> = state.image_list.clone();
        let current_index = state.current_image_index;
        let image_dir = state.image_dir.clone();
        let scroll_x = self.scroll_x;

        // Determine visible slots.
        let first = ((scroll_x / SLOT_W) as i64 - 1).max(0) as usize;
        let visible_w = self.view_width.max(1.0);
        let last = (((scroll_x + visible_w) / SLOT_W) as i64 + 1) as usize;

        let mut slots: Vec<gpui::AnyElement> = Vec::new();
        if !filenames.is_empty() {
            for idx in first..last.min(filenames.len()) {
                let name = filenames[idx].clone();
                let path = PathBuf::from(&image_dir).join(&name);
                let is_current = current_index == idx as i64;
                let x = idx as f32 * SLOT_W - scroll_x;

                let thumb = crate::images::ImageStore::thumb_cached(cx, &path);
                if thumb.is_none() && !self.pending.contains(&name) && path.exists() {
                    self.pending.insert(name.clone());
                    let name_for_pending = name.clone();
                    let entity = cx.entity();
                    let pending_name = name_for_pending.clone();
                    crate::images::load_thumb(cx, path.clone(), entity, move |strip, result, cx| {
                        strip.pending.remove(&pending_name);
                        if let Ok(image) = result {
                            crate::images::ImageStore::cache_thumb(cx, &path, image);
                        }
                        cx.notify();
                    });
                }

                let border = if is_current {
                    theme.rgb(theme.accent)
                } else {
                    theme.rgba(theme.border, 0.8)
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
                        .child(match thumb {
                            Some(image) => img(image)
                                .size(px(THUMB_W))
                                .h(px(THUMB_H))
                                .into_any_element(),
                            None => div()
                                .size_full()
                                .bg(theme.rgba(theme.fg_muted, 0.1))
                                .into_any_element(),
                        })
                        .into_any_element(),
                );
            }
        }

        let count = filenames.len();
        let strip = div()
            .id("filmstrip")
            .relative()
            .w_full()
            .h(px(STRIP_H))
            .flex_none()
            .overflow_hidden()
            .bg(theme.rgb(theme.bg_sidebar))
            .border_t_1()
            .border_color(theme.rgba(theme.border, 0.6))
            .on_scroll_wheel(cx.listener(Self::on_wheel))
            .on_mouse_down(gpui::MouseButton::Left, cx.listener(Self::on_down))
            .on_mouse_move(cx.listener(Self::on_move))
            .on_mouse_up(gpui::MouseButton::Left, cx.listener(Self::on_up))
            .on_hover(cx.listener(|strip: &mut Filmstrip, hovered: &bool, _w, cx| {
                if !*hovered && strip.hover.take().is_some() {
                    cx.notify();
                }
            }))
            .children(slots)
            .when(filenames.is_empty(), |el| {
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
            );

        strip
    }
}

impl Filmstrip {
    pub fn clear(&mut self) {
        self.scroll_x = 0.0;
        self.pending.clear();
    }
}

#[allow(dead_code)]
fn unused(_: SharedString) {}
