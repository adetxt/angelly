use angelly_core::config::{ColorScheme, Config, Env, TerminalColor};
use gpui_kit::*;
use terminal::{Colors, Rgb};

/// Resolved appearance shared by every view: the Ghostty config, the terminal
/// palette derived from it, and the SwiftUI-style label colors used by chrome.
pub struct Theme {
    pub config: Config,
    pub colors: Colors,
    pub font_family: SharedString,
    pub font_size: f32,
    pub dark: bool,
    pub primary: Hsla,
    pub accent: Hsla,
}

impl Global for Theme {}

fn rgb_of(c: angelly_core::config::Rgb) -> Rgb {
    Rgb::new(c.r, c.g, c.b)
}

pub fn hsla_of(c: Rgb) -> Hsla {
    rgb((c.r as u32) << 16 | (c.g as u32) << 8 | c.b as u32).into()
}

fn luminance(c: Rgb) -> f32 {
    (0.299 * c.r as f32 + 0.587 * c.g as f32 + 0.114 * c.b as f32) / 255.0
}

impl Theme {
    /// Loads `$ANGELLY_CONFIG` when set (used by tests so the real config is
    /// never read), otherwise the default Ghostty config locations.
    pub fn load() -> Theme {
        let env = Env::from_process();
        let config = match std::env::var_os("ANGELLY_CONFIG") {
            Some(path) => Config::load_files(&env, &[path.into()], ColorScheme::Dark),
            None => Config::load(&env, ColorScheme::Dark),
        };
        Theme::from_config(config)
    }

    pub fn from_config(config: Config) -> Theme {
        let fg = rgb_of(config.foreground());
        let bg = rgb_of(config.background());
        let mut colors = Colors::new(fg, bg);
        for (slot, c) in colors.palette.iter_mut().zip(config.palette().iter()) {
            *slot = rgb_of(*c);
        }
        let color = |c: Option<TerminalColor>| match c {
            Some(TerminalColor::Color(c)) => Some(rgb_of(c)),
            _ => None,
        };
        colors.cursor = color(config.cursor_color());
        colors.cursor_text = color(config.cursor_text());
        colors.selection_foreground = color(config.selection_foreground());
        colors.selection_background = color(config.selection_background());
        let dark = luminance(bg) < 0.5;
        let family = config
            .font_family()
            .first()
            .map(|f| f.to_string())
            .unwrap_or_else(|| "Menlo".to_string());
        Theme {
            font_size: config.font_size(),
            font_family: family.into(),
            colors,
            dark,
            primary: if dark { white() } else { black() },
            accent: rgb(0x0A84FF).into(),
            config,
        }
    }

    pub fn background(&self) -> Hsla {
        hsla_of(self.colors.background)
    }

    /// `Color.primary.opacity(a)`.
    pub fn tint(&self, alpha: f32) -> Hsla {
        self.primary.opacity(alpha)
    }

    /// SwiftUI `.secondary` label color.
    pub fn secondary(&self) -> Hsla {
        self.primary.opacity(0.55)
    }

    /// SwiftUI `.tertiary` label color.
    pub fn tertiary(&self) -> Hsla {
        self.primary.opacity(0.3)
    }
}
