use std::ffi::{c_char, c_void};

use vt_sys as sys;

use crate::{
    ffi::*,
    key::{Key, KeyEvent},
    term::{Mode, Term},
    types::{Mods, MouseTracking},
};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum MouseAction {
    Press,
    Release,
    Motion,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    WheelUp,
    WheelDown,
    WheelLeft,
    WheelRight,
    /// Buttons 8 through 11.
    Extra(u8),
}

impl MouseButton {
    fn raw(self) -> sys::GhosttyMouseButton::Type {
        use sys::GhosttyMouseButton as B;
        match self {
            MouseButton::Left => B::GHOSTTY_MOUSE_BUTTON_LEFT,
            MouseButton::Right => B::GHOSTTY_MOUSE_BUTTON_RIGHT,
            MouseButton::Middle => B::GHOSTTY_MOUSE_BUTTON_MIDDLE,
            MouseButton::WheelUp => B::GHOSTTY_MOUSE_BUTTON_FOUR,
            MouseButton::WheelDown => B::GHOSTTY_MOUSE_BUTTON_FIVE,
            MouseButton::WheelLeft => B::GHOSTTY_MOUSE_BUTTON_SIX,
            MouseButton::WheelRight => B::GHOSTTY_MOUSE_BUTTON_SEVEN,
            MouseButton::Extra(8) => B::GHOSTTY_MOUSE_BUTTON_EIGHT,
            MouseButton::Extra(9) => B::GHOSTTY_MOUSE_BUTTON_NINE,
            MouseButton::Extra(10) => B::GHOSTTY_MOUSE_BUTTON_TEN,
            MouseButton::Extra(11) => B::GHOSTTY_MOUSE_BUTTON_ELEVEN,
            MouseButton::Extra(_) => B::GHOSTTY_MOUSE_BUTTON_UNKNOWN,
        }
    }

    fn is_wheel(self) -> bool {
        matches!(
            self,
            MouseButton::WheelUp
                | MouseButton::WheelDown
                | MouseButton::WheelLeft
                | MouseButton::WheelRight
        )
    }

    fn bit(self) -> u16 {
        match self {
            MouseButton::Left => 1 << 1,
            MouseButton::Right => 1 << 2,
            MouseButton::Middle => 1 << 3,
            MouseButton::Extra(n) if (8..=11).contains(&n) => 1 << n,
            _ => 0,
        }
    }
}

/// A pointer event in surface pixels (relative to the view's top-left).
#[derive(Clone, Copy, Debug)]
pub struct MouseEvent {
    pub action: MouseAction,
    /// `None` for motion with no button held.
    pub button: Option<MouseButton>,
    pub mods: Mods,
    pub x: f32,
    pub y: f32,
}

