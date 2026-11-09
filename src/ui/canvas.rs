//! The annotation canvas: image painting with GPU-sampled zoom/pan, box
//! drawing/editing gestures, crosshair, preview overlay and box labels — a
//! faithful port of the Tkinter canvas interactions in `src/controller.py`
//! with the rendering upgraded to GPUI paths/quads/shaped text.

use std::path::PathBuf;
use std::sync::Arc;

use gpui::{
    div, prelude::*, px, App, Bounds, Context, Element, ElementId, Entity,
    EventEmitter, FocusHandle, Focusable, GlobalElementId, Hsla, InteractiveElement,
    IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    PathBuilder, Pixels, Point, RenderImage, ScrollWheelEvent, SharedString, Size, Stateful,
    Styled, TextRun, Window, quad, size,
};

use crate::state::model::AppState;
use crate::ui::theme::ThemeState;
use crate::yolo::BBox;

impl gpui::Global for AppState {}

/// Installs the AppState as a GPUI global.
pub fn install_app_state(cx: &mut App, state: AppState) {
    cx.set_global(state);
}

/// Convenience accessors for the global AppState.
pub fn app_state(cx: &App) -> &AppState {
    cx.global::<AppState>()
}
pub fn app_state_mut(cx: &mut App) -> &mut AppState {
    cx.global_mut::<AppState>()
}

// ---------------------------------------------------------------------------
// Transform math
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub struct Transform {
    /// image pixel → viewport pixel multiplier.
    pub scale: f64,
    /// Viewport-space origin of image pixel (0,0).
    pub origin: Point<Pixels>,
    /// Whether the image overflows the viewport (panning is possible).
    pub overflows: (bool, bool),
}

impl Transform {
    pub fn compute(
        dims: (u32, u32),
        view: Size<Pixels>,
        zoom_factor: f64,
        pan: Point<Pixels>,
    ) -> Self {
        let (iw, ih) = (dims.0 as f64, dims.1 as f64);
        let (cw, ch) = (f32::from(view.width) as f64, f32::from(view.height) as f64);
        if iw <= 0.0 || ih <= 0.0 || cw <= 0.0 || ch <= 0.0 {
            return Self {
                scale: 1.0,
                origin: Point::default(),
                overflows: (false, false),
            };
        }
        let base = (cw / iw).min(ch / ih);
        let scale = base * zoom_factor;
        let (nw, nh) = (iw * scale, ih * scale);
        let overflows_x = nw > cw + 0.5;
        let overflows_y = nh > ch + 0.5;
        let origin_x = if overflows_x {
            px(f32::from(pan.x).clamp((cw - nw) as f32, 0.0))
        } else {
            px(((cw - nw) / 2.0) as f32)
        };
        let origin_y = if overflows_y {
            px(f32::from(pan.y).clamp((ch - nh) as f32, 0.0))
        } else {
            px(((ch - nh) / 2.0) as f32)
        };
        Self {
            scale,
            origin: Point {
                x: origin_x,
                y: origin_y,
            },
            overflows: (overflows_x, overflows_y),
        }
    }

    /// Image-space pixel → viewport point.
    pub fn to_view(&self, x: f64, y: f64) -> Point<Pixels> {
        Point {
            x: self.origin.x + px((x * self.scale) as f32),
            y: self.origin.y + px((y * self.scale) as f32),
        }
    }

    /// Viewport point → image-space pixel (may lie outside the image).
    pub fn to_image(&self, p: Point<Pixels>) -> (f64, f64) {
        (
            ((f32::from(p.x - self.origin.x)) as f64) / self.scale,
            ((f32::from(p.y - self.origin.y)) as f64) / self.scale,
        )
    }

    /// The image rectangle in viewport space.
    pub fn image_bounds(&self, dims: (u32, u32)) -> Bounds<Pixels> {
        Bounds::new(
            self.origin,
            size(
                px((dims.0 as f64 * self.scale) as f32),
                px((dims.1 as f64 * self.scale) as f32),
            ),
        )
    }
}

