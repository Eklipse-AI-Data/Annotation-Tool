//! Themed multi-line text box (no soft wrap): Enter inserts a line break,
//! pasted text keeps its lines, Up/Down move between lines, and the box
//! scrolls to keep the cursor visible. Built on the same pattern as
//! `entry.rs` and reuses its editing actions under the "TextArea" key
//! context. Emits `EntryChanged` on every content change.

use gpui::{
    actions, div, fill, point, prelude::*, px, size, App, Bounds, ClipboardItem, Context, Element,
    ElementId, ElementInputHandler, Entity, EntityInputHandler, FocusHandle, Focusable,
    GlobalElementId, Hsla, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad,
    Pixels, Point, ScrollHandle, ShapedLine, SharedString, TextRun, TextStyle, UTF16Selection,
    Window,
};
use unicode_segmentation::UnicodeSegmentation;

use super::entry::{
    Backspace, CopyEntry, CutEntry, Delete, EntryChanged, EntryEnd, EntryHome, EntryLeft,
    EntryRight, PasteEntry, SelectAllEntry, SelectLeft, SelectRight,
};
use super::theme::Theme;

actions!(text_area, [Newline, LineUp, LineDown, SelectUp, SelectDown]);

impl gpui::EventEmitter<EntryChanged> for TextArea {}

pub struct TextArea {
    focus_handle: FocusHandle,
    content: String,
    pub placeholder: SharedString,
    selected_range: std::ops::Range<usize>,
    selection_reversed: bool,
    marked_range: Option<std::ops::Range<usize>>,
    last_lines: Vec<ShapedLine>,
    last_bounds: Option<Bounds<Pixels>>,
    last_line_height: Pixels,
    is_selecting: bool,
    scroll: ScrollHandle,
    scroll_to_cursor: bool,
    /// Visible height of the box.
    pub height: Pixels,
}

