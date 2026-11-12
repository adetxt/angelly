use std::time::Duration;

macro_rules! string_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident => $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum $name {
            $($variant),+
        }

        impl $name {
            pub const ALL: &'static [$name] = &[$($name::$variant),+];

            pub fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $text),+
                }
            }

            pub fn parse(value: &str) -> Option<Self> {
                match value {
                    $($text => Some(Self::$variant),)+
                    _ => None,
                }
            }

            pub fn is_valid(value: &str) -> bool {
                Self::parse(value).is_some()
            }
        }
    };
}

string_enum!(CursorStyle {
    Block => "block",
    Bar => "bar",
    Underline => "underline",
    BlockHollow => "block_hollow",
});

string_enum!(MacTitlebarStyle {
    Native => "native",
    Transparent => "transparent",
    Tabs => "tabs",
    Hidden => "hidden",
});

string_enum!(MacTitlebarProxyIcon {
    Visible => "visible",
    Hidden => "hidden",
});

string_enum!(OptionAsAlt {
    False => "false",
    True => "true",
    Left => "left",
    Right => "right",
});

string_enum!(WindowTheme {
    Auto => "auto",
    System => "system",
    Light => "light",
    Dark => "dark",
    Ghostty => "ghostty",
});

string_enum!(WindowSaveState {
    Default => "default",
    Never => "never",
    Always => "always",
});

string_enum!(WindowNewTabPosition {
    Current => "current",
    End => "end",
});

string_enum!(CopyOnSelect {
    None => "none",
    Primary => "primary",
    Clipboard => "clipboard",
    Both => "both",
});

string_enum!(ConfirmCloseSurface {
    False => "false",
    True => "true",
    Always => "always",
});

string_enum!(ClipboardAccess {
    Allow => "allow",
    Deny => "deny",
    Ask => "ask",
});

string_enum!(ShellIntegration {
    None => "none",
    Detect => "detect",
    Bash => "bash",
    Elvish => "elvish",
    Fish => "fish",
    Nushell => "nushell",
    Zsh => "zsh",
});

string_enum!(AutoUpdate {
    Off => "off",
    Check => "check",
    Download => "download",
});

string_enum!(WindowPaddingColor {
    Background => "background",
    Extend => "extend",
    ExtendAlways => "extend-always",
});

string_enum!(WindowPaddingBalance {
    False => "false",
    True => "true",
    Equal => "equal",
});

string_enum!(WindowDecoration {
    Auto => "auto",
    Client => "client",
    Server => "server",
    None => "none",
});

impl WindowDecoration {
    /// Booleans are accepted as aliases: true is `auto`, false is `none`.
    pub fn parse_config(value: Option<&str>) -> Option<Self> {
        let Some(value) = value else { return Some(Self::Auto) };
        match parse_bool(value) {
            Some(true) => Some(Self::Auto),
            Some(false) => Some(Self::None),
            None => Self::parse(value),
        }
    }
}

