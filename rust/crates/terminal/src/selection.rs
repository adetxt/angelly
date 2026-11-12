use std::{ffi::c_void, time::Duration};

use vt_sys as sys;

use crate::{ffi::*, term::Term};

/// Pointer position for selection gestures: the viewport cell under the
/// pointer plus the surface-space pixel position (relative to the top-left of
/// the view, padding included).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pointer {
    pub col: u16,
    pub row: u16,
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Autoscroll {
    Up,
    Down,
}

/// Keyboard-driven movement of the selection's moving end.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SelectionAdjust {
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    LineStart,
    LineEnd,
}

impl SelectionAdjust {
    fn raw(self) -> sys::GhosttySelectionAdjust::Type {
        use sys::GhosttySelectionAdjust as A;
        match self {
            SelectionAdjust::Left => A::GHOSTTY_SELECTION_ADJUST_LEFT,
            SelectionAdjust::Right => A::GHOSTTY_SELECTION_ADJUST_RIGHT,
            SelectionAdjust::Up => A::GHOSTTY_SELECTION_ADJUST_UP,
            SelectionAdjust::Down => A::GHOSTTY_SELECTION_ADJUST_DOWN,
            SelectionAdjust::Home => A::GHOSTTY_SELECTION_ADJUST_HOME,
            SelectionAdjust::End => A::GHOSTTY_SELECTION_ADJUST_END,
            SelectionAdjust::PageUp => A::GHOSTTY_SELECTION_ADJUST_PAGE_UP,
            SelectionAdjust::PageDown => A::GHOSTTY_SELECTION_ADJUST_PAGE_DOWN,
            SelectionAdjust::LineStart => A::GHOSTTY_SELECTION_ADJUST_BEGINNING_OF_LINE,
            SelectionAdjust::LineEnd => A::GHOSTTY_SELECTION_ADJUST_END_OF_LINE,
        }
    }
}

/// How selected or screen text is rendered.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TextFormat {
    Plain,
    /// Plain text with the escape sequences that reproduce colors and styles.
    Vt,
    Html,
}

impl TextFormat {
    fn raw(self) -> sys::GhosttyFormatterFormat::Type {
        use sys::GhosttyFormatterFormat as F;
        match self {
            TextFormat::Plain => F::GHOSTTY_FORMATTER_FORMAT_PLAIN,
            TextFormat::Vt => F::GHOSTTY_FORMATTER_FORMAT_VT,
            TextFormat::Html => F::GHOSTTY_FORMATTER_FORMAT_HTML,
        }
    }
}

/// The part of the screen [`Term::screen_text`] dumps.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScreenPart {
    /// The lines that scrolled off the top.
    Scrollback,
    /// The scrollback and the visible lines.
    Screen,
}

/// Characters that end a word, as in Ghostty's `selection-word-chars` default.
pub const DEFAULT_WORD_BOUNDARIES: &str = " \t'\"\u{2502}`|:;,()[]{}<>$";

/// What a press does on repeated clicks and where words end.
#[derive(Clone, Debug, Default)]
pub struct PressOptions {
    /// The third click selects the command output under the pointer instead of the line
    /// (Ghostty uses this while Ctrl or Super is held).
    pub output_on_triple: bool,
    /// Word boundary characters; `None` keeps the built-in defaults.
    pub word_boundaries: Option<String>,
}

/// Longest gap between clicks that still counts as a double/triple click.
pub const REPEAT_CLICK_INTERVAL: Duration = Duration::from_millis(500);

struct GestureEvent(sys::GhosttySelectionGestureEvent);

impl GestureEvent {
    fn new(ty: sys::GhosttySelectionGestureEventType::Type) -> GestureEvent {
        let mut ev = std::ptr::null_mut();
        unsafe {
            check(
                sys::ghostty_selection_gesture_event_new(std::ptr::null(), &mut ev, ty as _),
                "gesture_event_new",
            );
        }
        GestureEvent(ev)
    }