// ---------------------------------------------------------------------------
// Gestures
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Corner {
    Nw,
    Ne,
    Sw,
    Se,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Gesture {
    None,
    Drawing { start: Point<Pixels>, cur: Point<Pixels> },
    Moving { active: usize, anchor: Point<Pixels> },
    Resizing { index: usize, handle: Corner },
    Panning { last: Point<Pixels> },
}

/// Emitted to the workspace whenever canvas interactions change app state.
pub struct CanvasChanged;

// ---------------------------------------------------------------------------
// Canvas view
// ---------------------------------------------------------------------------

pub struct Canvas {
    pub focus: FocusHandle,
    pub current: Option<Arc<RenderImage>>,
    pub loading: Option<PathBuf>,
    /// Last known viewport size and bounds (updated during element prepaint).
    pub view_size: Size<Pixels>,
    pub view_bounds: Bounds<Pixels>,
    pub gesture: Gesture,
    pub hover: Option<Point<Pixels>>,
    pub pan: Point<Pixels>,
    /// Deferred image to request once the element has been laid out at least
    /// once (so fit-to-window math has a real viewport size).
    pub pending_request: Option<PathBuf>,
}

impl Canvas {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus: cx.focus_handle(),
            current: None,
            loading: None,
            view_size: size(px(1.), px(1.)),
            view_bounds: Bounds::default(),
            gesture: Gesture::None,
            hover: None,
            pan: Point::default(),
            pending_request: None,
        }
    }

    pub fn transform(&self, state: &AppState) -> Transform {
        Transform::compute(
            state.current_image_size.unwrap_or((1, 1)),
            self.view_size,
            state.zoom_factor,
            self.pan,
        )
    }

    /// Requests an image: cache hit → immediate; otherwise decode in the
    /// background. Updates `AppState.current_image_size` and resets pan.
    pub fn request_image(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if Some(&path) == self.loading.as_ref() {
            return;
        }
        if let Some(hit) = crate::images::ImageStore::full_cached(cx, &path) {
            let size = hit.size(0);
            self.current = Some(hit);
            self.loading = None;
            self.pan = Point::default();
            app_state_mut(cx).current_image_size =
                Some((size.width.0 as u32, size.height.0 as u32));
            cx.notify();
            cx.emit(CanvasChanged);
            return;
        }
        self.current = None;
        self.loading = Some(path.clone());
        self.pan = Point::default();
        let entity = cx.entity();
        crate::images::load_full(cx, path, entity, |canvas, (path, result), cx| match result {
            Ok((image, dims)) => {
                crate::images::ImageStore::cache_full(cx, &path, image.clone());
                canvas.current = Some(image);
                canvas.loading = None;
                app_state_mut(cx).current_image_size = Some(dims);
                cx.emit(CanvasChanged);
            }
            Err(_err) => {
                canvas.current = None;
                canvas.loading = None;
                app_state_mut(cx).current_image_size = None;
                cx.emit(CanvasChanged);
            }
        });
        cx.notify();
    }

    // ------------------------------------------------------------------
    // Event handlers
    // ------------------------------------------------------------------

    fn local(&self, position: Point<Pixels>) -> Point<Pixels> {
        position - self.view_bounds.origin
    }

    fn on_mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.button != MouseButton::Left {
            return;
        }
        window.focus(&self.focus);
        let state = app_state(cx);
        if state.current_image_size.is_none() {
            return;
        }
        let pos = self.local(event.position);
        let t = self.transform(state);
        let dims = state.current_image_size.unwrap();
        let img_bounds = t.image_bounds(dims);
        // Clamp to the image rect for gesture math (Python parity).
        let clamped = Point {
            x: pos.x.clamp(img_bounds.left(), img_bounds.right()),
            y: pos.y.clamp(img_bounds.top(), img_bounds.bottom()),
        };
        let (iw, ih) = (dims.0 as f64, dims.1 as f64);
        let (buf3, slack) = (px(3.0), px(11.0)); // box buffer, handle slack (6px + 8px)

        // 1) Resize handles — only on boxes that are already selected.
        if !state.selected_indices.is_empty() {
            for &idx in &state.selected_indices {
                let Some(bbox) = state.boxes.get(idx) else { continue };
                let (x1, y1, x2, y2) = bbox.denormalized(iw, ih);
                for (corner, cx_, cy_) in [
                    (Corner::Nw, x1, y1),
                    (Corner::Ne, x2, y1),
                    (Corner::Sw, x1, y2),
                    (Corner::Se, x2, y2),
                ] {
                    let c = t.to_view(cx_, cy_);
                    if (pos.x - c.x).abs() <= slack && (pos.y - c.y).abs() <= slack {
                        app_state_mut(cx).save_state();
                        self.gesture = Gesture::Resizing { index: idx, handle: corner };
                        cx.notify();
                        return;
                    }
                }
            }
        }

        // 2) Box bodies — topmost (reverse order) wins, ±3px buffer.
        for idx in (0..state.boxes.len()).rev() {
            let bbox = state.boxes[idx];
            let (x1, y1, x2, y2) = bbox.denormalized(iw, ih);
            let p1 = t.to_view(x1, y1);
            let p2 = t.to_view(x2, y2);
            let within = pos.x >= p1.x - buf3
                && pos.x <= p2.x + buf3
                && pos.y >= p1.y - buf3
                && pos.y <= p2.y + buf3;
            if within {
                let state = app_state_mut(cx);
                if event.modifiers.shift {
                    if state.selected_indices.contains(&idx) {
                        state.selected_indices.remove(&idx);
                    } else {
                        state.selected_indices.insert(idx);
                    }
                } else if !state.selected_indices.contains(&idx) {
                    state.selected_indices.clear();
                    state.selected_indices.insert(idx);
                }
                // Plain click on an already-selected box keeps the whole
                // multi-selection (group move).
                state.save_state();
                self.gesture = Gesture::Moving {
                    active: idx,
                    anchor: clamped,
                };
                cx.notify();
                cx.emit(CanvasChanged);
                return;
            }
        }

        // 3) Whitespace: clear selection, maybe start drawing.
        let state = app_state_mut(cx);
        if !event.modifiers.shift {
            state.selected_indices.clear();
        }
        if state.current_class_index != -1 {
            state.save_state();
            self.gesture = Gesture::Drawing {
                start: clamped,
                cur: clamped,
            };
        }
        cx.notify();
        cx.emit(CanvasChanged);
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let pos = self.local(event.position);
        self.hover = Some(pos);
        cx.notify(); // crosshair + coords

        let gesture = self.gesture;
        match gesture {
            Gesture::None => {}
            Gesture::Drawing { start, .. } => {
                self.gesture = Gesture::Drawing { start, cur: pos };
                cx.notify();
            }
            Gesture::Panning { last } => {
                let dx = pos.x - last.x;
                let dy = pos.y - last.y;
                self.pan = Point {
                    x: self.pan.x + dx,
                    y: self.pan.y + dy,
                };
                self.gesture = Gesture::Panning { last: pos };
                cx.notify();
            }
            Gesture::Moving { active: _, anchor } => {
                let state = app_state_mut(cx);
                let Some(dims) = state.current_image_size else {
                    self.gesture = Gesture::None;
                    return;
                };
                let t = self.transform(state);
                let (iw, ih) = (dims.0 as f64, dims.1 as f64);
                let (dx_img, dy_img) = (
                    ((f32::from(pos.x - anchor.x)) as f64) / t.scale,
                    ((f32::from(pos.y - anchor.y)) as f64) / t.scale,
                );
                // Group-clamp so the union bbox stays inside the image.
                let selected: Vec<usize> = state.selected_indices.iter().copied().collect();
                let mut min_x = f64::MAX;
                let mut min_y = f64::MAX;
                let mut max_x = f64::MIN;
                let mut max_y = f64::MIN;
                for &i in &selected {
                    if let Some(b) = state.boxes.get(i) {
                        let (x1, y1, x2, y2) = b.denormalized(iw, ih);
                        min_x = min_x.min(x1);
                        min_y = min_y.min(y1);
                        max_x = max_x.max(x2);
                        max_y = max_y.max(y2);
                    }
                }
                let dx_img = dx_img.clamp(-min_x, iw - max_x);
                let dy_img = dy_img.clamp(-min_y, ih - max_y);
                for &i in &selected {
                    if let Some(b) = state.boxes.get_mut(i) {
                        b.x_center += dx_img / iw;
                        b.y_center += dy_img / ih;
                    }
                }
                let active = state.selected_indices.iter().max().copied().unwrap_or(usize::MAX);
                self.gesture = Gesture::Moving {
                    active,
                    anchor: anchor + Point {
                        x: px((dx_img * t.scale) as f32),
                        y: px((dy_img * t.scale) as f32),
                    },
                };
                cx.notify();
                cx.emit(CanvasChanged);
            }
            Gesture::Resizing { index, handle } => {
                let state = app_state_mut(cx);
                let Some(dims) = state.current_image_size else {
                    self.gesture = Gesture::None;
                    return;
                };
                let t = self.transform(state);
                let (iw, ih) = (dims.0 as f64, dims.1 as f64);
                // Clamped cursor position in image px.
                let img_bounds = t.image_bounds(dims);
                let cl = Point {
                    x: pos.x.clamp(img_bounds.left(), img_bounds.right()),
                    y: pos.y.clamp(img_bounds.top(), img_bounds.bottom()),
                };
                let (mx, my) = t.to_image(cl);
                let (mx, my) = (mx.clamp(0.0, iw), my.clamp(0.0, ih));
                if let Some(b) = state.boxes.get_mut(index) {
                    let (mut x1, mut y1, mut x2, mut y2) = b.denormalized(iw, ih);
                    match handle {
                        Corner::Nw => {
                            x1 = mx;
                            y1 = my;
                        }
                        Corner::Ne => {
                            x2 = mx;
                            y1 = my;
                        }
                        Corner::Sw => {
                            x1 = mx;
                            y2 = my;
                        }
                        Corner::Se => {
                            x2 = mx;
                            y2 = my;
                        }
                    }
                    *b = BBox::from_corners(x1, y1, x2, y2, iw, ih, b.class_id);
                }
                cx.notify();
                cx.emit(CanvasChanged);
            }
        }
    }

    fn on_mouse_up(&mut self, event: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if event.button == MouseButton::Right {
            if matches!(self.gesture, Gesture::Panning { .. }) {
                self.gesture = Gesture::None;
            }
            return;
        }
        if event.button != MouseButton::Left {
            return;
        }
        match self.gesture {
            Gesture::Drawing { start, .. } => {
                let pos = self.local(event.position);
                let state = app_state_mut(cx);
                let Some(dims) = state.current_image_size else {
                    self.gesture = Gesture::None;
                    return;
                };
                let t = self.transform(state);
                let (iw, ih) = (dims.0 as f64, dims.1 as f64);
                let (dx, dy) = (f32::from(pos.x - start.x).abs(), f32::from(pos.y - start.y).abs());

                if dx > 5.0 || dy > 5.0 {
                    // Draw: map start & unclamped end into image px, clamp both.
                    let (sx, sy) = t.to_image(start);
                    let (ex, ey) = t.to_image(pos);
                    let (sx, sy) = (sx.clamp(0.0, iw), sy.clamp(0.0, ih));
                    let (ex, ey) = (ex.clamp(0.0, iw), ey.clamp(0.0, ih));
                    let class_id = state
                        .current_class()
                        .map(|c| c.id)
                        .unwrap_or(-1);
                    let new_box = BBox::from_corners(sx, sy, ex, ey, iw, ih, class_id);
                    state.boxes.push(new_box);
                    state.selected_indices.clear();
                    state.selected_indices.insert(state.boxes.len() - 1);
                } else if state.current_class_index != -1 {
                    // Stamp: template box centered on the click.
                    state.save_state(); // deduped against the draw-start snapshot
                    let class = state.current_class().cloned();
                    if let Some(class) = class {
                        let (cx_img, cy_img) = {
                            let img_bounds = t.image_bounds(dims);
                            let cl = Point {
                                x: start.x.clamp(img_bounds.left(), img_bounds.right()),
                                y: start.y.clamp(img_bounds.top(), img_bounds.bottom()),
                            };
                            t.to_image(cl)
                        };
                        let w = class.default_w;
                        let h = class.default_h;
                        let x1 = (cx_img - w / 2.0).clamp(0.0, iw);
                        let y1 = (cy_img - h / 2.0).clamp(0.0, ih);
                        let x2 = (cx_img + w / 2.0).min(iw);
                        let y2 = (cy_img + h / 2.0).min(ih);
                        let new_box = BBox::from_corners(x1, y1, x2, y2, iw, ih, class.id);
                        state.boxes.push(new_box);
                        state.selected_indices.clear();
                        state.selected_indices.insert(state.boxes.len() - 1);
                    }
                }
                self.gesture = Gesture::None;
                cx.notify();
                cx.emit(CanvasChanged);
            }
            Gesture::Moving { .. } | Gesture::Resizing { .. } => {
                self.gesture = Gesture::None;
                cx.notify();
                cx.emit(CanvasChanged);
            }
            _ => {}
        }
    }

    fn on_right_down(&mut self, event: &MouseDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        self.gesture = Gesture::Panning {
            last: self.local(event.position),
        };
        cx.notify();
    }

    fn on_scroll(&mut self, event: &ScrollWheelEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(pos) = self.hover else { return };
        if app_state(cx).current_image_size.is_none() {
            return;
        }
        let dy = match event.delta {
            gpui::ScrollDelta::Lines(l) => l.y * 40.0,
            gpui::ScrollDelta::Pixels(p) => f32::from(p.y),
            
        };
        if dy == 0.0 {
            return;
        }
        let multiplier = if dy < 0.0 { 1.1 } else { 1.0 / 1.1 };
        self.zoom_at(pos, multiplier, cx);
    }

    /// Zoom about a viewport anchor point, keeping that image point fixed.
    pub fn zoom_at(&mut self, anchor: Point<Pixels>, multiplier: f64, cx: &mut Context<Self>) {
        let dims = app_state(cx).current_image_size.unwrap_or((1, 1));
        let old_zoom = app_state(cx).zoom_factor;
        let before = Transform::compute(dims, self.view_size, old_zoom, self.pan);
        let (ix, iy) = before.to_image(anchor);

        let new_zoom = (old_zoom * multiplier).clamp(0.1, 10.0);
        app_state_mut(cx).zoom_factor = new_zoom;

        let after = Transform::compute(dims, self.view_size, new_zoom, self.pan);
        // Solve origin' = anchor - image_pt * scale' for the pan.
        let desired_origin = Point {
            x: anchor.x - px((ix * after.scale) as f32),
            y: anchor.y - px((iy * after.scale) as f32),
        };
        if after.overflows.0 {
            self.pan.x = desired_origin.x;
        }
        if after.overflows.1 {
            self.pan.y = desired_origin.y;
        }
        cx.notify();
        cx.emit(CanvasChanged);
    }

    /// Fits the image (zoom 1.0) and resets pan.
    pub fn fit(&mut self, cx: &mut Context<Self>) {
        app_state_mut(cx).zoom_factor = 1.0;
        self.pan = Point::default();
        cx.notify();
        cx.emit(CanvasChanged);
    }

    pub fn zoom_center(&mut self, multiplier: f64, cx: &mut Context<Self>) {
        let center = Point {
            x: px(f32::from(self.view_size.width) / 2.0),
            y: px(f32::from(self.view_size.height) / 2.0),
        };
        self.zoom_at(center, multiplier, cx);
    }

    /// Status-bar coordinate readout for the current hover position.
    pub fn coords_text(&self, state: &AppState) -> SharedString {
        if let (Some(dims), Some(hover)) = (state.current_image_size, self.hover) {
            let (iw, ih) = (dims.0, dims.1);
            let t = self.transform(state);
            let (x, y) = t.to_image(hover);
            let img = t.image_bounds(dims);
            let inside =
                hover.x >= img.left() && hover.x <= img.right() && hover.y >= img.top() && hover.y <= img.bottom();
            if inside {
                return format!("X: {}, Y: {} | {}x{}", x as i64, y as i64, iw, ih).into();
            }
            return format!("Out of bounds | {}x{}", iw, ih).into();
        }
        "".into()
    }
}