/// What a scroll wheel turn resolved to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Wheel {
    /// The viewport was scrolled; nothing to send.
    Scrolled,
    /// Bytes for the child (mouse reports, or arrow keys on the alternate
    /// screen). May be empty if the active mouse mode ignores wheel events.
    Send(Vec<u8>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PasteError {
    /// The text contains something that could execute commands (a newline in
    /// an unbracketed paste, or the bracketed-paste terminator). Ask the user
    /// and retry with `allow_unsafe`.
    Unsafe,
    Failed(i32),
}

fn encode_into(
    mut f: impl FnMut(*mut c_char, usize, &mut usize) -> sys::GhosttyResult::Type,
) -> Vec<u8> {
    let mut buf = [0u8; 128];
    let mut n = 0usize;
    let r = f(buf.as_mut_ptr() as *mut c_char, buf.len(), &mut n);
    if r == OK {
        return buf[..n].to_vec();
    }
    if r == sys::GhosttyResult::GHOSTTY_OUT_OF_SPACE {
        let mut big = vec![0u8; n];
        if f(big.as_mut_ptr() as *mut c_char, big.len(), &mut n) == OK {
            big.truncate(n);
            return big;
        }
    }
    Vec::new()
}

/// Bytes to send when the window gains or loses focus and the program
/// enabled focus reporting (DEC mode 1004).
pub fn encode_focus(gained: bool) -> Vec<u8> {
    let ev = if gained {
        sys::GhosttyFocusEvent::GHOSTTY_FOCUS_GAINED
    } else {
        sys::GhosttyFocusEvent::GHOSTTY_FOCUS_LOST
    };
    encode_into(|buf, len, n| unsafe { sys::ghostty_focus_encode(ev as _, buf, len, n) })
}

/// Whether pasting `text` as-is could execute commands (line terminators or
/// the bracketed paste end marker).
pub fn paste_is_safe(text: &str) -> bool {
    unsafe { sys::ghostty_paste_is_safe(text.as_ptr() as *const c_char, text.len()) }
}

/// Encodes pasted text for the PTY: control bytes become spaces, and the
/// text is wrapped in bracketed-paste markers or has newlines turned into
/// carriage returns.
pub fn encode_paste(text: &str, bracketed: bool) -> Vec<u8> {
    let mut data = text.as_bytes().to_vec();
    let (ptr, len) = (data.as_mut_ptr() as *mut c_char, data.len());
    encode_into(|buf, cap, n| unsafe {
        sys::ghostty_paste_encode(ptr, len, bracketed, buf, cap, n)
    })
}

impl Term {
    /// Encodes a key event according to the terminal's current keyboard modes
    /// (cursor keys, keypad, kitty keyboard protocol, modifyOtherKeys) and the
    /// configured option-as-alt behavior. Empty when the key sends nothing.
    pub fn encode_key(&mut self, ev: &KeyEvent) -> Vec<u8> {
        let e = self.key_event;
        let enc = self.key_encoder;
        unsafe {
            sys::ghostty_key_event_set_action(e, ev.action.raw() as _);
            sys::ghostty_key_event_set_key(e, ev.key.raw() as _);
            sys::ghostty_key_event_set_mods(e, ev.mods.0);
            sys::ghostty_key_event_set_consumed_mods(e, ev.consumed_mods.0);
            sys::ghostty_key_event_set_composing(e, ev.composing);
            sys::ghostty_key_event_set_utf8(
                e,
                if ev.text.is_empty() {
                    std::ptr::null()
                } else {
                    ev.text.as_ptr() as *const c_char
                },
                ev.text.len(),
            );
            let unshifted = ev.unshifted.or_else(|| ev.key.us_char());
            sys::ghostty_key_event_set_unshifted_codepoint(e, unshifted.map_or(0, |c| c as u32));

            sys::ghostty_key_encoder_setopt_from_terminal(enc, self.raw);
            let alt = self.option_as_alt.raw();
            sys::ghostty_key_encoder_setopt(
                enc,
                sys::GhosttyKeyEncoderOption::GHOSTTY_KEY_ENCODER_OPT_MACOS_OPTION_AS_ALT as _,
                &alt as *const _ as *const c_void,
            );
            let out =
                encode_into(|buf, len, n| sys::ghostty_key_encoder_encode(enc, e, buf, len, n));
            sys::ghostty_key_event_set_utf8(e, std::ptr::null(), 0);
            out
        }
    }

    /// True when the program asked to receive mouse events.
    pub fn mouse_reporting(&self) -> bool {
        self.mouse_tracking() != MouseTracking::None
    }

    fn mouse_format(&self) -> sys::GhosttyMouseFormat::Type {
        use sys::GhosttyMouseFormat as F;
        if self.mode(Mode::dec(1016)) {
            F::GHOSTTY_MOUSE_FORMAT_SGR_PIXELS
        } else if self.mode(Mode::dec(1006)) {
            F::GHOSTTY_MOUSE_FORMAT_SGR
        } else if self.mode(Mode::dec(1015)) {
            F::GHOSTTY_MOUSE_FORMAT_URXVT
        } else if self.mode(Mode::dec(1005)) {
            F::GHOSTTY_MOUSE_FORMAT_UTF8
        } else {
            F::GHOSTTY_MOUSE_FORMAT_X10
        }
    }

    /// Encodes a pointer event for the active tracking mode and report format.
    /// Empty when the mode doesn't report this event.
    pub fn encode_mouse(&mut self, ev: &MouseEvent) -> Vec<u8> {
        let e = self.mouse_event;
        let enc = self.mouse_encoder;
        let (s, p) = (self.size, self.padding);
        let any_pressed = self.mouse_buttons_down != 0;
        let action = match ev.action {
            MouseAction::Press => sys::GhosttyMouseAction::GHOSTTY_MOUSE_ACTION_PRESS,
            MouseAction::Release => sys::GhosttyMouseAction::GHOSTTY_MOUSE_ACTION_RELEASE,
            MouseAction::Motion => sys::GhosttyMouseAction::GHOSTTY_MOUSE_ACTION_MOTION,
        };
        let tracking = match self.mouse_tracking() {
            MouseTracking::None => sys::GhosttyMouseTrackingMode::GHOSTTY_MOUSE_TRACKING_NONE,
            MouseTracking::X10 => sys::GhosttyMouseTrackingMode::GHOSTTY_MOUSE_TRACKING_X10,
            MouseTracking::Normal => sys::GhosttyMouseTrackingMode::GHOSTTY_MOUSE_TRACKING_NORMAL,
            MouseTracking::Button => sys::GhosttyMouseTrackingMode::GHOSTTY_MOUSE_TRACKING_BUTTON,
            MouseTracking::Any => sys::GhosttyMouseTrackingMode::GHOSTTY_MOUSE_TRACKING_ANY,
        };
        let format = self.mouse_format();
        let out = unsafe {
            use sys::GhosttyMouseEncoderOption as O;
            // Unlike setopt_from_terminal and a size change, setting an
            // unchanged event/format keeps the encoder's last-cell state, which
            // motion deduplication depends on.
            sys::ghostty_mouse_encoder_setopt(
                enc,
                O::GHOSTTY_MOUSE_ENCODER_OPT_EVENT as _,
                &tracking as *const _ as *const c_void,
            );
            sys::ghostty_mouse_encoder_setopt(
                enc,
                O::GHOSTTY_MOUSE_ENCODER_OPT_FORMAT as _,
                &format as *const _ as *const c_void,
            );
            if self.mouse_geometry != Some((s, p)) {
                let size = sys::GhosttyMouseEncoderSize {
                    size: std::mem::size_of::<sys::GhosttyMouseEncoderSize>(),
                    screen_width: s.cols as u32 * s.cell_width + p.left + p.right,
                    screen_height: s.rows as u32 * s.cell_height + p.top + p.bottom,
                    cell_width: s.cell_width.max(1),
                    cell_height: s.cell_height.max(1),
                    padding_top: p.top,
                    padding_bottom: p.bottom,
                    padding_right: p.right,
                    padding_left: p.left,
                };
                sys::ghostty_mouse_encoder_setopt(
                    enc,
                    O::GHOSTTY_MOUSE_ENCODER_OPT_SIZE as _,
                    &size as *const _ as *const c_void,
                );
                self.mouse_geometry = Some((s, p));
            }
            sys::ghostty_mouse_encoder_setopt(
                enc,
                O::GHOSTTY_MOUSE_ENCODER_OPT_ANY_BUTTON_PRESSED as _,
                &any_pressed as *const _ as *const c_void,
            );
            sys::ghostty_mouse_event_set_action(e, action as _);
            match ev.button {
                Some(b) => sys::ghostty_mouse_event_set_button(e, b.raw() as _),
                None => sys::ghostty_mouse_event_clear_button(e),
            }
            sys::ghostty_mouse_event_set_mods(e, ev.mods.0);
            sys::ghostty_mouse_event_set_position(
                e,
                sys::GhosttyMousePosition { x: ev.x, y: ev.y },
            );
            encode_into(|buf, len, n| sys::ghostty_mouse_encoder_encode(enc, e, buf, len, n))
        };
        if let Some(b) = ev.button.filter(|b| !b.is_wheel()) {
            match ev.action {
                MouseAction::Press => self.mouse_buttons_down |= b.bit(),
                MouseAction::Release => self.mouse_buttons_down &= !b.bit(),
                MouseAction::Motion => {}
            }
        }
        out
    }

    /// Resolves a scroll wheel turn of `lines` rows (positive scrolls toward
    /// older output). Mouse-reporting programs get wheel buttons, the
    /// alternate screen with alternate-scroll mode gets arrow keys, and
    /// everything else scrolls the viewport.
    pub fn wheel(&mut self, lines: i32, x: f32, y: f32, mods: Mods) -> Wheel {
        if lines == 0 {
            return Wheel::Send(Vec::new());
        }
        let count = lines.unsigned_abs() as usize;
        if self.mouse_reporting() {
            let button = if lines > 0 {
                MouseButton::WheelUp
            } else {
                MouseButton::WheelDown
            };
            let ev = MouseEvent {
                action: MouseAction::Press,
                button: Some(button),
                mods,
                x,
                y,
            };
            let one = self.encode_mouse(&ev);
            return Wheel::Send(one.repeat(count));
        }
        if self.is_alt_screen() && self.mode(Mode::ALT_SCROLL) {
            let key = if lines > 0 {
                Key::ArrowUp
            } else {
                Key::ArrowDown
            };
            let one = self.encode_key(&KeyEvent::press(key, Mods::default()));
            return Wheel::Send(one.repeat(count));
        }
        self.scroll_lines(-(lines as isize));
        Wheel::Scrolled
    }

    /// Bytes for the child when focus changes; empty unless the program
    /// enabled focus reporting.
    pub fn encode_focus(&self, gained: bool) -> Vec<u8> {
        if self.focus_reporting() {
            encode_focus(gained)
        } else {
            Vec::new()
        }
    }

    /// Pastes `text` according to the terminal's modes and returns all pending
    /// output for the PTY (earlier query replies first, the paste last).
    /// Unsafe text is refused unless `allow_unsafe` is set.
    pub fn paste(&mut self, text: &str, allow_unsafe: bool) -> Result<Vec<u8>, PasteError> {
        struct Source<'a>(&'a str);

        unsafe extern "C" fn read(
            userdata: *mut c_void,
            _mime: sys::GhosttyString,
            writer: sys::GhosttyWriter,
        ) -> bool {
            let src = &*(userdata as *const Source);
            match writer.write {
                Some(write) => write(writer.userdata, src.0.as_ptr(), src.0.len()),
                None => false,
            }
        }

        let source = Source(text);
        let mime = sys::GhosttyString {
            ptr: b"text/plain".as_ptr(),
            len: 10,
        };
        let paste = sized::<sys::GhosttyPaste>(|p, n| {
            p.size = n;
            p.location = sys::GhosttyClipboardLocation::GHOSTTY_CLIPBOARD_LOCATION_STANDARD as _;
            p.source = sys::GhosttyPasteSource::GHOSTTY_PASTE_SOURCE_TEXT as _;
            p.mimes = &mime;
            p.mimes_len = 1;
            p.reader = sys::GhosttyMimeReader {
                read: Some(read),
                userdata: &source as *const _ as *mut c_void,
            };
            p.allow_unsafe = allow_unsafe;
        });
        let mut written = false;
        let r = unsafe { sys::ghostty_terminal_paste(self.raw, &paste, &mut written) };
        match r {
            OK => Ok(self.take_pty_output()),
            sys::GhosttyResult::GHOSTTY_REJECTED => Err(PasteError::Unsafe),
            other => Err(PasteError::Failed(other)),
        }
    }
}
