use std::{
    collections::VecDeque,
    ffi::c_void,
    mem::size_of,
    ptr, slice,
    time::{Duration, Instant},
};

use vt_sys as sys;

use crate::{
    event::*,
    ffi::*,
    types::{ColorScheme, Colors, CursorStyle, MouseTracking, OptionAsAlt, Rgb},
};

/// A terminal mode, either ANSI (`CSI n h`) or DEC private (`CSI ? n h`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Mode(u16);

impl Mode {
    pub const fn dec(n: u16) -> Mode {
        Mode(n & 0x7fff)
    }

    pub const fn ansi(n: u16) -> Mode {
        Mode((n & 0x7fff) | 0x8000)
    }

    pub const CURSOR_KEYS: Mode = Mode::dec(1);
    pub const ORIGIN: Mode = Mode::dec(6);
    pub const AUTO_WRAP: Mode = Mode::dec(7);
    pub const X10_MOUSE: Mode = Mode::dec(9);
    pub const CURSOR_VISIBLE: Mode = Mode::dec(25);
    pub const NORMAL_MOUSE: Mode = Mode::dec(1000);
    pub const BUTTON_MOUSE: Mode = Mode::dec(1002);
    pub const ANY_MOUSE: Mode = Mode::dec(1003);
    pub const FOCUS_EVENT: Mode = Mode::dec(1004);
    pub const ALT_SCROLL: Mode = Mode::dec(1007);
    pub const ALT_SCREEN: Mode = Mode::dec(1049);
    pub const BRACKETED_PASTE: Mode = Mode::dec(2004);
    pub const SYNC_OUTPUT: Mode = Mode::dec(2026);
    pub const GRAPHEME_CLUSTER: Mode = Mode::dec(2027);
    pub const COLOR_SCHEME_REPORT: Mode = Mode::dec(2031);
    pub const IN_BAND_RESIZE: Mode = Mode::dec(2048);
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Size {
    pub cols: u16,
    pub rows: u16,
    pub cell_width: u32,
    pub cell_height: u32,
}

/// Space around the cell grid in surface pixels, used to translate pointer
/// positions into cells for mouse reporting and selection.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Padding {
    pub left: u32,
    pub top: u32,
    pub right: u32,
    pub bottom: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Scrollbar {
    /// Total rows including scrollback.
    pub total: u64,
    /// Row index of the top of the viewport.
    pub offset: u64,
    /// Rows visible in the viewport.
    pub len: u64,
}

const MAX_QUEUED_EVENTS: usize = 256;
const SYNC_OUTPUT_TIMEOUT: Duration = Duration::from_secs(1);

/// State shared with the C callbacks. Lives in a `Box` so its address is
/// stable for the lifetime of the terminal.
pub(crate) struct Inner {
    pub version: String,
    pub events: VecDeque<Event>,
    pub pty_out: Vec<u8>,
    pub title_changed: bool,
    pub pwd_changed: bool,
    pub size: sys::GhosttySizeReportSize,
    pub scheme: ColorScheme,
    pub render_state: sys::GhosttyRenderState,
    pub hold_since: Option<Instant>,
    /// An OSC 52 read is waiting for the embedder: the library's own empty answer is dropped.
    pub clipboard_read_pending: bool,
}

impl Inner {
    fn push(&mut self, e: Event) {
        if self.events.len() >= MAX_QUEUED_EVENTS {
            self.events.pop_front();
        }
        self.events.push_back(e);
    }
}

pub struct Term {
    pub(crate) raw: sys::GhosttyTerminal,
    pub(crate) state: sys::GhosttyRenderState,
    pub(crate) row_iter: sys::GhosttyRenderStateRowIterator,
    pub(crate) row_cells: sys::GhosttyRenderStateRowCells,
    pub(crate) key_encoder: sys::GhosttyKeyEncoder,
    pub(crate) key_event: sys::GhosttyKeyEvent,
    pub(crate) mouse_encoder: sys::GhosttyMouseEncoder,
    pub(crate) mouse_event: sys::GhosttyMouseEvent,
    pub(crate) gesture: sys::GhosttySelectionGesture,
    pub(crate) search: Option<sys::GhosttySearch>,
    inner: *mut Inner,
    pub(crate) size: Size,
    pub(crate) padding: Padding,
    pub(crate) option_as_alt: OptionAsAlt,
    pub(crate) mouse_buttons_down: u16,
    pub(crate) mouse_geometry: Option<(Size, Padding)>,
    pub(crate) cursor_text: Option<Rgb>,
    pub(crate) selection_foreground: Option<Rgb>,
    pub(crate) selection_background: Option<Rgb>,
    pub(crate) epoch: Instant,
}

// The handles are plain heap pointers with no thread affinity; every access
// goes through `&self`/`&mut self`, so callers synchronize with a lock.
unsafe impl Send for Term {}

impl Term {
    pub fn new(cols: u16, rows: u16) -> Term {
        let (cols, rows) = (cols.max(1), rows.max(1));
        unsafe {
            let mut raw = ptr::null_mut();
            check(
                sys::ghostty_terminal_new(ptr::null(), &mut raw, cols, rows),
                "terminal_new",
            );
            let mut state = ptr::null_mut();
            check(
                sys::ghostty_render_state_new(ptr::null(), &mut state),
                "render_state_new",
            );
            let mut row_iter = ptr::null_mut();
            check(
                sys::ghostty_render_state_row_iterator_new(ptr::null(), &mut row_iter),
                "row_iterator_new",
            );
            let mut row_cells = ptr::null_mut();
            check(
                sys::ghostty_render_state_row_cells_new(ptr::null(), &mut row_cells),
                "row_cells_new",
            );
            let mut key_encoder = ptr::null_mut();
            check(
                sys::ghostty_key_encoder_new(ptr::null(), &mut key_encoder),
                "key_encoder_new",
            );
            let mut key_event = ptr::null_mut();
            check(
                sys::ghostty_key_event_new(ptr::null(), &mut key_event),
                "key_event_new",
            );
            let mut mouse_encoder = ptr::null_mut();
            check(
                sys::ghostty_mouse_encoder_new(ptr::null(), &mut mouse_encoder),
                "mouse_encoder_new",
            );
            let mut mouse_event = ptr::null_mut();
            check(
                sys::ghostty_mouse_event_new(ptr::null(), &mut mouse_event),
                "mouse_event_new",
            );
            let mut gesture = ptr::null_mut();
            check(
                sys::ghostty_selection_gesture_new(ptr::null(), &mut gesture),
                "gesture_new",
            );

            let size = Size {
                cols,
                rows,
                cell_width: 0,
                cell_height: 0,
            };
            let inner = Box::into_raw(Box::new(Inner {
                version: format!("ghostty {}", env!("CARGO_PKG_VERSION")),
                events: VecDeque::new(),
                pty_out: Vec::new(),
                title_changed: false,
                pwd_changed: false,
                size: report_size(&size),
                scheme: ColorScheme::Dark,
                render_state: state,
                hold_since: None,
                clipboard_read_pending: false,
            }));

            let set = |opt: sys::GhosttyTerminalOption::Type, v: *const c_void| {
                sys::ghostty_terminal_set(raw, opt as _, v);
            };
            use sys::GhosttyTerminalOption as O;
            set(O::GHOSTTY_TERMINAL_OPT_USERDATA, inner as *const c_void);
            set(O::GHOSTTY_TERMINAL_OPT_XTVERSION, cb_xtversion as *const c_void);
            set(
                O::GHOSTTY_TERMINAL_OPT_WRITE_PTY,
                cb_write_pty as *const c_void,
            );
            set(O::GHOSTTY_TERMINAL_OPT_BELL, cb_bell as *const c_void);
            set(
                O::GHOSTTY_TERMINAL_OPT_TITLE_CHANGED,
                cb_title_changed as *const c_void,
            );
            set(
                O::GHOSTTY_TERMINAL_OPT_PWD_CHANGED,
                cb_pwd_changed as *const c_void,
            );
            set(O::GHOSTTY_TERMINAL_OPT_SIZE, cb_size as *const c_void);
            set(
                O::GHOSTTY_TERMINAL_OPT_COLOR_SCHEME,
                cb_color_scheme as *const c_void,
            );
            set(
                O::GHOSTTY_TERMINAL_OPT_DEVICE_ATTRIBUTES,
                cb_device_attributes as *const c_void,
            );
            set(
                O::GHOSTTY_TERMINAL_OPT_CLIPBOARD_WRITE,
                cb_clipboard_write as *const c_void,
            );
            set(
                O::GHOSTTY_TERMINAL_OPT_CLIPBOARD_READ,
                cb_clipboard_read as *const c_void,
            );
            set(
                O::GHOSTTY_TERMINAL_OPT_DESKTOP_NOTIFICATION,
                cb_notification as *const c_void,
            );
            set(
                O::GHOSTTY_TERMINAL_OPT_PROGRESS_REPORT,
                cb_progress as *const c_void,
            );
            set(
                O::GHOSTTY_TERMINAL_OPT_PROGRAM_STATUS,
                cb_program_status as *const c_void,
            );
            set(
                O::GHOSTTY_TERMINAL_OPT_SEMANTIC_PROMPT,
                cb_semantic_prompt as *const c_void,
            );
            set(
                O::GHOSTTY_TERMINAL_OPT_RESIZE_WINDOW,
                cb_resize_window as *const c_void,
            );
            set(
                O::GHOSTTY_TERMINAL_OPT_RENDER_HOLD,
                cb_render_hold as *const c_void,
            );

            // The shells run here load Ghostty's integration, which redraws the prompt on
            // resize; without this the old wrapped prompt stays behind the redrawn one.
            let redraws_prompt = true;
            set(
                O::GHOSTTY_TERMINAL_OPT_SHELL_REDRAWS_PROMPT,
                &redraws_prompt as *const bool as *const c_void,
            );

            // Ghostty enables grapheme clustering by default; the mouse encoder
            // only needs motion deduplication configured once.
            let grapheme = sys::GhosttyTerminalModeConfig {
                mode: Mode::GRAPHEME_CLUSTER.0,
                value: true,
            };
            set(
                O::GHOSTTY_TERMINAL_OPT_MODE_DEFAULT,
                &grapheme as *const _ as *const c_void,
            );
            let track_last_cell = true;
            sys::ghostty_mouse_encoder_setopt(
                mouse_encoder,
                sys::GhosttyMouseEncoderOption::GHOSTTY_MOUSE_ENCODER_OPT_TRACK_LAST_CELL as _,
                &track_last_cell as *const _ as *const c_void,
            );

            Term {
                raw,
                state,
                row_iter,
                row_cells,
                key_encoder,
                key_event,
                mouse_encoder,
                mouse_event,
                gesture,
                search: None,
                inner,
                size,
                padding: Padding::default(),
                option_as_alt: OptionAsAlt::False,
                mouse_buttons_down: 0,
                mouse_geometry: None,
                cursor_text: None,
                selection_foreground: None,
                selection_background: None,
                epoch: Instant::now(),
            }
        }
    }