impl EventEmitter<CanvasChanged> for Canvas {}

impl Focusable for Canvas {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Canvas {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = ThemeState::current(cx);
        let state = app_state(cx);
        let has_image = state.current_image_size.is_some();
        // An image is selected but still decoding: not an empty state.
        let loading = !has_image && state.current_image_index != -1;
        let (empty_title, empty_hint) = if state.image_dir.is_empty() {
            ("No image loaded", "Open an image folder to start annotating —")
        } else {
            ("No images to show", "This folder (or the active filter) has no images —")
        };
        let canvas_entity = cx.entity();

        let el: Stateful<gpui::Div> = div()
            .id("annotation-canvas")
            .relative()
            .size_full()
            .overflow_hidden()
            .track_focus(&self.focus)
            .bg(theme.rgb(theme.bg_canvas))
            .cursor(gpui::CursorStyle::Crosshair)
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_down(MouseButton::Right, cx.listener(Self::on_right_down))
            .on_mouse_up(MouseButton::Right, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_scroll_wheel(cx.listener(Self::on_scroll))
            .on_hover(cx.listener(|canvas: &mut Canvas, hovered: &bool, _window, cx| {
                if !*hovered && canvas.hover.take().is_some() {
                    cx.notify();
                }
            }))
            .child(CanvasElement {
                canvas: canvas_entity,
            });

        if has_image {
            el.into_any_element()
        } else if loading {
            el.child(
                div()
                    .absolute()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(12.))
                    .text_color(theme.rgba(theme.fg_muted, 0.8))
                    .child("Loading image…"),
            )
            .into_any_element()
        } else {
            el.child(
                div()
                    .absolute()
                    .size_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(12.))
                    .child(
                        div()
                            .size(px(56.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(14.))
                            .bg(theme.rgba(theme.fg_muted, 0.08))
                            .border_1()
                            .border_color(theme.rgba(theme.fg_muted, 0.14))
                            .child(super::icons::icon(
                                "image",
                                26.0,
                                theme.rgba(theme.fg_muted, 0.6),
                            )),
                    )
                    .child(
                        div()
                            .text_size(px(13.5))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(theme.rgb(theme.fg_text))
                            .child(empty_title),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(
                                div()
                                    .text_size(px(11.5))
                                    .text_color(theme.rgb(theme.fg_muted))
                                    .child(empty_hint),
                            )
                            .child(super::widgets::kbd("Ctrl+Shift+O", theme)),
                    ),
            )
            .into_any_element()
        }
    }
}