impl TextArea {
    pub fn new(placeholder: &str, cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            content: String::new(),
            placeholder: placeholder.to_string().into(),
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            last_lines: Vec::new(),
            last_bounds: None,
            last_line_height: px(18.),
            is_selecting: false,
            scroll: ScrollHandle::new(),
            scroll_to_cursor: false,
            height: px(160.),
        }
    }

    pub fn text(&self) -> String {
        self.content.clone()
    }

    pub fn set_text(&mut self, text: &str, cx: &mut Context<Self>) {
        self.content = normalize(text);
        self.selected_range = self.content.len()..self.content.len();
        self.marked_range = None;
        self.scroll_to_cursor = true;
        cx.notify();
        cx.emit(EntryChanged);
    }

    // ---- line geometry ----

    fn line_starts(&self) -> Vec<usize> {
        std::iter::once(0)
            .chain(self.content.match_indices('\n').map(|(i, _)| i + 1))
            .collect()
    }

    /// (row, row start offset, row end offset) for a byte offset.
    fn row_of(&self, offset: usize) -> (usize, usize, usize) {
        let starts = self.line_starts();
        let row = starts.partition_point(|&s| s <= offset).saturating_sub(1);
        let start = starts[row];
        let end = self.content[start..].find('\n').map_or(self.content.len(), |i| start + i);
        (row, start, end)
    }

    fn x_for_offset(&self, offset: usize) -> Pixels {
        let (row, start, _) = self.row_of(offset);
        self.last_lines.get(row).map_or(px(0.), |l| l.x_for_index(offset - start))
    }

    fn offset_for_row_x(&self, row: usize, x: Pixels) -> usize {
        let starts = self.line_starts();
        let Some(&start) = starts.get(row) else {
            return self.content.len();
        };
        let end = self.content[start..].find('\n').map_or(self.content.len(), |i| start + i);
        let col = self.last_lines.get(row).map_or(0, |l| l.closest_index_for_x(x));
        (start + col).min(end)
    }

    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        let Some(bounds) = self.last_bounds else {
            return 0;
        };
        if self.content.is_empty() {
            return 0;
        }
        let y = position.y - bounds.top();
        if y < px(0.) {
            return 0;
        }
        let row = (y / self.last_line_height).floor() as usize;
        if row >= self.line_starts().len() {
            return self.content.len();
        }
        self.offset_for_row_x(row, position.x - bounds.left())
    }

    // ---- selection helpers ----

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.selected_range = offset..offset;
        self.selection_reversed = false;
        self.scroll_to_cursor = true;
        cx.notify()
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        if self.selection_reversed {
            self.selected_range.start = offset
        } else {
            self.selected_range.end = offset
        };
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        self.scroll_to_cursor = true;
        cx.notify()
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .rev()
            .find_map(|(idx, _)| (idx < offset).then_some(idx))
            .unwrap_or(0)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .find_map(|(idx, _)| (idx > offset).then_some(idx))
            .unwrap_or(self.content.len())
    }

    fn vertical_target(&self, down: bool) -> usize {
        let cursor = self.cursor_offset();
        let (row, _, _) = self.row_of(cursor);
        let rows = self.line_starts().len();
        if !down && row == 0 {
            return 0;
        }
        if down && row + 1 >= rows {
            return self.content.len();
        }
        let x = self.x_for_offset(cursor);
        self.offset_for_row_x(if down { row + 1 } else { row - 1 }, x)
    }

    // ---- UTF-16 conversion (platform input handler) ----

    fn offset_from_utf16(&self, offset: usize) -> usize {
        let mut utf8 = 0;
        let mut utf16 = 0;
        for ch in self.content.chars() {
            if utf16 >= offset {
                break;
            }
            utf16 += ch.len_utf16();
            utf8 += ch.len_utf8();
        }
        utf8
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        let mut utf16 = 0;
        let mut utf8 = 0;
        for ch in self.content.chars() {
            if utf8 >= offset {
                break;
            }
            utf8 += ch.len_utf8();
            utf16 += ch.len_utf16();
        }
        utf16
    }

    fn range_to_utf16(&self, r: &std::ops::Range<usize>) -> std::ops::Range<usize> {
        self.offset_to_utf16(r.start)..self.offset_to_utf16(r.end)
    }

    fn range_from_utf16(&self, r: &std::ops::Range<usize>) -> std::ops::Range<usize> {
        self.offset_from_utf16(r.start)..self.offset_from_utf16(r.end)
    }

    // ---- action handlers ----

    fn newline(&mut self, _: &Newline, window: &mut Window, cx: &mut Context<Self>) {
        self.replace_text_in_range(None, "\n", window, cx);
    }

    fn line_up(&mut self, _: &LineUp, _: &mut Window, cx: &mut Context<Self>) {
        let t = self.vertical_target(false);
        self.move_to(t, cx);
    }

    fn line_down(&mut self, _: &LineDown, _: &mut Window, cx: &mut Context<Self>) {
        let t = self.vertical_target(true);
        self.move_to(t, cx);
    }

    fn select_up(&mut self, _: &SelectUp, _: &mut Window, cx: &mut Context<Self>) {
        let t = self.vertical_target(false);
        self.select_to(t, cx);
    }

    fn select_down(&mut self, _: &SelectDown, _: &mut Window, cx: &mut Context<Self>) {
        let t = self.vertical_target(true);
        self.select_to(t, cx);
    }

    fn left(&mut self, _: &EntryLeft, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.previous_boundary(self.cursor_offset()), cx);
        } else {
            self.move_to(self.selected_range.start, cx)
        }
    }

    fn right(&mut self, _: &EntryRight, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.next_boundary(self.selected_range.end), cx);
        } else {
            self.move_to(self.selected_range.end, cx)
        }
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.previous_boundary(self.cursor_offset()), cx)
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_boundary(self.cursor_offset()), cx)
    }

    fn select_all(&mut self, _: &SelectAllEntry, _: &mut Window, cx: &mut Context<Self>) {
        self.selected_range = 0..self.content.len();
        self.selection_reversed = false;
        cx.notify();
    }

    fn home(&mut self, _: &EntryHome, _: &mut Window, cx: &mut Context<Self>) {
        let (_, start, _) = self.row_of(self.cursor_offset());
        self.move_to(start, cx);
    }

    fn end(&mut self, _: &EntryEnd, _: &mut Window, cx: &mut Context<Self>) {
        let (_, _, end) = self.row_of(self.cursor_offset());
        self.move_to(end, cx);
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(self.previous_boundary(self.cursor_offset()), cx)
        }
        self.replace_text_in_range(None, "", window, cx)
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(self.next_boundary(self.cursor_offset()), cx)
        }
        self.replace_text_in_range(None, "", window, cx)
    }

    fn paste(&mut self, _: &PasteEntry, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.replace_text_in_range(None, &text, window, cx);
        }
    }

    fn copy(&mut self, _: &CopyEntry, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
        }
    }

    fn cut(&mut self, _: &CutEntry, window: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
            self.replace_text_in_range(None, "", window, cx)
        }
    }

    fn on_mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus_handle);
        self.is_selecting = true;
        let ix = self.index_for_mouse_position(event.position);
        if event.modifiers.shift {
            self.select_to(ix, cx);
        } else {
            self.move_to(ix, cx)
        }
        self.scroll_to_cursor = false;
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_selecting {
            self.select_to(self.index_for_mouse_position(event.position), cx);
        }
    }

    /// Nudges the scroll offset so the cursor's line is inside the viewport.
    fn apply_scroll_to_cursor(&mut self) {
        if !std::mem::take(&mut self.scroll_to_cursor) {
            return;
        }
        let viewport = self.scroll.bounds().size.height;
        if viewport <= px(0.) {
            return;
        }
        let lh = self.last_line_height;
        let (row, _, _) = self.row_of(self.cursor_offset());
        let top = lh * row as f32;
        let bottom = top + lh;
        let mut offset = self.scroll.offset();
        let visible_top = -offset.y;
        if top < visible_top {
            offset.y = -top;
        } else if bottom > visible_top + viewport {
            offset.y = -(bottom - viewport);
        } else {
            return;
        }
        self.scroll.set_offset(offset);
    }
}