    pub(crate) fn inner(&mut self) -> &mut Inner {
        unsafe { &mut *self.inner }
    }

    /// Feeds output from the child process into the terminal.
    pub fn write(&mut self, bytes: &[u8]) {
        unsafe { sys::ghostty_terminal_vt_write(self.raw, bytes.as_ptr(), bytes.len()) }
        self.inner().clipboard_read_pending = false;
    }

    /// Changes the grid size. `cell_width`/`cell_height` are in pixels and
    /// feed size reports and mouse encoding.
    pub fn resize(&mut self, cols: u16, rows: u16, cell_width: u32, cell_height: u32) {
        let size = Size {
            cols: cols.max(1),
            rows: rows.max(1),
            cell_width,
            cell_height,
        };
        self.size = size;
        self.inner().size = report_size(&size);
        unsafe {
            check(
                sys::ghostty_terminal_resize(
                    self.raw,
                    size.cols,
                    size.rows,
                    cell_width,
                    cell_height,
                ),
                "terminal_resize",
            );
        }
    }

    pub fn size(&self) -> Size {
        self.size
    }

    pub fn set_padding(&mut self, padding: Padding) {
        self.padding = padding;
    }

    /// Full reset (RIS).
    pub fn reset(&mut self) {
        unsafe { sys::ghostty_terminal_reset(self.raw) }
    }