    fn set<T>(&mut self, opt: sys::GhosttySelectionGestureEventOption::Type, v: &T) {
        unsafe {
            sys::ghostty_selection_gesture_event_set(
                self.0,
                opt as _,
                v as *const T as *const c_void,
            );
        }
    }
}

impl Drop for GestureEvent {
    fn drop(&mut self) {
        unsafe { sys::ghostty_selection_gesture_event_free(self.0) }
    }
}

impl Term {
    fn current_selection(&self) -> Option<sys::GhosttySelection> {
        let mut sel = sized::<sys::GhosttySelection>(|s, n| s.size = n);
        let r = unsafe {
            sys::ghostty_terminal_get(
                self.raw,
                sys::GhosttyTerminalData::GHOSTTY_TERMINAL_DATA_SELECTION as _,
                &mut sel as *mut _ as *mut c_void,
            )
        };
        (r == OK).then_some(sel)
    }

    pub fn has_selection(&self) -> bool {
        self.current_selection().is_some()
    }

    pub fn clear_selection(&mut self) {
        unsafe {
            sys::ghostty_terminal_set(
                self.raw,
                sys::GhosttyTerminalOption::GHOSTTY_TERMINAL_OPT_SELECTION as _,
                std::ptr::null(),
            );
        }
    }

    fn install_selection(&mut self, sel: &sys::GhosttySelection) {
        unsafe {
            sys::ghostty_terminal_set(
                self.raw,
                sys::GhosttyTerminalOption::GHOSTTY_TERMINAL_OPT_SELECTION as _,
                sel as *const _ as *const c_void,
            );
        }
    }

    pub fn select_all(&mut self) -> bool {
        let mut sel = sized::<sys::GhosttySelection>(|s, n| s.size = n);
        if unsafe { sys::ghostty_terminal_select_all(self.raw, &mut sel) } != OK {
            return false;
        }
        self.install_selection(&sel);
        true
    }

    /// Selects the word under a viewport cell.
    pub fn select_word(&mut self, col: u16, row: u16) -> bool {
        let Some(r) = self.viewport_ref(col, row) else {
            return false;
        };
        let opts = sized::<sys::GhosttyTerminalSelectWordOptions>(|o, n| {
            o.size = n;
            o.ref_ = r;
        });
        let mut sel = sized::<sys::GhosttySelection>(|s, n| s.size = n);
        if unsafe { sys::ghostty_terminal_select_word(self.raw, &opts, &mut sel) } != OK {
            return false;
        }
        self.install_selection(&sel);
        true
    }

    /// Selects the logical line (following soft wraps) under a viewport cell.
    pub fn select_line(&mut self, col: u16, row: u16) -> bool {
        let Some(r) = self.viewport_ref(col, row) else {
            return false;
        };
        let opts = sized::<sys::GhosttyTerminalSelectLineOptions>(|o, n| {
            o.size = n;
            o.ref_ = r;
        });
        let mut sel = sized::<sys::GhosttySelection>(|s, n| s.size = n);
        if unsafe { sys::ghostty_terminal_select_line(self.raw, &opts, &mut sel) } != OK {
            return false;
        }
        self.install_selection(&sel);
        true
    }

    /// Selects between two viewport cells (inclusive); `rectangle` makes it a
    /// block selection.
    pub fn select_range(&mut self, start: (u16, u16), end: (u16, u16), rectangle: bool) -> bool {
        let (Some(s), Some(e)) = (
            self.viewport_ref(start.0, start.1),
            self.viewport_ref(end.0, end.1),
        ) else {
            return false;
        };
        let sel = sized::<sys::GhosttySelection>(|sel, n| {
            sel.size = n;
            sel.start = s;
            sel.end = e;
            sel.rectangle = rectangle;
        });
        self.install_selection(&sel);
        true
    }