// ---------------------------------------------------------------------------
// The custom painting element
// ---------------------------------------------------------------------------

struct CanvasElement {
    canvas: Entity<Canvas>,
}

impl IntoElement for CanvasElement {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for CanvasElement {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (gpui::LayoutId, Self::RequestLayoutState) {
        let mut style = gpui::Style::default();
        style.size.width = gpui::Length::Definite(gpui::DefiniteLength::Fraction(1.0_f32.into()));
        style.size.height = gpui::Length::Definite(gpui::DefiniteLength::Fraction(1.0_f32.into()));
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.canvas.update(cx, |canvas, _| {
            let size_changed = canvas.view_size != bounds.size;
            canvas.view_size = bounds.size;
            canvas.view_bounds = bounds;
            if size_changed {
                canvas.pending_request = None;
            }
        });
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let theme = ThemeState::current(cx);
        let (image, dims, t, boxes_snapshot, preview_snapshot, gesture, hover, crosshair_color, crosshair_dash, show_labels) = {
            let canvas = self.canvas.read(cx);
            let state = app_state(cx);
            let dims = state.current_image_size.unwrap_or((0, 0));
            let t = canvas.transform(state);
            (
                canvas.current.clone(),
                dims,
                t,
                state.boxes.clone(),
                if state.preview_visible { state.preview_boxes.clone() } else { Vec::new() },
                canvas.gesture,
                canvas.hover,
                state.config.crosshair_color.clone(),
                state.config.crosshair_dash,
                state.show_labels,
            )
        };

