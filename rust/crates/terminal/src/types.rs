use std::ops::{BitAnd, BitOr, BitOrAssign};

use vt_sys as sys;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Rgb { r, g, b }
    }

    pub const fn from_hex(hex: u32) -> Self {
        Rgb::new((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
    }

    /// Parses any color syntax Ghostty accepts in its config: X11 names,
    /// `#rgb`, `#rrggbb`, `rgb:r/g/b`, ...
    pub fn parse(s: &str) -> Option<Rgb> {
        let mut out = sys::GhosttyColorRgb::default();
        let r = unsafe { sys::ghostty_color_parse(s.as_ptr() as *const _, s.len(), &mut out) };
        (r == sys::GhosttyResult::GHOSTTY_SUCCESS).then(|| out.into())
    }
}

impl From<sys::GhosttyColorRgb> for Rgb {
    fn from(c: sys::GhosttyColorRgb) -> Self {
        Rgb {
            r: c.r,
            g: c.g,
            b: c.b,
        }
    }
}

impl From<Rgb> for sys::GhosttyColorRgb {
    fn from(c: Rgb) -> Self {
        sys::GhosttyColorRgb {
            r: c.r,
            g: c.g,
            b: c.b,
        }
    }
}

macro_rules! bit_set {
    ($(#[$m:meta])* $name:ident($t:ty) { $($(#[$cm:meta])* $c:ident = $v:expr;)* }) => {
        $(#[$m])*
        #[derive(Clone, Copy, PartialEq, Eq, Hash, Default, Debug)]
        pub struct $name(pub $t);

        impl $name {
            $($(#[$cm])* pub const $c: $name = $name($v);)*

            pub const fn contains(self, other: $name) -> bool {
                self.0 & other.0 == other.0
            }

            pub const fn is_empty(self) -> bool {
                self.0 == 0
            }
        }

        impl BitOr for $name {
            type Output = $name;
            fn bitor(self, rhs: $name) -> $name {
                $name(self.0 | rhs.0)
            }
        }

        impl BitOrAssign for $name {
            fn bitor_assign(&mut self, rhs: $name) {
                self.0 |= rhs.0;
            }
        }

        impl BitAnd for $name {
            type Output = $name;
            fn bitand(self, rhs: $name) -> $name {
                $name(self.0 & rhs.0)
            }
        }
    };
}

bit_set! {
    /// Keyboard modifiers. The `*_SIDE` bits mark that the right-hand key of
    /// the pair is the one held.
    Mods(u16) {
        SHIFT = 1 << 0;
        CTRL = 1 << 1;
        ALT = 1 << 2;
        SUPER = 1 << 3;
        CAPS_LOCK = 1 << 4;
        NUM_LOCK = 1 << 5;
        SHIFT_SIDE = 1 << 6;
        CTRL_SIDE = 1 << 7;
        ALT_SIDE = 1 << 8;
        SUPER_SIDE = 1 << 9;
    }
}

bit_set! {
    /// Text attributes of a cell. Colors and underline style are separate
    /// fields on [`Cell`](crate::Cell).
    CellFlags(u16) {
        BOLD = 1 << 0;
        ITALIC = 1 << 1;
        FAINT = 1 << 2;
        BLINK = 1 << 3;
        INVERSE = 1 << 4;
        INVISIBLE = 1 << 5;
        STRIKETHROUGH = 1 << 6;
        OVERLINE = 1 << 7;
        HYPERLINK = 1 << 8;
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Default, Debug)]
pub enum Underline {
    #[default]
    None,
    Single,
    Double,
    Curly,
    Dotted,
    Dashed,
}

impl Underline {
    pub(crate) fn from_raw(v: sys::GhosttySgrUnderline::Type) -> Underline {
        use sys::GhosttySgrUnderline as U;
        match v {
            U::GHOSTTY_SGR_UNDERLINE_SINGLE => Underline::Single,
            U::GHOSTTY_SGR_UNDERLINE_DOUBLE => Underline::Double,
            U::GHOSTTY_SGR_UNDERLINE_CURLY => Underline::Curly,
            U::GHOSTTY_SGR_UNDERLINE_DOTTED => Underline::Dotted,
            U::GHOSTTY_SGR_UNDERLINE_DASHED => Underline::Dashed,
            _ => Underline::None,
        }
    }
}

/// How a cell participates in a double-width character.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default, Debug)]
pub enum Wide {
    #[default]
    Narrow,
    /// First cell of a double-width character; carries the text.
    Wide,
    /// Second cell of a double-width character; empty, draw nothing.
    SpacerTail,
    /// Placeholder at the end of a line where a wide character wrapped.
    SpacerHead,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Default, Debug)]
pub enum CursorStyle {
    Bar,
    #[default]
    Block,
    Underline,
    BlockHollow,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum MouseTracking {
    None,
    X10,
    Normal,
    Button,
    Any,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ColorScheme {
    Light,
    Dark,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum OptionAsAlt {
    False,
    True,
    Left,
    Right,
}

impl OptionAsAlt {
    pub(crate) fn raw(self) -> sys::GhosttyOptionAsAlt::Type {
        match self {
            OptionAsAlt::False => sys::GhosttyOptionAsAlt::GHOSTTY_OPTION_AS_ALT_FALSE,
            OptionAsAlt::True => sys::GhosttyOptionAsAlt::GHOSTTY_OPTION_AS_ALT_TRUE,
            OptionAsAlt::Left => sys::GhosttyOptionAsAlt::GHOSTTY_OPTION_AS_ALT_LEFT,
            OptionAsAlt::Right => sys::GhosttyOptionAsAlt::GHOSTTY_OPTION_AS_ALT_RIGHT,
        }
    }
}

/// Palette plus the default colors, as configured by the user's theme.
#[derive(Clone, Debug)]
pub struct Colors {
    pub foreground: Rgb,
    pub background: Rgb,
    pub cursor: Option<Rgb>,
    pub cursor_text: Option<Rgb>,
    pub selection_foreground: Option<Rgb>,
    pub selection_background: Option<Rgb>,
    pub palette: [Rgb; 256],
}

impl Colors {
    /// Ghostty's built-in palette with the given default colors.
    pub fn new(foreground: Rgb, background: Rgb) -> Colors {
        Colors {
            foreground,
            background,
            cursor: None,
            cursor_text: None,
            selection_foreground: None,
            selection_background: None,
            palette: default_palette(),
        }
    }

    /// Replaces palette entries 0-15 and regenerates the 216-color cube and
    /// grayscale ramp from them (Ghostty's `palette-generate`).
    pub fn with_base16(mut self, base: [Rgb; 16], harmonious: bool) -> Colors {
        let mut raw_base = [sys::GhosttyColorRgb::default(); 256];
        for (i, c) in self.palette.iter().enumerate() {
            raw_base[i] = (*c).into();
        }
        for (i, c) in base.iter().enumerate() {
            raw_base[i] = (*c).into();
        }
        let bg: sys::GhosttyColorRgb = self.background.into();
        let fg: sys::GhosttyColorRgb = self.foreground.into();
        let mut out = [sys::GhosttyColorRgb::default(); 256];
        unsafe {
            sys::ghostty_color_palette_generate(
                raw_base.as_ptr(),
                std::ptr::null(),
                &bg,
                &fg,
                harmonious,
                out.as_mut_ptr(),
            )
        };
        for (dst, src) in self.palette.iter_mut().zip(out) {
            *dst = src.into();
        }
        self
    }
}

pub fn default_palette() -> [Rgb; 256] {
    let mut raw = [sys::GhosttyColorRgb::default(); 256];
    unsafe { sys::ghostty_color_palette_default(raw.as_mut_ptr()) };
    raw.map(Rgb::from)
}