    /// Moves the moving end of the current selection.
    pub fn adjust_selection(&mut self, adjust: SelectionAdjust) -> bool {
        let Some(mut sel) = self.current_selection() else {
            return false;
        };
        if unsafe { sys::ghostty_terminal_selection_adjust(self.raw, &mut sel, adjust.raw() as _) }
            != OK
        {
            return false;
        }
        self.install_selection(&sel);
        true
    }

    /// The selected text as it would be copied: soft wraps joined, trailing
    /// blanks trimmed.
    pub fn selection_text(&self) -> Option<String> {
        self.format_selection(None, TextFormat::Plain, true)
    }

    /// Mouse-down in a selection gesture: single/double/triple clicks select
    /// nothing/word/line, repeated clicks are detected by time and distance.
    pub fn select_press(&mut self, p: Pointer) {
        self.select_press_with(p, &PressOptions::default());
    }

    /// [`select_press`](Self::select_press) with the output-selecting triple click and custom
    /// word boundaries.
    pub fn select_press_with(&mut self, p: Pointer, options: &PressOptions) {
        let Some(r) = self.viewport_ref(p.col, p.row) else {
            return;
        };
        let mut ev = GestureEvent::new(
            sys::GhosttySelectionGestureEventType::GHOSTTY_SELECTION_GESTURE_EVENT_TYPE_PRESS,
        );
        let pos = sys::GhosttySurfacePosition { x: p.x, y: p.y };
        let now = self.elapsed_ns();
        let repeat_distance = self.size.cell_width.max(1) as f64;
        let repeat_interval = REPEAT_CLICK_INTERVAL.as_nanos() as u64;
        use sys::GhosttySelectionGestureEventOption as O;
        ev.set(O::GHOSTTY_SELECTION_GESTURE_EVENT_OPT_REF, &r);
        ev.set(O::GHOSTTY_SELECTION_GESTURE_EVENT_OPT_POSITION, &pos);
        ev.set(O::GHOSTTY_SELECTION_GESTURE_EVENT_OPT_TIME_NS, &now);
        ev.set(
            O::GHOSTTY_SELECTION_GESTURE_EVENT_OPT_REPEAT_DISTANCE,
            &repeat_distance,
        );
        ev.set(
            O::GHOSTTY_SELECTION_GESTURE_EVENT_OPT_REPEAT_INTERVAL_NS,
            &repeat_interval,
        );
        let behaviors = sys::GhosttySelectionGestureBehaviors {
            single_click: sys::GhosttySelectionGestureBehavior::GHOSTTY_SELECTION_GESTURE_BEHAVIOR_CELL,
            double_click: sys::GhosttySelectionGestureBehavior::GHOSTTY_SELECTION_GESTURE_BEHAVIOR_WORD,
            triple_click: if options.output_on_triple {
                sys::GhosttySelectionGestureBehavior::GHOSTTY_SELECTION_GESTURE_BEHAVIOR_OUTPUT
            } else {
                sys::GhosttySelectionGestureBehavior::GHOSTTY_SELECTION_GESTURE_BEHAVIOR_LINE
            },
        };
        ev.set(O::GHOSTTY_SELECTION_GESTURE_EVENT_OPT_BEHAVIORS, &behaviors);
        let boundaries = options.word_boundaries.as_deref().map(codepoints);
        let boundaries = boundaries.as_ref().map(|b| sys::GhosttyCodepoints { ptr: b.as_ptr(), len: b.len() });
        if let Some(b) = &boundaries {
            ev.set(O::GHOSTTY_SELECTION_GESTURE_EVENT_OPT_WORD_BOUNDARY_CODEPOINTS, b);
        }
        match self.apply_gesture(&ev) {
            Some(sel) => self.install_selection(&sel),
            None => self.clear_selection(),
        }
    }