        // GPUI paints in client-window coordinates: shift the transform from
        // element-local space into client space so everything lines up with
        // the mouse. (Input math stays element-local via view_bounds.)
        let t = Transform {
            origin: Point {
                x: t.origin.x + bounds.origin.x,
                y: t.origin.y + bounds.origin.y,
            },
            ..t
        };
        let hover = hover.map(|h| Point {
            x: h.x + bounds.origin.x,
            y: h.y + bounds.origin.y,
        });

        // --- Image ---
        if let Some(img) = &image {
            if dims.0 > 0 {
                let img_bounds = t.image_bounds(dims);
                let _ = window.paint_image(img_bounds, px(0.).into(), img.clone(), 0, false);
            }
        } else if dims.0 > 0 {
            // Loading: subtle placeholder where the image will land.
            let img_bounds = t.image_bounds(dims);
            window.paint_quad(gpui::fill(img_bounds, theme.rgba(theme.bg_elevated, 0.6)));
        }

        let (iw, ih) = (dims.0 as f64, dims.1 as f64);
        // Precomputed lookup so the paint closure doesn't borrow `cx`.
        let mut class_color_map: std::collections::HashMap<i64, (Hsla, String)> =
            std::collections::HashMap::new();
        for c in &app_state(cx).classes {
            class_color_map.insert(
                c.id,
                (
                    gpui::rgb((c.color.0 as u32) << 16 | (c.color.1 as u32) << 8 | c.color.2 as u32).into(),
                    c.name.clone(),
                ),
            );
        }
        let class_color = move |class_id: i64| -> (Hsla, String) {
            match class_color_map.get(&class_id) {
                Some((color, name)) => (*color, name.clone()),
                None if class_id == -1 => (theme.rgb(theme.fg_highlight), "Unlabeled".to_string()),
                None => (theme.rgb(theme.fg_highlight), "Unknown".to_string()),
            }
        };