/// CRLF / CR → LF so offsets and line splitting stay consistent.
fn normalize(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

impl Focusable for TextArea {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EntityInputHandler for TextArea {
    fn text_for_range(
        &mut self,
        range_utf16: std::ops::Range<usize>,
        actual_range: &mut Option<std::ops::Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        Some(self.content[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: self.selection_reversed,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<std::ops::Range<usize>> {
        self.marked_range.as_ref().map(|r| self.range_to_utf16(r))
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.marked_range = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<std::ops::Range<usize>>,
        new_text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|r| self.range_from_utf16(r))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());
        let new_text = normalize(new_text);
        self.content = self.content[..range.start].to_owned() + &new_text + &self.content[range.end..];
        let cursor = range.start + new_text.len();
        self.selected_range = cursor..cursor;
        self.selection_reversed = false;
        self.marked_range = None;
        self.scroll_to_cursor = true;
        cx.notify();
        cx.emit(EntryChanged);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<std::ops::Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<std::ops::Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|r| self.range_from_utf16(r))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());
        let new_text = normalize(new_text);
        self.content = self.content[..range.start].to_owned() + &new_text + &self.content[range.end..];
        self.marked_range = (!new_text.is_empty()).then(|| range.start..range.start + new_text.len());
        self.selected_range = new_selected_range_utf16
            .as_ref()
            .map(|r| self.range_from_utf16(r))
            .map(|r| r.start + range.start..r.end + range.start)
            .unwrap_or_else(|| range.start + new_text.len()..range.start + new_text.len());
        self.scroll_to_cursor = true;
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: std::ops::Range<usize>,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let range = self.range_from_utf16(&range_utf16);
        let (row, _, _) = self.row_of(range.start);
        let top = bounds.top() + self.last_line_height * row as f32;
        Some(Bounds::from_corners(
            point(bounds.left() + self.x_for_offset(range.start), top),
            point(bounds.left() + self.x_for_offset(range.end.max(range.start)), top + self.last_line_height),
        ))
    }

    fn character_index_for_point(
        &mut self,
        position: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        Some(self.offset_to_utf16(self.index_for_mouse_position(position)))
    }
}

struct TextAreaElement {
    input: Entity<TextArea>,
    theme: &'static Theme,
}

struct PrepaintState {
    lines: Vec<ShapedLine>,
    line_height: Pixels,
    cursor: Option<PaintQuad>,
    selections: Vec<PaintQuad>,
}

impl IntoElement for TextAreaElement {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TextAreaElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (gpui::LayoutId, Self::RequestLayoutState) {
        let rows = self.input.read(cx).content.split('\n').count().max(1);
        let mut style = gpui::Style::default();
        style.size.width = gpui::Length::Definite(gpui::DefiniteLength::Fraction(1.0_f32.into()));
        style.size.height = (window.line_height() * rows as f32).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let input = self.input.read(cx);
        let theme = self.theme;
        let style: TextStyle = window.text_style();
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line_height = window.line_height();

        let (source, color): (String, Hsla) = if input.content.is_empty() {
            (input.placeholder.to_string(), theme.rgb(theme.fg_muted))
        } else {
            (input.content.clone(), style.color)
        };
        let lines: Vec<ShapedLine> = source
            .split('\n')
            .map(|text| {
                let run = TextRun {
                    len: text.len(),
                    font: style.font(),
                    color,
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                window.text_system().shape_line(text.to_string().into(), font_size, &[run], None)
            })
            .collect();

        let mut selections = Vec::new();
        let mut cursor = None;
        if !input.content.is_empty() || input.selected_range.is_empty() {
            let sel = input.selected_range.clone();
            let cursor_off = input.cursor_offset();
            let mut start = 0usize;
            for (row, text) in input.content.split('\n').enumerate() {
                let end = start + text.len();
                let top = bounds.top() + line_height * row as f32;
                let line = &lines[row.min(lines.len() - 1)];
                if !sel.is_empty() && sel.start <= end && sel.end > start {
                    let a = sel.start.max(start) - start;
                    let b = sel.end.min(end) - start;
                    let mut right = bounds.left() + line.x_for_index(b);
                    if sel.end > end {
                        right += px(5.); // show the selected line break
                    }
                    selections.push(fill(
                        Bounds::from_corners(point(bounds.left() + line.x_for_index(a), top), point(right, top + line_height)),
                        theme.rgba(theme.selection, 0.55),
                    ));
                }
                if sel.is_empty() && cursor_off >= start && cursor_off <= end {
                    let x = if input.content.is_empty() { px(0.) } else { line.x_for_index(cursor_off - start) };
                    cursor = Some(fill(
                        Bounds::new(point(bounds.left() + x, top), size(px(1.5), line_height)),
                        theme.rgb(theme.fg_highlight),
                    ));
                }
                start = end + 1;
            }
        }
        PrepaintState { lines, line_height, cursor, selections }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.input.read(cx).focus_handle.clone();
        window.handle_input(&focus_handle, ElementInputHandler::new(bounds, self.input.clone()), cx);
        for quad in prepaint.selections.drain(..) {
            window.paint_quad(quad);
        }
        let lh = prepaint.line_height;
        for (row, line) in prepaint.lines.iter().enumerate() {
            let origin = point(bounds.left(), bounds.top() + lh * row as f32);
            line.paint(origin, lh, window, cx).ok();
        }
        if focus_handle.is_focused(window)
            && let Some(cursor) = prepaint.cursor.take()
        {
            window.paint_quad(cursor);
        }
        let lines = std::mem::take(&mut prepaint.lines);
        let is_placeholder = self.input.read(cx).content.is_empty();
        self.input.update(cx, |input, _| {
            input.last_lines = if is_placeholder { Vec::new() } else { lines };
            input.last_bounds = Some(bounds);
            input.last_line_height = lh;
        });
    }
}

impl Render for TextArea {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.apply_scroll_to_cursor();
        let theme = super::theme::ThemeState::current(cx);
        let focused = self.focus_handle.is_focused(window);
        let border = if focused { theme.rgb(theme.accent) } else { gpui::transparent_black() };
        div()
            .id("textarea-wrapper")
            .key_context("TextArea")
            .track_focus(&self.focus_handle)
            .cursor(gpui::CursorStyle::IBeam)
            .on_action(cx.listener(Self::newline))
            .on_action(cx.listener(Self::line_up))
            .on_action(cx.listener(Self::line_down))
            .on_action(cx.listener(Self::select_up))
            .on_action(cx.listener(Self::select_down))
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::copy))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .w_full()
            .h(self.height)
            .rounded(px(6.))
            .bg(theme.rgba(theme.fg_muted, 0.07))
            .border_1()
            .border_color(border)
            .px_2()
            .py_1()
            .text_size(px(12.5))
            .text_color(theme.rgb(theme.entry_fg))
            .child(
                div()
                    .id("textarea-scroll")
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .child(TextAreaElement { input: cx.entity(), theme }),
            )
    }
}