    /// Pointer moved with the button held; extends the selection.
    pub fn select_drag(&mut self, p: Pointer, rectangle: bool) {
        let Some(r) = self.viewport_ref(p.col, p.row) else {
            return;
        };
        let mut ev = GestureEvent::new(
            sys::GhosttySelectionGestureEventType::GHOSTTY_SELECTION_GESTURE_EVENT_TYPE_DRAG,
        );
        let pos = sys::GhosttySurfacePosition { x: p.x, y: p.y };
        let geometry = self.gesture_geometry();
        use sys::GhosttySelectionGestureEventOption as O;
        ev.set(O::GHOSTTY_SELECTION_GESTURE_EVENT_OPT_REF, &r);
        ev.set(O::GHOSTTY_SELECTION_GESTURE_EVENT_OPT_POSITION, &pos);
        ev.set(O::GHOSTTY_SELECTION_GESTURE_EVENT_OPT_GEOMETRY, &geometry);
        ev.set(O::GHOSTTY_SELECTION_GESTURE_EVENT_OPT_RECTANGLE, &rectangle);
        if let Some(sel) = self.apply_gesture(&ev) {
            self.install_selection(&sel);
        }
    }

    /// Mouse-up. `cell` is the viewport cell under the pointer, if any.
    pub fn select_release(&mut self, cell: Option<(u16, u16)>) {
        let mut ev = GestureEvent::new(
            sys::GhosttySelectionGestureEventType::GHOSTTY_SELECTION_GESTURE_EVENT_TYPE_RELEASE,
        );
        if let Some(r) = cell.and_then(|(c, r)| self.viewport_ref(c, r)) {
            ev.set(
                sys::GhosttySelectionGestureEventOption::GHOSTTY_SELECTION_GESTURE_EVENT_OPT_REF,
                &r,
            );
        }
        self.apply_gesture(&ev);
    }

    /// Whether an in-progress drag wants the viewport scrolled (pointer is
    /// above/below the grid).
    pub fn selection_autoscroll(&self) -> Option<Autoscroll> {
        let v = self.gesture_get::<sys::GhosttySelectionGestureAutoscroll::Type>(
            sys::GhosttySelectionGestureData::GHOSTTY_SELECTION_GESTURE_DATA_AUTOSCROLL,
        )?;
        match v {
            sys::GhosttySelectionGestureAutoscroll::GHOSTTY_SELECTION_GESTURE_AUTOSCROLL_UP => {
                Some(Autoscroll::Up)
            }
            sys::GhosttySelectionGestureAutoscroll::GHOSTTY_SELECTION_GESTURE_AUTOSCROLL_DOWN => {
                Some(Autoscroll::Down)
            }
            _ => None,
        }
    }

    /// Call on a timer after scrolling for [`selection_autoscroll`](Self::selection_autoscroll)
    /// so the selection follows the newly revealed rows. `p` is the pointer
    /// clamped to the viewport.
    pub fn select_autoscroll_tick(&mut self, p: Pointer, rectangle: bool) {
        let mut ev = GestureEvent::new(sys::GhosttySelectionGestureEventType::GHOSTTY_SELECTION_GESTURE_EVENT_TYPE_AUTOSCROLL_TICK);
        let pos = sys::GhosttySurfacePosition { x: p.x, y: p.y };
        let coord = sys::GhosttyPointCoordinate {
            x: p.col,
            y: p.row as u32,
        };
        let geometry = self.gesture_geometry();
        use sys::GhosttySelectionGestureEventOption as O;
        ev.set(O::GHOSTTY_SELECTION_GESTURE_EVENT_OPT_VIEWPORT, &coord);
        ev.set(O::GHOSTTY_SELECTION_GESTURE_EVENT_OPT_POSITION, &pos);
        ev.set(O::GHOSTTY_SELECTION_GESTURE_EVENT_OPT_GEOMETRY, &geometry);
        ev.set(O::GHOSTTY_SELECTION_GESTURE_EVENT_OPT_RECTANGLE, &rectangle);
        if let Some(sel) = self.apply_gesture(&ev) {
            self.install_selection(&sel);
        }
    }