        // --- Boxes ---
        let selected = app_state(cx).selected_indices.clone();
        let stroke_box = |window: &mut Window, p1: Point<Pixels>, p2: Point<Pixels>, color: Hsla, width: f32, dash: Option<(f32, f32)>| {
            let mut builder = PathBuilder::stroke(px(width));
            if let Some((a, b)) = dash {
                builder = builder.dash_array(&[px(a), px(b)]);
            }
            builder.move_to(p1);
            builder.line_to(Point { x: p2.x, y: p1.y });
            builder.line_to(p2);
            builder.line_to(Point { x: p1.x, y: p2.y });
            builder.close();
            if let Ok(path) = builder.build() {
                window.paint_path(path, color);
            }
        };

        for (i, b) in boxes_snapshot.iter().enumerate() {
            let (x1, y1, x2, y2) = b.denormalized(iw, ih);
            let p1 = t.to_view(x1, y1);
            let p2 = t.to_view(x2, y2);
            let is_selected = selected.contains(&i);
            let (color, name) = class_color(b.class_id);
            let stroke_color = if is_selected { theme.rgb(theme.selected_box) } else { color };
            let width = if is_selected { 3.0 } else { 2.0 };
            stroke_box(window, p1, p2, stroke_color, width, None);

            // Handles: 6px squares on selected boxes with a bg halo.
            if is_selected {
                let half = px(3.0);
                for c in [
                    Point { x: p1.x, y: p1.y },
                    Point { x: p2.x, y: p1.y },
                    Point { x: p1.x, y: p2.y },
                    Point { x: p2.x, y: p2.y },
                ] {
                    let hb = Bounds::new(Point { x: c.x - half, y: c.y - half }, size(px(6.0), px(6.0)));
                    window.paint_quad(quad(
                        hb,
                        gpui::Corners::from(px(1.)),
                        theme.rgb(theme.selected_box),
                        gpui::Edges::from(px(1.)),
                        theme.rgb(theme.bg_main),
                        gpui::BorderStyle::Solid,
                    ));
                }
            }

            // Label chip.
            if show_labels && iw > 0.0 {
                paint_label(window, cx, theme, p1, &name, color);
            }
        }