    /// Bytes the terminal wants sent back to the child (query replies, mode
    /// reports, ...). Must be forwarded to the PTY in order.
    pub fn take_pty_output(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.inner().pty_out)
    }

    pub fn take_events(&mut self) -> Vec<Event> {
        let (title, pwd) = {
            let inner = self.inner();
            (
                std::mem::take(&mut inner.title_changed),
                std::mem::take(&mut inner.pwd_changed),
            )
        };
        if title {
            let t = self.title();
            self.inner().push(Event::Title(t));
        }
        if pwd {
            let p = self.pwd().unwrap_or_default();
            self.inner().push(Event::Pwd(p));
        }
        self.inner().events.drain(..).collect()
    }

    // Configuration ---------------------------------------------------

    /// Applies default colors, cursor color and the 256-color palette. Also
    /// picks the light/dark scheme reported to programs from the background.
    pub fn set_colors(&mut self, colors: &Colors) {
        unsafe {
            let fg: sys::GhosttyColorRgb = colors.foreground.into();
            let bg: sys::GhosttyColorRgb = colors.background.into();
            let set = |opt: sys::GhosttyTerminalOption::Type, v: *const c_void| {
                sys::ghostty_terminal_set(self.raw, opt as _, v);
            };
            use sys::GhosttyTerminalOption as O;
            set(
                O::GHOSTTY_TERMINAL_OPT_COLOR_FOREGROUND,
                &fg as *const _ as *const c_void,
            );
            set(
                O::GHOSTTY_TERMINAL_OPT_COLOR_BACKGROUND,
                &bg as *const _ as *const c_void,
            );
            let cursor = colors.cursor.map(sys::GhosttyColorRgb::from);
            set(
                O::GHOSTTY_TERMINAL_OPT_COLOR_CURSOR,
                cursor
                    .as_ref()
                    .map_or(ptr::null(), |c| c as *const _ as *const c_void),
            );
            let palette = colors.palette.map(sys::GhosttyColorRgb::from);
            set(
                O::GHOSTTY_TERMINAL_OPT_COLOR_PALETTE,
                palette.as_ptr() as *const c_void,
            );
            let dark = sys::ghostty_color_luminance(&bg) < 0.5;
            self.cursor_text = colors.cursor_text;
            self.selection_foreground = colors.selection_foreground;
            self.selection_background = colors.selection_background;
            self.set_color_scheme(if dark {
                ColorScheme::Dark
            } else {
                ColorScheme::Light
            });
        }
    }