    /// The OSC 8 hyperlink target under a viewport cell.
    pub fn hyperlink_at(&self, col: u16, row: u16) -> Option<String> {
        let r = self.viewport_ref(col, row)?;
        let mut buf = [0u8; 512];
        let mut len = 0usize;
        let res = unsafe {
            sys::ghostty_grid_ref_hyperlink_uri(&r, buf.as_mut_ptr(), buf.len(), &mut len)
        };
        match res {
            x if x == OK && len > 0 => Some(String::from_utf8_lossy(&buf[..len]).into_owned()),
            x if x == sys::GhosttyResult::GHOSTTY_OUT_OF_SPACE => {
                let mut big = vec![0u8; len];
                let res = unsafe {
                    sys::ghostty_grid_ref_hyperlink_uri(&r, big.as_mut_ptr(), big.len(), &mut len)
                };
                (res == OK).then(|| String::from_utf8_lossy(&big[..len]).into_owned())
            }
            _ => None,
        }
    }

    fn apply_gesture(&mut self, ev: &GestureEvent) -> Option<sys::GhosttySelection> {
        let mut sel = sized::<sys::GhosttySelection>(|s, n| s.size = n);
        let r =
            unsafe { sys::ghostty_selection_gesture_event(self.gesture, self.raw, ev.0, &mut sel) };
        (r == OK).then_some(sel)
    }

    fn gesture_get<T: Default>(&self, data: sys::GhosttySelectionGestureData::Type) -> Option<T> {
        let mut v = T::default();
        let r = unsafe {
            sys::ghostty_selection_gesture_get(
                self.gesture,
                self.raw,
                data as _,
                &mut v as *mut T as *mut c_void,
            )
        };
        (r == OK).then_some(v)
    }

    fn gesture_geometry(&self) -> sys::GhosttySelectionGestureGeometry {
        let (s, p) = (self.size, self.padding);
        sys::GhosttySelectionGestureGeometry {
            columns: s.cols as u32,
            cell_width: s.cell_width.max(1),
            padding_left: p.left,
            screen_height: (s.rows as u32 * s.cell_height + p.top + p.bottom).max(1),
        }
    }
}

/// The boundary set as codepoints, always including NUL (an empty cell).
fn codepoints(chars: &str) -> Vec<u32> {
    let mut out = vec![0];
    out.extend(chars.chars().map(u32::from));
    out
}

/// Moves `from` (column, screen row) to the end of the next word or of the previous one, so
/// that moving the end of a selection forward and back retraces its steps. `is_boundary` tells
/// whether a cell separates words. Stops at the screen edges.
pub(crate) fn word_move(
    from: (u16, u32),
    forward: bool,
    cols: u16,
    rows: u32,
    is_boundary: impl Fn(u16, u32) -> bool,
) -> (u16, u32) {
    let last_col = cols.saturating_sub(1);
    let step = |(x, y): (u16, u32)| -> Option<(u16, u32)> {
        if forward {
            if x < last_col {
                Some((x + 1, y))
            } else {
                (y + 1 < rows).then_some((0, y + 1))
            }
        } else if x > 0 {
            Some((x - 1, y))
        } else {
            y.checked_sub(1).map(|y| (last_col, y))
        }
    };
    let mut at = from;
    if forward {
        let Some(first) = step(at) else { return from };
        at = first;
        while is_boundary(at.0, at.1) {
            match step(at) {
                Some(next) => at = next,
                None => return at,
            }
        }
        while let Some(next) = step(at).filter(|n| !is_boundary(n.0, n.1)) {
            at = next;
        }
    } else {
        while !is_boundary(at.0, at.1) {
            match step(at) {
                Some(next) => at = next,
                None => return at,
            }
        }
        while is_boundary(at.0, at.1) {
            match step(at) {
                Some(next) => at = next,
                None => return at,
            }
        }
    }
    at
}