        // --- Preview boxes (clipboard / held preset) ---
        for b in &preview_snapshot {
            let (x1, y1, x2, y2) = b.denormalized(iw, ih);
            let p1 = t.to_view(x1, y1);
            let p2 = t.to_view(x2, y2);
            let (_, name) = class_color(b.class_id);
            stroke_box(window, p1, p2, theme.rgb(theme.info), 2.0, Some((6.0, 4.0)));
            if show_labels {
                paint_label(window, cx, theme, p1, &format!("{name} (Preview)"), theme.rgb(theme.info));
            }
        }

        // --- In-progress draw rectangle ---
        if let Gesture::Drawing { start, cur } = gesture {
            let p1 = Point {
                x: start.x.min(cur.x) + bounds.origin.x,
                y: start.y.min(cur.y) + bounds.origin.y,
            };
            let p2 = Point {
                x: start.x.max(cur.x) + bounds.origin.x,
                y: start.y.max(cur.y) + bounds.origin.y,
            };
            stroke_box(window, p1, p2, theme.rgb(theme.selected_box), 2.0, Some((4.0, 4.0)));
        }

        // --- Crosshair ---
        if let Some(hover) = hover {
            let (mut ch_color, _dash) = parse_crosshair_config(&crosshair_color, crosshair_dash, theme);
            // gpui 0.2.2 silently drops dash_array paths, so the dashed
            // crosshair becomes a semi-transparent solid line instead.
            ch_color.a *= 0.7;
            let x0 = bounds.origin.x;
            let y0 = bounds.origin.y;
            let w = bounds.size.width;
            let h = bounds.size.height;
            let mut h_line = PathBuilder::stroke(px(1.0));
            h_line.move_to(Point { x: x0, y: hover.y });
            h_line.line_to(Point { x: x0 + w, y: hover.y });
            if let Ok(p) = h_line.build() {
                window.paint_path(p, ch_color);
            }
            let mut v_line = PathBuilder::stroke(px(1.0));
            v_line.move_to(Point { x: hover.x, y: y0 });
            v_line.line_to(Point { x: hover.x, y: y0 + h });
            if let Ok(p) = v_line.build() {
                window.paint_path(p, ch_color);
            }
        }
    }
}

fn parse_crosshair_config(hex: &str, dash: (f32, f32), theme: &crate::ui::theme::Theme) -> (Hsla, (f32, f32)) {
    let parse_hex = |s: &str| -> Option<u32> {
        let s = s.trim_start_matches('#');
        if s.len() == 6 {
            u32::from_str_radix(s, 16).ok()
        } else {
            None
        }
    };
    let color = parse_hex(hex)
        .map(|c| gpui::rgb(c).into())
        .unwrap_or_else(|| theme.rgb(theme.crosshair));
    (color, dash)
}

/// Paints a box label: rounded dark chip + colored bold text at (p1.x, y1-15),
/// flipping below the box top edge when it would clip above the canvas.
fn paint_label(window: &mut Window, cx: &mut App, theme: &crate::ui::theme::Theme, p1: Point<Pixels>, text: &str, color: Hsla) {
    let style = window.text_style();
    let mut run = TextRun {
        len: text.len(),
        font: style.font(),
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    run.font.weight = gpui::FontWeight::SEMIBOLD;
    let font_size = px(11.0);
    let line = window.text_system().shape_line(text.to_string().into(), font_size, &[run], None);
    let text_w = line.width;
    let text_h = px(14.0);
    let pad = px(4.0);
    let y = p1.y - px(17.0);
    let y = if f32::from(y) < 0.0 { p1.y + px(4.0) } else { y };
    let chip = Bounds::new(
        Point { x: p1.x, y },
        size(text_w + pad * 2.0, text_h),
    );
    window.paint_quad(quad(
        chip,
        gpui::Corners::from(px(3.)),
        theme.rgba(0x000000, 0.55),
        gpui::Edges::from(px(0.)),
        gpui::transparent_black(),
        gpui::BorderStyle::Solid,
    ));
    let _ = line.paint(Point { x: chip.origin.x + pad, y: chip.origin.y + px(0.5) }, text_h, window, cx);
}
