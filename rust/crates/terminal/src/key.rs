use vt_sys as sys;

use crate::types::Mods;

macro_rules! keys {
    ($($name:ident => $raw:ident,)*) => {
        /// Physical key, named after the W3C UI Events `code` values.
        #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
        pub enum Key {
            $($name,)*
        }

        /// The character a US layout prints for each macOS virtual key code of a printable key
/// (`kVK_ANSI_*`), which does not depend on the active layout.
const MAC_PRINTABLE_KEYS: [(u16, char); 48] = [
    (0x00, 'a'), (0x01, 's'), (0x02, 'd'), (0x03, 'f'), (0x04, 'h'), (0x05, 'g'), (0x06, 'z'), (0x07, 'x'),
    (0x08, 'c'), (0x09, 'v'), (0x0B, 'b'), (0x0C, 'q'), (0x0D, 'w'), (0x0E, 'e'), (0x0F, 'r'), (0x10, 'y'),
    (0x11, 't'), (0x12, '1'), (0x13, '2'), (0x14, '3'), (0x15, '4'), (0x16, '6'), (0x17, '5'), (0x18, '='),
    (0x19, '9'), (0x1A, '7'), (0x1B, '-'), (0x1C, '8'), (0x1D, '0'), (0x1E, ']'), (0x1F, 'o'), (0x20, 'u'),
    (0x21, '['), (0x22, 'i'), (0x23, 'p'), (0x25, 'l'), (0x26, 'j'), (0x27, '\''), (0x28, 'k'), (0x29, ';'),
    (0x2A, '\\'), (0x2B, ','), (0x2C, '/'), (0x2D, 'n'), (0x2E, 'm'), (0x2F, '.'), (0x32, '`'), (0x31, ' '),
];

impl Key {
            pub(crate) fn raw(self) -> sys::GhosttyKey::Type {
                match self {
                    $(Key::$name => sys::GhosttyKey::$raw,)*
                }
            }

            #[cfg(test)]
            pub(crate) const ALL: &'static [Key] = &[$(Key::$name,)*];
        }
    };
}