    /// Sets the scheme reported for `CSI ? 996 n`, and pushes an unsolicited
    /// report when the program enabled mode 2031.
    pub fn set_color_scheme(&mut self, scheme: ColorScheme) {
        if self.inner().scheme == scheme {
            return;
        }
        self.inner().scheme = scheme;
        if self.mode(Mode::COLOR_SCHEME_REPORT) {
            let raw = match scheme {
                ColorScheme::Light => sys::GhosttyColorScheme::GHOSTTY_COLOR_SCHEME_LIGHT,
                ColorScheme::Dark => sys::GhosttyColorScheme::GHOSTTY_COLOR_SCHEME_DARK,
            };
            let mut buf = [0u8; 32];
            let mut n = 0usize;
            let r = unsafe {
                sys::ghostty_color_scheme_report_encode(
                    raw,
                    buf.as_mut_ptr() as *mut _,
                    buf.len(),
                    &mut n,
                )
            };
            if r == OK {
                self.inner().pty_out.extend_from_slice(&buf[..n]);
            }
        }
    }

    /// The name reported for XTGETTCAP `TN` (the `$TERM` the child sees).
    pub fn set_terminal_name(&mut self, name: &str) {
        let s = sys::GhosttyString {
            ptr: name.as_ptr(),
            len: name.len(),
        };
        unsafe {
            sys::ghostty_terminal_set(
                self.raw,
                sys::GhosttyTerminalOption::GHOSTTY_TERMINAL_OPT_TERMINFO_NAME as _,
                &s as *const _ as *const c_void,
            );
        }
    }

    /// Matches TERM_PROGRAM's Ghostty identity for capability detection.
    pub fn set_program_version(&mut self, version: &str) {
        self.inner().version = format!("ghostty {version}");
    }

    /// Caps scrollback memory. Zero disables scrollback.
    pub fn set_scrollback_limit_bytes(&mut self, bytes: usize) {
        unsafe {
            sys::ghostty_terminal_set(
                self.raw,
                sys::GhosttyTerminalOption::GHOSTTY_TERMINAL_OPT_SCROLLBACK_MAX_BYTES as _,
                &bytes as *const _ as *const c_void,
            );
        }
    }

    pub fn set_option_as_alt(&mut self, v: OptionAsAlt) {
        self.option_as_alt = v;
    }

    /// Cursor shape and blink used until the program sets its own (DECSCUSR 0).
    pub fn set_default_cursor(&mut self, style: CursorStyle, blink: bool) {
        let style_raw = match style {
            CursorStyle::Bar => sys::GhosttyTerminalCursorStyle::GHOSTTY_TERMINAL_CURSOR_STYLE_BAR,
            CursorStyle::Block => {
                sys::GhosttyTerminalCursorStyle::GHOSTTY_TERMINAL_CURSOR_STYLE_BLOCK
            }
            CursorStyle::Underline => {
                sys::GhosttyTerminalCursorStyle::GHOSTTY_TERMINAL_CURSOR_STYLE_UNDERLINE
            }
            CursorStyle::BlockHollow => {
                sys::GhosttyTerminalCursorStyle::GHOSTTY_TERMINAL_CURSOR_STYLE_BLOCK_HOLLOW
            }
        };
        unsafe {
            use sys::GhosttyTerminalOption as O;
            sys::ghostty_terminal_set(
                self.raw,
                O::GHOSTTY_TERMINAL_OPT_DEFAULT_CURSOR_STYLE as _,
                &style_raw as *const _ as *const c_void,
            );
            sys::ghostty_terminal_set(
                self.raw,
                O::GHOSTTY_TERMINAL_OPT_DEFAULT_CURSOR_BLINK as _,
                &blink as *const _ as *const c_void,
            );
        }
    }

    // State queries ---------------------------------------------------

    pub fn title(&self) -> String {
        terminal_get::<sys::GhosttyString>(
            self.raw,
            sys::GhosttyTerminalData::GHOSTTY_TERMINAL_DATA_TITLE,
        )
        .map(|s| unsafe { string(s) })
        .unwrap_or_default()
    }

    /// Raw working directory as reported by OSC 7, if any.
    pub fn pwd(&self) -> Option<String> {
        let s = terminal_get::<sys::GhosttyString>(
            self.raw,
            sys::GhosttyTerminalData::GHOSTTY_TERMINAL_DATA_PWD,
        )
        .map(|s| unsafe { string(s) })?;
        (!s.is_empty()).then_some(s)
    }

    /// The OSC 7 working directory decoded to a filesystem path.
    pub fn pwd_path(&self) -> Option<String> {
        let pwd = self.pwd()?;
        // Kitty's shell integration sends a raw path, including literal percent signs.
        if let Some(rest) = pwd.strip_prefix("kitty-shell-cwd://") {
            return rest.find('/').map(|i| rest[i..].to_string());
        }
        Some(match pwd.strip_prefix("file://") {
            Some(rest) => {
                let path = rest.find('/').map_or("/", |i| &rest[i..]);
                percent_decode(path)
            }
            None => pwd,
        })
    }