impl Term {
    /// Selects the command output (between OSC 133 marks) under a viewport cell. Needs shell
    /// integration; returns false when the cell is not in command output.
    pub fn select_output(&mut self, col: u16, row: u16) -> bool {
        let Some(r) = self.viewport_ref(col, row) else {
            return false;
        };
        let mut sel = sized::<sys::GhosttySelection>(|s, n| s.size = n);
        if unsafe { sys::ghostty_terminal_select_output(self.raw, r, &mut sel) } != OK {
            return false;
        }
        self.install_selection(&sel);
        true
    }

    fn format_selection(
        &self,
        selection: Option<&sys::GhosttySelection>,
        format: TextFormat,
        trim: bool,
    ) -> Option<String> {
        let opts = sized::<sys::GhosttyTerminalSelectionFormatOptions>(|o, n| {
            o.size = n;
            o.emit = format.raw() as _;
            o.unwrap = true;
            o.trim = trim;
            o.selection = selection.map_or(std::ptr::null(), |s| s as *const _);
        });
        let mut ptr = std::ptr::null_mut();
        let mut len = 0usize;
        let r = unsafe {
            sys::ghostty_terminal_selection_format_alloc(self.raw, std::ptr::null(), opts, &mut ptr, &mut len)
        };
        if r != OK {
            return None;
        }
        // Empty selections succeed with a null pointer and zero length.
        let text = unsafe { string(sys::GhosttyString { ptr, len }) };
        unsafe { sys::ghostty_free(std::ptr::null(), ptr, len) };
        Some(text)
    }

    /// The selected text in `format`, with trailing blanks on each line removed when `trim`;
    /// `None` without a selection.
    pub fn selection_text_as(&self, format: TextFormat, trim: bool) -> Option<String> {
        self.format_selection(None, format, trim)
    }

    /// The text of `part` of the screen, with soft wraps joined and trailing blanks kept, as
    /// Ghostty's `write_*_file` actions dump it. `None` when `part` is empty.
    pub fn screen_text(&self, part: ScreenPart, format: TextFormat) -> Option<String> {
        let mut sel = sized::<sys::GhosttySelection>(|s, n| s.size = n);
        match part {
            ScreenPart::Screen => {
                if unsafe { sys::ghostty_terminal_select_all(self.raw, &mut sel) } != OK {
                    return None;
                }
            }
            ScreenPart::Scrollback => {
                let history = self.scrollbar().total.checked_sub(u64::from(self.size.rows))?.checked_sub(1)?;
                sel.start = self.grid_ref(sys::GhosttyPointTag::GHOSTTY_POINT_TAG_HISTORY, 0, 0)?;
                sel.end = self.grid_ref(
                    sys::GhosttyPointTag::GHOSTTY_POINT_TAG_HISTORY,
                    self.size.cols.saturating_sub(1),
                    u32::try_from(history).ok()?,
                )?;
            }
        }
        self.format_selection(Some(&sel), format, false)
    }

    fn screen_point(&self, r: &sys::GhosttyGridRef) -> Option<(u16, u32)> {
        let mut out = sys::GhosttyPointCoordinate::default();
        let res = unsafe {
            sys::ghostty_terminal_point_from_grid_ref(
                self.raw,
                r,
                sys::GhosttyPointTag::GHOSTTY_POINT_TAG_SCREEN,
                &mut out,
            )
        };
        (res == OK).then_some((out.x, out.y))
    }

    fn screen_ref(&self, col: u16, row: u32) -> Option<sys::GhosttyGridRef> {
        let point = sys::GhosttyPoint {
            tag: sys::GhosttyPointTag::GHOSTTY_POINT_TAG_SCREEN,
            value: sys::GhosttyPointValue {
                coordinate: sys::GhosttyPointCoordinate { x: col, y: row },
            },
        };
        let mut r = sized::<sys::GhosttyGridRef>(|r, n| r.size = n);
        (unsafe { sys::ghostty_terminal_grid_ref(self.raw, point, &mut r) } == OK).then_some(r)
    }