keys! {
        Backquote => GHOSTTY_KEY_BACKQUOTE,
        Backslash => GHOSTTY_KEY_BACKSLASH,
        BracketLeft => GHOSTTY_KEY_BRACKET_LEFT,
        BracketRight => GHOSTTY_KEY_BRACKET_RIGHT,
        Comma => GHOSTTY_KEY_COMMA,
        Digit0 => GHOSTTY_KEY_DIGIT_0,
        Digit1 => GHOSTTY_KEY_DIGIT_1,
        Digit2 => GHOSTTY_KEY_DIGIT_2,
        Digit3 => GHOSTTY_KEY_DIGIT_3,
        Digit4 => GHOSTTY_KEY_DIGIT_4,
        Digit5 => GHOSTTY_KEY_DIGIT_5,
        Digit6 => GHOSTTY_KEY_DIGIT_6,
        Digit7 => GHOSTTY_KEY_DIGIT_7,
        Digit8 => GHOSTTY_KEY_DIGIT_8,
        Digit9 => GHOSTTY_KEY_DIGIT_9,
        Equal => GHOSTTY_KEY_EQUAL,
        IntlBackslash => GHOSTTY_KEY_INTL_BACKSLASH,
        IntlRo => GHOSTTY_KEY_INTL_RO,
        IntlYen => GHOSTTY_KEY_INTL_YEN,
        A => GHOSTTY_KEY_A,
        B => GHOSTTY_KEY_B,
        C => GHOSTTY_KEY_C,
        D => GHOSTTY_KEY_D,
        E => GHOSTTY_KEY_E,
        F => GHOSTTY_KEY_F,
        G => GHOSTTY_KEY_G,
        H => GHOSTTY_KEY_H,
        I => GHOSTTY_KEY_I,
        J => GHOSTTY_KEY_J,
        K => GHOSTTY_KEY_K,
        L => GHOSTTY_KEY_L,
        M => GHOSTTY_KEY_M,
        N => GHOSTTY_KEY_N,
        O => GHOSTTY_KEY_O,
        P => GHOSTTY_KEY_P,
        Q => GHOSTTY_KEY_Q,
        R => GHOSTTY_KEY_R,
        S => GHOSTTY_KEY_S,
        T => GHOSTTY_KEY_T,
        U => GHOSTTY_KEY_U,
        V => GHOSTTY_KEY_V,
        W => GHOSTTY_KEY_W,
        X => GHOSTTY_KEY_X,
        Y => GHOSTTY_KEY_Y,
        Z => GHOSTTY_KEY_Z,
        Minus => GHOSTTY_KEY_MINUS,
        Period => GHOSTTY_KEY_PERIOD,
        Quote => GHOSTTY_KEY_QUOTE,
        Semicolon => GHOSTTY_KEY_SEMICOLON,
        Slash => GHOSTTY_KEY_SLASH,
        AltLeft => GHOSTTY_KEY_ALT_LEFT,
        AltRight => GHOSTTY_KEY_ALT_RIGHT,
        Backspace => GHOSTTY_KEY_BACKSPACE,
        CapsLock => GHOSTTY_KEY_CAPS_LOCK,
        ContextMenu => GHOSTTY_KEY_CONTEXT_MENU,
        ControlLeft => GHOSTTY_KEY_CONTROL_LEFT,
        ControlRight => GHOSTTY_KEY_CONTROL_RIGHT,
        Enter => GHOSTTY_KEY_ENTER,
        MetaLeft => GHOSTTY_KEY_META_LEFT,
        MetaRight => GHOSTTY_KEY_META_RIGHT,
        ShiftLeft => GHOSTTY_KEY_SHIFT_LEFT,
        ShiftRight => GHOSTTY_KEY_SHIFT_RIGHT,
        Space => GHOSTTY_KEY_SPACE,
        Tab => GHOSTTY_KEY_TAB,
        Convert => GHOSTTY_KEY_CONVERT,
        KanaMode => GHOSTTY_KEY_KANA_MODE,
        NonConvert => GHOSTTY_KEY_NON_CONVERT,
        Delete => GHOSTTY_KEY_DELETE,
        End => GHOSTTY_KEY_END,
        Help => GHOSTTY_KEY_HELP,
        Home => GHOSTTY_KEY_HOME,
        Insert => GHOSTTY_KEY_INSERT,
        PageDown => GHOSTTY_KEY_PAGE_DOWN,
        PageUp => GHOSTTY_KEY_PAGE_UP,
        ArrowDown => GHOSTTY_KEY_ARROW_DOWN,
        ArrowLeft => GHOSTTY_KEY_ARROW_LEFT,
        ArrowRight => GHOSTTY_KEY_ARROW_RIGHT,
        ArrowUp => GHOSTTY_KEY_ARROW_UP,
        NumLock => GHOSTTY_KEY_NUM_LOCK,
        Numpad0 => GHOSTTY_KEY_NUMPAD_0,
        Numpad1 => GHOSTTY_KEY_NUMPAD_1,
        Numpad2 => GHOSTTY_KEY_NUMPAD_2,
        Numpad3 => GHOSTTY_KEY_NUMPAD_3,
        Numpad4 => GHOSTTY_KEY_NUMPAD_4,
        Numpad5 => GHOSTTY_KEY_NUMPAD_5,
        Numpad6 => GHOSTTY_KEY_NUMPAD_6,
        Numpad7 => GHOSTTY_KEY_NUMPAD_7,
        Numpad8 => GHOSTTY_KEY_NUMPAD_8,
        Numpad9 => GHOSTTY_KEY_NUMPAD_9,
        NumpadAdd => GHOSTTY_KEY_NUMPAD_ADD,
        NumpadBackspace => GHOSTTY_KEY_NUMPAD_BACKSPACE,
        NumpadClear => GHOSTTY_KEY_NUMPAD_CLEAR,
        NumpadClearEntry => GHOSTTY_KEY_NUMPAD_CLEAR_ENTRY,
        NumpadComma => GHOSTTY_KEY_NUMPAD_COMMA,
        NumpadDecimal => GHOSTTY_KEY_NUMPAD_DECIMAL,
        NumpadDivide => GHOSTTY_KEY_NUMPAD_DIVIDE,
        NumpadEnter => GHOSTTY_KEY_NUMPAD_ENTER,
        NumpadEqual => GHOSTTY_KEY_NUMPAD_EQUAL,
        NumpadMemoryAdd => GHOSTTY_KEY_NUMPAD_MEMORY_ADD,
        NumpadMemoryClear => GHOSTTY_KEY_NUMPAD_MEMORY_CLEAR,
        NumpadMemoryRecall => GHOSTTY_KEY_NUMPAD_MEMORY_RECALL,
        NumpadMemoryStore => GHOSTTY_KEY_NUMPAD_MEMORY_STORE,
        NumpadMemorySubtract => GHOSTTY_KEY_NUMPAD_MEMORY_SUBTRACT,
        NumpadMultiply => GHOSTTY_KEY_NUMPAD_MULTIPLY,
        NumpadParenLeft => GHOSTTY_KEY_NUMPAD_PAREN_LEFT,
        NumpadParenRight => GHOSTTY_KEY_NUMPAD_PAREN_RIGHT,
        NumpadSubtract => GHOSTTY_KEY_NUMPAD_SUBTRACT,
        NumpadSeparator => GHOSTTY_KEY_NUMPAD_SEPARATOR,
        NumpadUp => GHOSTTY_KEY_NUMPAD_UP,
        NumpadDown => GHOSTTY_KEY_NUMPAD_DOWN,
        NumpadRight => GHOSTTY_KEY_NUMPAD_RIGHT,
        NumpadLeft => GHOSTTY_KEY_NUMPAD_LEFT,
        NumpadBegin => GHOSTTY_KEY_NUMPAD_BEGIN,
        NumpadHome => GHOSTTY_KEY_NUMPAD_HOME,
        NumpadEnd => GHOSTTY_KEY_NUMPAD_END,
        NumpadInsert => GHOSTTY_KEY_NUMPAD_INSERT,
        NumpadDelete => GHOSTTY_KEY_NUMPAD_DELETE,
        NumpadPageUp => GHOSTTY_KEY_NUMPAD_PAGE_UP,
        NumpadPageDown => GHOSTTY_KEY_NUMPAD_PAGE_DOWN,
        Escape => GHOSTTY_KEY_ESCAPE,
        F1 => GHOSTTY_KEY_F1,
        F2 => GHOSTTY_KEY_F2,
        F3 => GHOSTTY_KEY_F3,
        F4 => GHOSTTY_KEY_F4,
        F5 => GHOSTTY_KEY_F5,
        F6 => GHOSTTY_KEY_F6,
        F7 => GHOSTTY_KEY_F7,
        F8 => GHOSTTY_KEY_F8,
        F9 => GHOSTTY_KEY_F9,
        F10 => GHOSTTY_KEY_F10,
        F11 => GHOSTTY_KEY_F11,
        F12 => GHOSTTY_KEY_F12,
        F13 => GHOSTTY_KEY_F13,
        F14 => GHOSTTY_KEY_F14,
        F15 => GHOSTTY_KEY_F15,
        F16 => GHOSTTY_KEY_F16,
        F17 => GHOSTTY_KEY_F17,
        F18 => GHOSTTY_KEY_F18,
        F19 => GHOSTTY_KEY_F19,
        F20 => GHOSTTY_KEY_F20,
        F21 => GHOSTTY_KEY_F21,
        F22 => GHOSTTY_KEY_F22,
        F23 => GHOSTTY_KEY_F23,
        F24 => GHOSTTY_KEY_F24,
        F25 => GHOSTTY_KEY_F25,
        Fn => GHOSTTY_KEY_FN,
        FnLock => GHOSTTY_KEY_FN_LOCK,
        PrintScreen => GHOSTTY_KEY_PRINT_SCREEN,
        ScrollLock => GHOSTTY_KEY_SCROLL_LOCK,
        Pause => GHOSTTY_KEY_PAUSE,
        BrowserBack => GHOSTTY_KEY_BROWSER_BACK,
        BrowserFavorites => GHOSTTY_KEY_BROWSER_FAVORITES,
        BrowserForward => GHOSTTY_KEY_BROWSER_FORWARD,
        BrowserHome => GHOSTTY_KEY_BROWSER_HOME,
        BrowserRefresh => GHOSTTY_KEY_BROWSER_REFRESH,
        BrowserSearch => GHOSTTY_KEY_BROWSER_SEARCH,
        BrowserStop => GHOSTTY_KEY_BROWSER_STOP,
        Eject => GHOSTTY_KEY_EJECT,
        LaunchApp1 => GHOSTTY_KEY_LAUNCH_APP_1,
        LaunchApp2 => GHOSTTY_KEY_LAUNCH_APP_2,
        LaunchMail => GHOSTTY_KEY_LAUNCH_MAIL,
        MediaPlayPause => GHOSTTY_KEY_MEDIA_PLAY_PAUSE,
        MediaSelect => GHOSTTY_KEY_MEDIA_SELECT,
        MediaStop => GHOSTTY_KEY_MEDIA_STOP,
        MediaTrackNext => GHOSTTY_KEY_MEDIA_TRACK_NEXT,
        MediaTrackPrevious => GHOSTTY_KEY_MEDIA_TRACK_PREVIOUS,
        Power => GHOSTTY_KEY_POWER,
        Sleep => GHOSTTY_KEY_SLEEP,
        AudioVolumeDown => GHOSTTY_KEY_AUDIO_VOLUME_DOWN,
        AudioVolumeMute => GHOSTTY_KEY_AUDIO_VOLUME_MUTE,
        AudioVolumeUp => GHOSTTY_KEY_AUDIO_VOLUME_UP,
        WakeUp => GHOSTTY_KEY_WAKE_UP,
        Copy => GHOSTTY_KEY_COPY,
        Cut => GHOSTTY_KEY_CUT,
        Paste => GHOSTTY_KEY_PASTE,
}