    pub fn mode(&self, mode: Mode) -> bool {
        let mut cfg = sys::GhosttyTerminalModeConfig {
            mode: mode.0,
            value: false,
        };
        let r = unsafe {
            sys::ghostty_terminal_get(
                self.raw,
                sys::GhosttyTerminalData::GHOSTTY_TERMINAL_DATA_MODE as _,
                &mut cfg as *mut _ as *mut c_void,
            )
        };
        r == OK && cfg.value
    }

    /// `grapheme-width-method`: whether grapheme clusters take the width the Unicode standard
    /// gives them (the default) or are measured one codepoint at a time, as `wcswidth`-based
    /// programs do. A program that sets mode 2027 itself still overrides this until it resets.
    pub fn set_grapheme_clustering(&mut self, on: bool) {
        let cfg = sys::GhosttyTerminalModeConfig { mode: Mode::GRAPHEME_CLUSTER.0, value: on };
        unsafe {
            sys::ghostty_terminal_set(
                self.raw,
                sys::GhosttyTerminalOption::GHOSTTY_TERMINAL_OPT_MODE_DEFAULT as _,
                &cfg as *const _ as *const c_void,
            );
        }
        self.set_mode(Mode::GRAPHEME_CLUSTER, on);
    }

    pub fn set_mode(&mut self, mode: Mode, value: bool) {
        let cfg = sys::GhosttyTerminalModeConfig {
            mode: mode.0,
            value,
        };
        unsafe {
            sys::ghostty_terminal_set(
                self.raw,
                sys::GhosttyTerminalOption::GHOSTTY_TERMINAL_OPT_MODE as _,
                &cfg as *const _ as *const c_void,
            );
        }
    }

    pub fn is_alt_screen(&self) -> bool {
        terminal_get::<sys::GhosttyTerminalScreen::Type>(
            self.raw,
            sys::GhosttyTerminalData::GHOSTTY_TERMINAL_DATA_ACTIVE_SCREEN,
        ) == Some(sys::GhosttyTerminalScreen::GHOSTTY_TERMINAL_SCREEN_ALTERNATE)
    }

    pub fn mouse_tracking(&self) -> MouseTracking {
        if self.mode(Mode::ANY_MOUSE) {
            MouseTracking::Any
        } else if self.mode(Mode::BUTTON_MOUSE) {
            MouseTracking::Button
        } else if self.mode(Mode::NORMAL_MOUSE) {
            MouseTracking::Normal
        } else if self.mode(Mode::X10_MOUSE) {
            MouseTracking::X10
        } else {
            MouseTracking::None
        }
    }

    pub fn bracketed_paste(&self) -> bool {
        self.mode(Mode::BRACKETED_PASTE)
    }

    pub fn focus_reporting(&self) -> bool {
        self.mode(Mode::FOCUS_EVENT)
    }

    /// Current kitty keyboard protocol flag stack top (0 when disabled).
    pub fn kitty_keyboard_flags(&self) -> u8 {
        terminal_get::<u8>(
            self.raw,
            sys::GhosttyTerminalData::GHOSTTY_TERMINAL_DATA_KITTY_KEYBOARD_FLAGS,
        )
        .unwrap_or(0)
    }

    // Scrolling -------------------------------------------------------

    fn scroll(&mut self, tag: sys::GhosttyTerminalScrollViewportTag::Type, value: isize) {
        let v = sys::GhosttyTerminalScrollViewport {
            tag,
            value: sys::GhosttyTerminalScrollViewportValue { delta: value },
        };
        unsafe { sys::ghostty_terminal_scroll_viewport(self.raw, v) }
    }

    /// Scrolls by `delta` rows; negative moves toward older output.
    pub fn scroll_lines(&mut self, delta: isize) {
        self.scroll(
            sys::GhosttyTerminalScrollViewportTag::GHOSTTY_SCROLL_VIEWPORT_DELTA,
            delta,
        );
    }

    pub fn scroll_pages(&mut self, pages: isize) {
        self.scroll_lines(pages * self.size.rows as isize);
    }

    pub fn scroll_to_top(&mut self) {
        self.scroll(
            sys::GhosttyTerminalScrollViewportTag::GHOSTTY_SCROLL_VIEWPORT_TOP,
            0,
        );
    }

    pub fn scroll_to_bottom(&mut self) {
        self.scroll(
            sys::GhosttyTerminalScrollViewportTag::GHOSTTY_SCROLL_VIEWPORT_BOTTOM,
            0,
        );
    }

    /// Puts `row` (0 = oldest scrollback line) at the top of the viewport.
    pub fn scroll_to_row(&mut self, row: usize) {
        self.scroll(
            sys::GhosttyTerminalScrollViewportTag::GHOSTTY_SCROLL_VIEWPORT_ROW,
            row as isize,
        );
    }