/// Keybindings for the multi-line box, registered at startup next to the
/// entry bindings. The "TextArea" context outranks the app-wide arrow/enter
/// shortcuts while the box has focus.
pub fn text_area_key_bindings() -> Vec<gpui::KeyBinding> {
    use gpui::KeyBinding;
    let ctx = Some("TextArea");
    vec![
        KeyBinding::new("enter", Newline, ctx),
        KeyBinding::new("shift-enter", Newline, ctx),
        KeyBinding::new("up", LineUp, ctx),
        KeyBinding::new("down", LineDown, ctx),
        KeyBinding::new("shift-up", SelectUp, ctx),
        KeyBinding::new("shift-down", SelectDown, ctx),
        KeyBinding::new("backspace", Backspace, ctx),
        KeyBinding::new("delete", Delete, ctx),
        KeyBinding::new("left", EntryLeft, ctx),
        KeyBinding::new("right", EntryRight, ctx),
        KeyBinding::new("shift-left", SelectLeft, ctx),
        KeyBinding::new("shift-right", SelectRight, ctx),
        KeyBinding::new("ctrl-a", SelectAllEntry, ctx),
        KeyBinding::new("cmd-a", SelectAllEntry, ctx),
        KeyBinding::new("home", EntryHome, ctx),
        KeyBinding::new("end", EntryEnd, ctx),
        KeyBinding::new("ctrl-v", PasteEntry, ctx),
        KeyBinding::new("cmd-v", PasteEntry, ctx),
        KeyBinding::new("ctrl-c", CopyEntry, ctx),
        KeyBinding::new("cmd-c", CopyEntry, ctx),
        KeyBinding::new("ctrl-x", CutEntry, ctx),
        KeyBinding::new("cmd-x", CutEntry, ctx),
    ]
}