impl Key {
    /// The character this key produces unshifted on a US layout, used when
    /// the caller doesn't supply [`KeyEvent::unshifted`].
    pub(crate) fn us_char(self) -> Option<char> {
        use Key::*;
        Some(match self {
            A => 'a',
            B => 'b',
            C => 'c',
            D => 'd',
            E => 'e',
            F => 'f',
            G => 'g',
            H => 'h',
            I => 'i',
            J => 'j',
            K => 'k',
            L => 'l',
            M => 'm',
            N => 'n',
            O => 'o',
            P => 'p',
            Q => 'q',
            R => 'r',
            S => 's',
            T => 't',
            U => 'u',
            V => 'v',
            W => 'w',
            X => 'x',
            Y => 'y',
            Z => 'z',
            Digit0 => '0',
            Digit1 => '1',
            Digit2 => '2',
            Digit3 => '3',
            Digit4 => '4',
            Digit5 => '5',
            Digit6 => '6',
            Digit7 => '7',
            Digit8 => '8',
            Digit9 => '9',
            Minus => '-',
            Equal => '=',
            BracketLeft => '[',
            BracketRight => ']',
            Backslash => '\\',
            Semicolon => ';',
            Quote => '\'',
            Comma => ',',
            Period => '.',
            Slash => '/',
            Backquote => '`',
            Space => ' ',
            _ => return None,
        })
    }