    /// Jumps to the previous (`-1`) or next (`1`) shell prompt (OSC 133).
    pub fn scroll_to_prompt(&mut self, delta: isize) {
        self.scroll(
            sys::GhosttyTerminalScrollViewportTag::GHOSTTY_SCROLL_VIEWPORT_DELTA_PROMPT,
            delta,
        );
    }

    pub fn scrollbar(&self) -> Scrollbar {
        terminal_get::<sys::GhosttyTerminalScrollbar>(
            self.raw,
            sys::GhosttyTerminalData::GHOSTTY_TERMINAL_DATA_SCROLLBAR,
        )
        .map(|s| Scrollbar {
            total: s.total,
            offset: s.offset,
            len: s.len,
        })
        .unwrap_or_default()
    }

    /// True when the viewport follows live output rather than history.
    pub fn viewport_at_bottom(&self) -> bool {
        terminal_get::<bool>(
            self.raw,
            sys::GhosttyTerminalData::GHOSTTY_TERMINAL_DATA_VIEWPORT_ACTIVE,
        )
        .unwrap_or(true)
    }

    // Internal helpers ------------------------------------------------

    pub(crate) fn viewport_ref(&self, col: u16, row: u16) -> Option<sys::GhosttyGridRef> {
        self.grid_ref(sys::GhosttyPointTag::GHOSTTY_POINT_TAG_VIEWPORT, col, row as u32)
    }

    pub(crate) fn grid_ref(&self, tag: sys::GhosttyPointTag::Type, col: u16, row: u32) -> Option<sys::GhosttyGridRef> {
        let point = sys::GhosttyPoint {
            tag,
            value: sys::GhosttyPointValue {
                coordinate: sys::GhosttyPointCoordinate {
                    x: col,
                    y: row,
                },
            },
        };
        let mut r = sized::<sys::GhosttyGridRef>(|r, n| r.size = n);
        let res = unsafe { sys::ghostty_terminal_grid_ref(self.raw, point, &mut r) };
        (res == OK).then_some(r)
    }

    pub(crate) fn elapsed_ns(&self) -> u64 {
        self.epoch.elapsed().as_nanos() as u64
    }

    /// Releases a synchronized-output hold that outlived the grace period.
    pub(crate) fn expire_render_hold(&mut self) -> bool {
        let held = self.inner().hold_since;
        match held {
            Some(since) if since.elapsed() >= SYNC_OUTPUT_TIMEOUT => {
                self.set_mode(Mode::SYNC_OUTPUT, false);
                self.inner().hold_since = None;
                false
            }
            Some(_) => true,
            None => false,
        }
    }
}

impl Drop for Term {
    fn drop(&mut self) {
        unsafe {
            if let Some(s) = self.search.take() {
                sys::ghostty_search_free(s);
            }
            sys::ghostty_selection_gesture_free(self.gesture, self.raw);
            sys::ghostty_mouse_event_free(self.mouse_event);
            sys::ghostty_mouse_encoder_free(self.mouse_encoder);
            sys::ghostty_key_event_free(self.key_event);
            sys::ghostty_key_encoder_free(self.key_encoder);
            sys::ghostty_render_state_row_cells_free(self.row_cells);
            sys::ghostty_render_state_row_iterator_free(self.row_iter);
            sys::ghostty_render_state_free(self.state);
            sys::ghostty_terminal_free(self.raw);
            drop(Box::from_raw(self.inner));
        }
    }
}