/// Ghostty's boolean spellings.
pub fn parse_bool(value: &str) -> Option<bool> {
    match value {
        "1" | "t" | "T" | "true" => Some(true),
        "0" | "f" | "F" | "false" => Some(false),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackgroundBlur {
    Disabled,
    Enabled,
    MacosGlassRegular,
    MacosGlassClear,
    Radius(u8),
}

impl BackgroundBlur {
    pub fn parse(value: Option<&str>) -> Option<Self> {
        let Some(value) = value else { return Some(Self::Enabled) };
        if let Some(b) = parse_bool(value) {
            return Some(if b { Self::Enabled } else { Self::Disabled });
        }
        match value {
            "macos-glass-regular" => Some(Self::MacosGlassRegular),
            "macos-glass-clear" => Some(Self::MacosGlassClear),
            _ => parse_int_auto::<u8>(value).map(Self::Radius),
        }
    }

    pub fn enabled(self) -> bool {
        match self {
            Self::Disabled => false,
            Self::Radius(r) => r > 0,
            _ => true,
        }
    }

    /// The value handed to the window system: the radius, `20` for plain `true`, and
    /// `-1` / `-2` for the macOS glass styles.
    pub fn cval(self) -> i16 {
        match self {
            Self::Disabled => 0,
            Self::Enabled => 20,
            Self::Radius(r) => i16::from(r),
            Self::MacosGlassRegular => -1,
            Self::MacosGlassClear => -2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowPadding {
    pub top_left: u32,
    pub bottom_right: u32,
}

impl WindowPadding {
    pub fn parse(value: &str) -> Option<Self> {
        let number = |s: &str| s.trim_matches([' ', '\t']).parse::<u32>().ok();
        match value.split_once(',') {
            Some((a, b)) => Some(Self { top_left: number(a)?, bottom_right: number(b)? }),
            None => {
                let v = number(value)?;
                Some(Self { top_left: v, bottom_right: v })
            }
        }
    }
}

/// Parses an integer with Zig's base-0 rules: an optional sign and `0x`, `0o` or
/// `0b` prefixes, with `_` digit separators.
pub fn parse_int_auto<T: TryFrom<i128>>(value: &str) -> Option<T> {
    let (negative, digits) = match value.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, value.strip_prefix('+').unwrap_or(value)),
    };
    let (radix, digits) = match digits.get(..2) {
        Some("0x") => (16, &digits[2..]),
        Some("0o") => (8, &digits[2..]),
        Some("0b") => (2, &digits[2..]),
        _ => (10, digits),
    };
    if digits.is_empty() || digits.starts_with('_') || digits.ends_with('_') || digits.contains("__") {
        return None;
    }
    let n = i128::from_str_radix(&digits.replace('_', ""), radix).ok()?;
    T::try_from(if negative { -n } else { n }).ok()
}

/// A unit-suffixed duration such as `5s`, `250ms` or `1m 30s`.
pub fn parse_duration(value: &str) -> Option<Duration> {
    const UNITS: &[(&str, u128)] = &[
        ("y", 365 * 24 * 3_600 * 1_000_000_000),
        ("w", 7 * 24 * 3_600 * 1_000_000_000),
        ("d", 24 * 3_600 * 1_000_000_000),
        ("h", 3_600 * 1_000_000_000),
        ("m", 60 * 1_000_000_000),
        ("s", 1_000_000_000),
        ("ms", 1_000_000),
        ("µs", 1_000),
        ("us", 1_000),
        ("ns", 1),
    ];
    let mut rest = value;
    let mut total: Option<u128> = None;
    loop {
        rest = rest.trim_start();
        if rest.is_empty() {
            break;
        }
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        let number: u128 = rest[..digits].parse().ok()?;
        rest = &rest[digits..];
        if rest.is_empty() {
            if number == 0 {
                total = Some(0);
                break;
            }
            return None;
        }
        let (name, factor) = UNITS
            .iter()
            .filter(|(name, _)| rest.starts_with(name))
            .max_by_key(|(name, _)| name.len())?;
        rest = &rest[name.len()..];
        total = Some(total.unwrap_or(0).saturating_add(number.saturating_mul(*factor)));
    }
    let nanos = total?.min(u64::MAX as u128) as u64;
    Some(Duration::from_nanos(nanos))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bool_spellings() {
        assert_eq!(parse_bool("t"), Some(true));
        assert_eq!(parse_bool("0"), Some(false));
        assert_eq!(parse_bool("yes"), None);
    }

    #[test]
    fn background_blur_forms() {
        assert_eq!(BackgroundBlur::parse(None), Some(BackgroundBlur::Enabled));
        assert_eq!(BackgroundBlur::parse(Some("false")), Some(BackgroundBlur::Disabled));
        assert_eq!(BackgroundBlur::parse(Some("macos-glass-clear")), Some(BackgroundBlur::MacosGlassClear));
        assert_eq!(BackgroundBlur::parse(Some("0x10")), Some(BackgroundBlur::Radius(16)));
        assert_eq!(BackgroundBlur::parse(Some("256")), None);
        assert!(!BackgroundBlur::Radius(0).enabled());
        assert!(BackgroundBlur::MacosGlassRegular.enabled());
    }

    #[test]
    fn window_padding_forms() {
        assert_eq!(WindowPadding::parse("4"), Some(WindowPadding { top_left: 4, bottom_right: 4 }));
        assert_eq!(WindowPadding::parse(" 2 , 6 "), Some(WindowPadding { top_left: 2, bottom_right: 6 }));
        assert_eq!(WindowPadding::parse("a"), None);
        assert_eq!(WindowPadding::parse("-1"), None);
    }

    #[test]
    fn window_decoration_accepts_booleans() {
        assert_eq!(WindowDecoration::parse_config(Some("true")), Some(WindowDecoration::Auto));
        assert_eq!(WindowDecoration::parse_config(Some("false")), Some(WindowDecoration::None));
        assert_eq!(WindowDecoration::parse_config(Some("server")), Some(WindowDecoration::Server));
        assert_eq!(WindowDecoration::parse_config(None), Some(WindowDecoration::Auto));
        assert_eq!(WindowDecoration::parse_config(Some("x")), None);
    }

    #[test]
    fn durations() {
        assert_eq!(parse_duration("5s"), Some(Duration::from_secs(5)));
        assert_eq!(parse_duration("1m30s"), Some(Duration::from_secs(90)));
        assert_eq!(parse_duration("1m 30s"), Some(Duration::from_secs(90)));
        assert_eq!(parse_duration("250ms"), Some(Duration::from_millis(250)));
        assert_eq!(parse_duration("0"), Some(Duration::ZERO));
        assert_eq!(parse_duration("5"), None);
        assert_eq!(parse_duration("s"), None);
        assert_eq!(parse_duration(""), None);
    }

    #[test]
    fn auto_base_integers() {
        assert_eq!(parse_int_auto::<u8>("0x10"), Some(16));
        assert_eq!(parse_int_auto::<u8>("1_0"), Some(10));
        assert_eq!(parse_int_auto::<u8>("0b11"), Some(3));
        assert_eq!(parse_int_auto::<u8>("300"), None);
        assert_eq!(parse_int_auto::<u8>("_1"), None);
    }

    #[test]
    fn enums_round_trip() {
        for v in CursorStyle::ALL {
            assert_eq!(CursorStyle::parse(v.as_str()), Some(*v));
        }
        assert_eq!(CopyOnSelect::parse("both"), Some(CopyOnSelect::Both));
        assert_eq!(CopyOnSelect::parse("true"), None);
    }
}