    /// The character a US layout prints for the macOS virtual key code of a printable key, so a
    /// key can be identified by where it sits whatever the layout makes of it.
    pub fn us_char_for_mac_keycode(code: u16) -> Option<char> {
        MAC_PRINTABLE_KEYS.iter().find(|(c, _)| *c == code).map(|(_, ch)| *ch)
    }

    /// The physical key behind a macOS virtual key code of a printable key.
    pub fn from_mac_keycode(code: u16) -> Option<Key> {
        Key::from_name(&Key::us_char_for_mac_keycode(code)?.to_string())
    }

    /// Maps a key name as reported by UI toolkits ("a", "enter", "pageup",
    /// "f5", "/", ...) to a physical key on a US layout.
    pub fn from_name(name: &str) -> Option<Key> {
        use Key::*;
        let lower = name.to_ascii_lowercase();
        if let [c] = lower.as_bytes() {
            return Some(match c {
                b'a' => A,
                b'b' => B,
                b'c' => C,
                b'd' => D,
                b'e' => E,
                b'f' => F,
                b'g' => G,
                b'h' => H,
                b'i' => I,
                b'j' => J,
                b'k' => K,
                b'l' => L,
                b'm' => M,
                b'n' => N,
                b'o' => O,
                b'p' => P,
                b'q' => Q,
                b'r' => R,
                b's' => S,
                b't' => T,
                b'u' => U,
                b'v' => V,
                b'w' => W,
                b'x' => X,
                b'y' => Y,
                b'z' => Z,
                b'0' => Digit0,
                b'1' => Digit1,
                b'2' => Digit2,
                b'3' => Digit3,
                b'4' => Digit4,
                b'5' => Digit5,
                b'6' => Digit6,
                b'7' => Digit7,
                b'8' => Digit8,
                b'9' => Digit9,
                b'-' => Minus,
                b'=' => Equal,
                b'[' => BracketLeft,
                b']' => BracketRight,
                b'\\' => Backslash,
                b';' => Semicolon,
                b'\'' => Quote,
                b',' => Comma,
                b'.' => Period,
                b'/' => Slash,
                b'`' => Backquote,
                b' ' => Space,
                _ => return None,
            });
        }
        if let Some(n) = lower.strip_prefix('f').and_then(|n| n.parse::<u8>().ok()) {
            return [
                F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12, F13, F14, F15, F16, F17, F18,
                F19, F20, F21, F22, F23, F24, F25,
            ]
            .get((n as usize).checked_sub(1)?)
            .copied();
        }
        Some(match lower.as_str() {
            "enter" | "return" => Enter,
            "escape" | "esc" => Escape,
            "backspace" => Backspace,
            "tab" => Tab,
            "space" => Space,
            "left" | "arrowleft" => ArrowLeft,
            "right" | "arrowright" => ArrowRight,
            "up" | "arrowup" => ArrowUp,
            "down" | "arrowdown" => ArrowDown,
            "home" => Home,
            "end" => End,
            "pageup" | "page_up" => PageUp,
            "pagedown" | "page_down" => PageDown,
            "delete" | "forwarddelete" => Delete,
            "insert" => Insert,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum KeyAction {
    Press,
    Repeat,
    Release,
}

impl KeyAction {
    pub(crate) fn raw(self) -> sys::GhosttyKeyAction::Type {
        match self {
            KeyAction::Press => sys::GhosttyKeyAction::GHOSTTY_KEY_ACTION_PRESS,
            KeyAction::Repeat => sys::GhosttyKeyAction::GHOSTTY_KEY_ACTION_REPEAT,
            KeyAction::Release => sys::GhosttyKeyAction::GHOSTTY_KEY_ACTION_RELEASE,
        }
    }
}

/// A key event as delivered by the platform.
///
/// `text` is what the keystroke produced on the active layout before any
/// Ctrl/Meta transformation; leave it empty for control characters and
/// function keys. `consumed_mods` are the modifiers the layout used to
/// produce `text` (e.g. Shift for an upper-case letter); don't include Alt
/// for macOS Option, so option-as-alt can still apply. `unshifted` is the
/// character on the key without modifiers (needed by the kitty protocol and
/// option-as-alt); it defaults to the US-layout character for the key.
#[derive(Clone, Copy, Debug)]
pub struct KeyEvent<'a> {
    pub key: Key,
    pub action: KeyAction,
    pub mods: Mods,
    pub consumed_mods: Mods,
    pub text: &'a str,
    pub unshifted: Option<char>,
    pub composing: bool,
}

impl<'a> KeyEvent<'a> {
    pub fn press(key: Key, mods: Mods) -> KeyEvent<'static> {
        KeyEvent {
            key,
            action: KeyAction::Press,
            mods,
            consumed_mods: Mods::default(),
            text: "",
            unshifted: None,
            composing: false,
        }
    }

    pub fn with_text(self, text: &str) -> KeyEvent<'_> {
        KeyEvent { text, ..self }
    }

    pub fn with_action(self, action: KeyAction) -> Self {
        KeyEvent { action, ..self }
    }

    pub fn with_consumed(self, consumed_mods: Mods) -> Self {
        KeyEvent {
            consumed_mods,
            ..self
        }
    }

    pub fn with_unshifted(self, unshifted: char) -> Self {
        KeyEvent {
            unshifted: Some(unshifted),
            ..self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_values_are_distinct() {
        let mut seen = std::collections::HashSet::new();
        for k in Key::ALL {
            assert!(seen.insert(k.raw()), "{k:?} duplicates a raw value");
        }
        assert_eq!(Key::ALL.len(), 175);
    }

    #[test]
    fn names() {
        assert_eq!(Key::from_name("A"), Some(Key::A));
        assert_eq!(Key::from_name("f12"), Some(Key::F12));
        assert_eq!(Key::from_name("pageup"), Some(Key::PageUp));
        assert_eq!(Key::from_name("f0"), None);
        assert_eq!(Key::from_name("hyper"), None);
    }

    #[test]
    fn mac_keycodes_name_the_key_by_position() {
        assert_eq!(Key::from_mac_keycode(0x08), Some(Key::C));
        assert_eq!(Key::from_mac_keycode(0x12), Some(Key::Digit1));
        assert_eq!(Key::from_mac_keycode(0x2A), Some(Key::Backslash));
        assert_eq!(Key::from_mac_keycode(0x32), Some(Key::Backquote));
        assert_eq!(Key::from_mac_keycode(0x24), None, "return is not a printable key");
        let mut letters: Vec<char> = (0..=0x32).filter_map(Key::us_char_for_mac_keycode).collect();
        letters.sort_unstable();
        letters.dedup();
        assert_eq!(letters.len(), 48, "every code maps to its own character");
        for code in 0..=0x32 {
            if let Some(c) = Key::us_char_for_mac_keycode(code) {
                assert!(Key::from_name(&c.to_string()).is_some(), "{c:?}");
            }
        }
    }
}