fn report_size(s: &Size) -> sys::GhosttySizeReportSize {
    sys::GhosttySizeReportSize {
        rows: s.rows,
        columns: s.cols,
        cell_width: s.cell_width,
        cell_height: s.cell_height,
    }
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hex = std::str::from_utf8(&b[i + 1..i + 3]).ok();
            if let Some(v) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

// Callbacks -------------------------------------------------------------

unsafe fn inner<'a>(ud: *mut c_void) -> &'a mut Inner {
    &mut *(ud as *mut Inner)
}

unsafe extern "C" fn cb_write_pty(
    _t: sys::GhosttyTerminal,
    ud: *mut c_void,
    data: *const u8,
    len: usize,
) {
    if len == 0 {
        return;
    }
    let data = slice::from_raw_parts(data, len);
    let inner = inner(ud);
    if inner.clipboard_read_pending && data.starts_with(b"\x1b]52;") {
        inner.clipboard_read_pending = false;
        return;
    }
    inner.pty_out.extend_from_slice(data);
}

unsafe extern "C" fn cb_bell(_t: sys::GhosttyTerminal, ud: *mut c_void) {
    inner(ud).push(Event::Bell);
}

unsafe extern "C" fn cb_title_changed(_t: sys::GhosttyTerminal, ud: *mut c_void) {
    inner(ud).title_changed = true;
}

unsafe extern "C" fn cb_pwd_changed(_t: sys::GhosttyTerminal, ud: *mut c_void) {
    inner(ud).pwd_changed = true;
}

unsafe extern "C" fn cb_size(
    _t: sys::GhosttyTerminal,
    ud: *mut c_void,
    out: *mut sys::GhosttySizeReportSize,
) -> bool {
    *out = inner(ud).size;
    true
}

unsafe extern "C" fn cb_color_scheme(
    _t: sys::GhosttyTerminal,
    ud: *mut c_void,
    out: *mut sys::GhosttyColorScheme::Type,
) -> bool {
    *out = match inner(ud).scheme {
        ColorScheme::Light => sys::GhosttyColorScheme::GHOSTTY_COLOR_SCHEME_LIGHT,
        ColorScheme::Dark => sys::GhosttyColorScheme::GHOSTTY_COLOR_SCHEME_DARK,
    };
    true
}

unsafe extern "C" fn cb_xtversion(
    _t: sys::GhosttyTerminal,
    ud: *mut c_void,
) -> sys::GhosttyString {
    let version = &(*(ud as *const Inner)).version;
    sys::GhosttyString { ptr: version.as_ptr(), len: version.len() }
}

unsafe extern "C" fn cb_device_attributes(
    _t: sys::GhosttyTerminal,
    _ud: *mut c_void,
    out: *mut sys::GhosttyDeviceAttributes,
) -> bool {
    let attrs = &mut *out;
    attrs.primary.conformance_level = sys::GHOSTTY_DA_CONFORMANCE_LEVEL_2 as u16;
    attrs.primary.features[0] = sys::GHOSTTY_DA_FEATURE_ANSI_COLOR as u16;
    attrs.primary.features[1] = sys::GHOSTTY_DA_FEATURE_CLIPBOARD as u16;
    attrs.primary.num_features = 2;
    attrs.secondary.device_type = sys::GHOSTTY_DA_DEVICE_TYPE_VT220 as u16;
    attrs.secondary.firmware_version = 10;
    attrs.secondary.rom_cartridge = 0;
    attrs.tertiary.unit_id = 0;
    true
}

unsafe extern "C" fn cb_clipboard_write(
    _t: sys::GhosttyTerminal,
    ud: *mut c_void,
    write: *const sys::GhosttyClipboardWrite,
) {
    let w = &*write;
    let location = clipboard_location(w.location);
    let contents: &[sys::GhosttyClipboardContent] = if w.contents_len == 0 {
        &[]
    } else {
        slice::from_raw_parts(w.contents, w.contents_len)
    };
    let text = if contents.is_empty() {
        None
    } else {
        let t = contents
            .iter()
            .find(|c| string(c.mime).starts_with("text/"));
        match t {
            Some(c) => Some(string(c.data)),
            None => return,
        }
    };
    inner(ud).push(Event::ClipboardWrite { location, text });
    if let Some(reply) = w.reply {
        let r = sys::GhosttyClipboardWriteReply {
            size: size_of::<sys::GhosttyClipboardWriteReply>(),
            result: sys::GhosttyClipboardWriteResult::GHOSTTY_CLIPBOARD_WRITE_RESULT_SUCCESS,
            remember: false,
        };
        reply(w, &r);
    }
}

fn clipboard_location(location: sys::GhosttyClipboardLocation::Type) -> ClipboardLocation {
    match location {
        sys::GhosttyClipboardLocation::GHOSTTY_CLIPBOARD_LOCATION_SELECTION => ClipboardLocation::Selection,
        sys::GhosttyClipboardLocation::GHOSTTY_CLIPBOARD_LOCATION_PRIMARY => ClipboardLocation::Primary,
        _ => ClipboardLocation::Standard,
    }
}

/// OSC 52 reads are answered by the embedder later through
/// [`clipboard_read_reply`], once policy and the user have decided; the library's immediate
/// empty answer is suppressed. Requests the OSC 52 reply cannot express (Kitty OSC 5522) are
/// declined.
unsafe extern "C" fn cb_clipboard_read(
    _t: sys::GhosttyTerminal,
    ud: *mut c_void,
    read: *const sys::GhosttyClipboardRead,
) {
    let r = &*read;
    let text_only = r.mimes_len == 1 && !r.list && string(*r.mimes) == "text/plain";
    if !text_only {
        if let Some(reply) = r.reply {
            let answer = sys::GhosttyClipboardReadReply {
                size: size_of::<sys::GhosttyClipboardReadReply>(),
                result: sys::GhosttyClipboardReadResult::GHOSTTY_CLIPBOARD_READ_RESULT_UNSUPPORTED,
                contents: std::ptr::null(),
                contents_len: 0,
                available: std::ptr::null(),
                available_len: 0,
                remember: false,
            };
            reply(read, &answer);
        }
        return;
    }
    let location = clipboard_location(r.location);
    let inner = inner(ud);
    inner.clipboard_read_pending = true;
    inner.push(Event::ClipboardRead { location });
}

/// The bytes that answer an OSC 52 read of `location` with `text`.
pub fn clipboard_read_reply(location: ClipboardLocation, text: &str) -> Vec<u8> {
    use base64::Engine as _;
    let kind = match location {
        ClipboardLocation::Standard => 'c',
        ClipboardLocation::Selection => 's',
        ClipboardLocation::Primary => 'p',
    };
    let data = base64::engine::general_purpose::STANDARD.encode(text);
    format!("\x1b]52;{kind};{data}\x1b\\").into_bytes()
}

unsafe extern "C" fn cb_notification(
    _t: sys::GhosttyTerminal,
    ud: *mut c_void,
    n: *const sys::GhosttyTerminalDesktopNotification,
) {
    let n = &*n;
    inner(ud).push(Event::Notification {
        title: string(n.title),
        body: string(n.body),
    });
}

unsafe extern "C" fn cb_progress(
    _t: sys::GhosttyTerminal,
    ud: *mut c_void,
    r: *const sys::GhosttyTerminalProgressReport,
) {
    use sys::GhosttyTerminalProgressState as S;
    let r = &*r;
    let state = match r.state {
        S::GHOSTTY_TERMINAL_PROGRESS_STATE_SET => ProgressState::Set,
        S::GHOSTTY_TERMINAL_PROGRESS_STATE_ERROR => ProgressState::Error,
        S::GHOSTTY_TERMINAL_PROGRESS_STATE_INDETERMINATE => ProgressState::Indeterminate,
        S::GHOSTTY_TERMINAL_PROGRESS_STATE_PAUSE => ProgressState::Pause,
        _ => ProgressState::Remove,
    };
    let percent = (r.progress >= 0).then_some(r.progress as u8);
    inner(ud).push(Event::Progress(Progress { state, percent }));
}

unsafe extern "C" fn cb_program_status(
    _t: sys::GhosttyTerminal,
    ud: *mut c_void,
    r: *const sys::GhosttyTerminalProgramStatus,
) {
    use sys::GhosttyProgramStatusKind as K;
    use sys::GhosttyProgramStatusState as S;
    let r = &*r;
    let state = match r.state {
        S::GHOSTTY_PROGRAM_STATUS_STATE_WORKING => ProgramState::Working,
        S::GHOSTTY_PROGRAM_STATUS_STATE_DONE => ProgramState::Done,
        S::GHOSTTY_PROGRAM_STATUS_STATE_BLOCKED => ProgramState::Blocked,
        S::GHOSTTY_PROGRAM_STATUS_STATE_ERROR => ProgramState::Error,
        S::GHOSTTY_PROGRAM_STATUS_STATE_CLEAR => ProgramState::Clear,
        _ => ProgramState::Idle,
    };
    let kind = match r.kind {
        K::GHOSTTY_PROGRAM_STATUS_KIND_PERMISSION => ProgramKind::Permission,
        K::GHOSTTY_PROGRAM_STATUS_KIND_QUESTION => ProgramKind::Question,
        K::GHOSTTY_PROGRAM_STATUS_KIND_AUTH => ProgramKind::Auth,
        _ => ProgramKind::None,
    };
    inner(ud).push(Event::ProgramStatus(ProgramStatus {
        state,
        kind,
        progress: (r.progress >= 0).then_some(r.progress as u8),
        id: string(r.id),
        app: string(r.app),
        title: string(r.title),
        message: string(r.message),
    }));
}

unsafe extern "C" fn cb_semantic_prompt(
    _t: sys::GhosttyTerminal,
    ud: *mut c_void,
    e: *const sys::GhosttyTerminalSemanticPrompt,
) {
    use sys::GhosttySemanticPromptKind as K;
    let e = &*e;
    let kind = match e.kind {
        K::GHOSTTY_SEMANTIC_PROMPT_PROMPT_START => PromptKind::PromptStart,
        K::GHOSTTY_SEMANTIC_PROMPT_INPUT_START => PromptKind::InputStart,
        K::GHOSTTY_SEMANTIC_PROMPT_OUTPUT_START => PromptKind::OutputStart,
        K::GHOSTTY_SEMANTIC_PROMPT_COMMAND_END => PromptKind::CommandEnd,
        _ => return,
    };
    inner(ud).push(Event::SemanticPrompt {
        kind,
        exit_code: e.has_exit_code.then_some(e.exit_code),
    });
}

unsafe extern "C" fn cb_resize_window(
    _t: sys::GhosttyTerminal,
    ud: *mut c_void,
    req: *const sys::GhosttyTerminalResizeWindow,
) {
    let req = &*req;
    if req.tag == sys::GhosttyTerminalResizeWindowTag::GHOSTTY_TERMINAL_RESIZE_WINDOW_GRID {
        let g = req.value.grid;
        inner(ud).push(Event::ResizeRequest {
            cols: g.columns,
            rows: g.rows,
        });
    }
}

unsafe extern "C" fn cb_render_hold(t: sys::GhosttyTerminal, ud: *mut c_void, held: bool) {
    let inner = inner(ud);
    if held {
        sys::ghostty_render_state_update(inner.render_state, t);
        inner.hold_since = Some(Instant::now());
    } else {
        inner.hold_since = None;
    }
}