    fn cell_codepoint(&self, col: u16, row: u32) -> u32 {
        let Some(r) = self.screen_ref(col, row) else { return 0 };
        let mut cp = 0u32;
        let mut len = 0usize;
        let res = unsafe { sys::ghostty_grid_ref_graphemes(&r, &mut cp, 1, &mut len) };
        if (res == OK || res == sys::GhosttyResult::GHOSTTY_OUT_OF_SPACE) && len > 0 {
            cp
        } else {
            0
        }
    }

    /// Scrolls the viewport so the top of the selection is its first row. False without a
    /// selection.
    pub fn scroll_to_selection(&mut self) -> bool {
        let Some(sel) = self.current_selection() else { return false };
        let (Some(a), Some(b)) = (self.screen_point(&sel.start), self.screen_point(&sel.end)) else {
            return false;
        };
        self.scroll_to_row(a.1.min(b.1) as usize);
        true
    }

    /// Moves the selection's moving end to the start of the previous word (`forward == false`)
    /// or the end of the next one. `boundaries` are the word separator characters (see
    /// [`DEFAULT_WORD_BOUNDARIES`]).
    pub fn adjust_selection_by_word(&mut self, forward: bool, boundaries: &str) -> bool {
        let Some(mut sel) = self.current_selection() else { return false };
        let Some(end) = self.screen_point(&sel.end) else { return false };
        let separators = codepoints(boundaries);
        let rows = u32::try_from(self.scrollbar().total).unwrap_or(u32::MAX);
        let to = word_move(end, forward, self.size.cols, rows, |x, y| {
            separators.contains(&self.cell_codepoint(x, y))
        });
        let Some(r) = self.screen_ref(to.0, to.1) else { return false };
        sel.end = r;
        self.install_selection(&sel);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid<'a>(lines: &'a [&'a str]) -> impl Fn(u16, u32) -> bool + 'a {
        move |x, y| {
            lines
                .get(y as usize)
                .and_then(|l| l.chars().nth(usize::from(x)))
                .is_none_or(|c| c == ' ')
        }
    }

    #[test]
    fn word_move_right_lands_on_the_end_of_the_next_word() {
        let lines = ["foo bar  baz"];
        let g = grid(&lines);
        assert_eq!(word_move((0, 0), true, 12, 1, &g), (2, 0));
        assert_eq!(word_move((2, 0), true, 12, 1, &g), (6, 0));
        assert_eq!(word_move((6, 0), true, 12, 1, &g), (11, 0));
        assert_eq!(word_move((11, 0), true, 12, 1, &g), (11, 0));
    }

    #[test]
    fn word_move_left_lands_on_the_end_of_the_previous_word() {
        let lines = ["foo bar  baz"];
        let g = grid(&lines);
        assert_eq!(word_move((11, 0), false, 12, 1, &g), (6, 0));
        assert_eq!(word_move((6, 0), false, 12, 1, &g), (2, 0));
        assert_eq!(word_move((8, 0), false, 12, 1, &g), (6, 0));
        assert_eq!(word_move((2, 0), false, 12, 1, &g), (0, 0));
        assert_eq!(word_move((0, 0), false, 12, 1, &g), (0, 0));
    }

    #[test]
    fn word_move_crosses_rows() {
        let lines = ["ab  ", "cd  "];
        let g = grid(&lines);
        assert_eq!(word_move((1, 0), true, 4, 2, &g), (1, 1));
        assert_eq!(word_move((0, 1), false, 4, 2, &g), (1, 0));
    }

    #[test]
    fn default_boundaries_match_ghosttys() {
        assert_eq!(codepoints(DEFAULT_WORD_BOUNDARIES).len(), 20);
        assert!(codepoints(DEFAULT_WORD_BOUNDARIES).contains(&0x2502));
    }
}
