//! The part of the Ghostty config the terminal view reads, resolved once per
//! config load.

use std::time::Duration;

use angelly_core::config::{parse_bool, parse_duration, BackgroundBlur, Config, CopyOnSelect, WindowPaddingBalance};
use terminal::{CursorStyle, OptionAsAlt, Rgb};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BoldColor {
    AsIs,
    Bright,
    Color(Rgb),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BellFeatures {
    pub system: bool,
    pub audio: bool,
    pub attention: bool,
    pub title: bool,
    pub border: bool,
}

/// When the view jumps back to the bottom of the scrollback.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScrollToBottom {
    pub keystroke: bool,
    pub output: bool,
}

impl ScrollToBottom {
    /// A comma separated list of `keystroke` and `output`, each optionally prefixed with `no-`.
    /// Options left out keep their defaults (`keystroke, no-output`).
    pub fn parse(value: Option<&str>) -> ScrollToBottom {
        let mut out = ScrollToBottom { keystroke: true, output: false };
        for option in value.unwrap_or_default().split(',').map(str::trim) {
            let (name, on) = match option.strip_prefix("no-") {
                Some(name) => (name, false),
                None => (option, true),
            };
            match name {
                "keystroke" => out.keystroke = on,
                "output" => out.output = on,
                _ => {}
            }
        }
        out
    }
}

/// Which hovered links get a preview of their destination.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkPreviews {
    Always,
    Never,
    /// Only OSC 8 hyperlinks, whose text can differ from where they lead.
    Osc8,
}

impl LinkPreviews {
    pub fn shows(self, osc8: bool) -> bool {
        match self {
            LinkPreviews::Always => true,
            LinkPreviews::Never => false,
            LinkPreviews::Osc8 => osc8,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RightClick {
    ContextMenu,
    Paste,
    Copy,
    CopyOrPaste,
    Ignore,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResizeOverlay {
    Always,
    Never,
    AfterFirst,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverlayPosition {
    Center,
    TopLeft,
    TopCenter,
    TopRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotifyOnFinish {
    Never,
    Unfocused,
    Always,
}

/// Padding around the cell grid in points.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Padding {
    pub left: f32,
    pub right: f32,
    pub top: f32,
    pub bottom: f32,
}

#[derive(Clone, Debug)]
pub struct Settings {
    pub font_families: Vec<String>,
    pub font_features: Vec<(String, u32)>,
    pub font_size: f32,
    pub adjust_cell_width: Adjust,
    pub adjust_cell_height: Adjust,
    pub adjust: Adjustments,
    pub cursor_style: CursorStyle,
    pub cursor_blink: Option<bool>,
    pub cursor_opacity: f32,
    pub click_to_move: bool,
    pub bold: BoldColor,
    pub faint_opacity: f32,
    pub minimum_contrast: f32,
    pub background_opacity: f32,
    pub blur: bool,
    pub padding: Padding,
    pub balance_padding: bool,
    pub option_as_alt: OptionAsAlt,
    pub copy_on_select: CopyOnSelect,
    pub right_click: RightClick,
    pub scroll_precision: f32,
    pub scroll_discrete: f32,
    pub hide_while_typing: bool,
    pub link_url: bool,
    pub link_previews: LinkPreviews,
    /// OSC 8 hyperlinks are highlighted and clickable.
    pub link_osc8: bool,
    /// The user's `link` patterns, earlier ones first.
    pub links: Vec<super::links::CustomLink>,
    pub shift_captured_by_apps: bool,
    pub selection_clear_on_typing: bool,
    pub selection_clear_on_copy: bool,
    /// `scroll-to-bottom`: jump to the live screen on a keystroke and on new output.
    pub scroll_to_bottom: ScrollToBottom,
    /// Characters that end a word for double-click and word-wise selection.
    pub word_boundaries: String,
    pub clipboard_trim_trailing_spaces: bool,
    pub scrollbar: bool,
    pub paste_protection: bool,
    pub bell: BellFeatures,
    pub bell_audio_path: Option<String>,
    pub bell_audio_volume: f32,
    pub resize_overlay: ResizeOverlay,
    pub resize_overlay_position: OverlayPosition,
    pub resize_overlay_duration: Duration,
    pub notify_on_finish: NotifyOnFinish,
    pub notify_after: Duration,
    pub unfocused_opacity: f32,
    pub unfocused_fill: Option<Rgb>,
    pub abnormal_exit_runtime: Duration,
}

/// A `adjust-cell-*` value: a point offset or a percentage.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Adjust {
    #[default]
    None,
    Points(f32),
    Percent(f32),
}

impl Adjust {
    pub fn parse(value: Option<&str>) -> Adjust {
        let Some(v) = value.map(str::trim) else { return Adjust::None };
        if let Some(p) = v.strip_suffix('%') {
            return p.parse().map_or(Adjust::None, |p: f32| Adjust::Percent(p / 100.0));
        }
        v.parse().map_or(Adjust::None, Adjust::Points)
    }

    pub fn apply(self, base: f32) -> f32 {
        match self {
            Adjust::None => base,
            Adjust::Points(p) => base + p,
            Adjust::Percent(p) => base * (1.0 + p),
        }
    }
}

/// The `adjust-*` settings that move lines within a cell.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Adjustments {
    /// Distance from the bottom of the cell to the baseline: increase to move text up.
    pub baseline: Adjust,
    pub underline_position: Adjust,
    pub underline_thickness: Adjust,
    pub strikethrough_position: Adjust,
    pub strikethrough_thickness: Adjust,
    pub overline_position: Adjust,
    pub overline_thickness: Adjust,
    pub cursor_thickness: Adjust,
    pub cursor_height: Adjust,
}

impl Adjustments {
    fn from_config(config: &Config) -> Adjustments {
        let get = |key: &str| Adjust::parse(config.get(key));
        Adjustments {
            baseline: get("adjust-font-baseline"),
            underline_position: get("adjust-underline-position"),
            underline_thickness: get("adjust-underline-thickness"),
            strikethrough_position: get("adjust-strikethrough-position"),
            strikethrough_thickness: get("adjust-strikethrough-thickness"),
            overline_position: get("adjust-overline-position"),
            overline_thickness: get("adjust-overline-thickness"),
            cursor_thickness: get("adjust-cursor-thickness"),
            cursor_height: get("adjust-cursor-height"),
        }
    }
}

fn bool_of(config: &Config, key: &str, default: bool) -> bool {
    match config.get(key) {
        Some(v) => parse_bool(v).unwrap_or(default),
        None => default,
    }
}

fn float_of(config: &Config, key: &str, default: f32) -> f32 {
    config.get(key).and_then(|v| v.trim().parse().ok()).unwrap_or(default)
}

/// Parses a comma separated flag list such as `system,no-audio,border`.
pub fn parse_bell_features(value: Option<&str>) -> BellFeatures {
    let mut f = BellFeatures { system: false, audio: false, attention: true, title: true, border: false };
    let Some(value) = value else { return f };
    for part in value.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        if let Some(all) = parse_bool(part) {
            f = BellFeatures { system: all, audio: all, attention: all, title: all, border: all };
            continue;
        }
        let (name, on) = match part.strip_prefix("no-") {
            Some(n) => (n, false),
            None => (part, true),
        };
        match name {
            "system" => f.system = on,
            "audio" => f.audio = on,
            "attention" => f.attention = on,
            "title" => f.title = on,
            "border" => f.border = on,
            _ => {}
        }
    }
    f
}

/// Parses `mouse-scroll-multiplier`: `3`, `precision:0.1` or
/// `precision:1,discrete:3`. Returns `(precision, discrete)`.
pub fn parse_scroll_multiplier(value: Option<&str>) -> (f32, f32) {
    let (mut precision, mut discrete) = (1.0f32, 3.0f32);
    if let Some(value) = value {
        for part in value.split(',').map(str::trim) {
            match part.split_once(':') {
                Some(("precision", v)) => precision = v.parse().unwrap_or(precision),
                Some(("discrete", v)) => discrete = v.parse().unwrap_or(discrete),
                Some(_) => {}
                None => {
                    if let Ok(v) = part.parse() {
                        precision = v;
                        discrete = v;
                    }
                }
            }
        }
    }
    (precision.clamp(0.01, 10_000.0), discrete.clamp(0.01, 10_000.0))
}

fn parse_font_features(config: &Config) -> Vec<(String, u32)> {
    config
        .get_all("font-feature")
        .into_iter()
        .flat_map(|v| v.split(','))
        .filter_map(|f| {
            let f = f.trim().trim_matches('"');
            if f.is_empty() {
                return None;
            }
            if let Some(tag) = f.strip_prefix('-') {
                return Some((tag.to_string(), 0));
            }
            let f = f.strip_prefix('+').unwrap_or(f);
            match f.split_once(['=', ' ']) {
                Some((tag, v)) => Some((tag.trim().to_string(), v.trim().parse().unwrap_or(1))),
                None => Some((f.to_string(), 1)),
            }
        })
        .collect()
}

impl Settings {
    pub fn from_config(config: &Config) -> Settings {
        let colour = |v: &str| Rgb::parse(v);
        let bold = match config.get("bold-color") {
            Some("bright") => BoldColor::Bright,
            Some(v) => colour(v).map_or(BoldColor::AsIs, BoldColor::Color),
            None => BoldColor::AsIs,
        };
        let padding = {
            let (x, y) = (config.window_padding_x(), config.window_padding_y());
            Padding {
                left: x.top_left as f32,
                right: x.bottom_right as f32,
                top: y.top_left as f32,
                bottom: y.bottom_right as f32,
            }
        };
        let (scroll_precision, scroll_discrete) = parse_scroll_multiplier(config.get("mouse-scroll-multiplier"));
        let opacity = config.background_opacity() as f32;
        let blur = !matches!(config.background_blur(), BackgroundBlur::Disabled);
        Settings {
            font_families: config.font_family().into_iter().map(str::to_string).collect(),
            font_features: parse_font_features(config),
            font_size: config.font_size(),
            adjust_cell_width: Adjust::parse(config.get("adjust-cell-width")),
            adjust_cell_height: Adjust::parse(config.get("adjust-cell-height")),
            adjust: Adjustments::from_config(config),
            cursor_style: match config.cursor_style() {
                angelly_core::config::CursorStyle::Block => CursorStyle::Block,
                angelly_core::config::CursorStyle::Bar => CursorStyle::Bar,
                angelly_core::config::CursorStyle::Underline => CursorStyle::Underline,
                angelly_core::config::CursorStyle::BlockHollow => CursorStyle::BlockHollow,
            },
            cursor_blink: config.cursor_style_blink(),
            cursor_opacity: config.cursor_opacity() as f32,
            click_to_move: config.cursor_click_to_move(),
            bold,
            faint_opacity: float_of(config, "faint-opacity", 0.5).clamp(0.0, 1.0),
            minimum_contrast: config.minimum_contrast() as f32,
            background_opacity: opacity,
            blur: blur && opacity < 1.0,
            padding,
            balance_padding: config.window_padding_balance() != WindowPaddingBalance::False,
            option_as_alt: match config.macos_option_as_alt() {
                Some(angelly_core::config::OptionAsAlt::True) => OptionAsAlt::True,
                Some(angelly_core::config::OptionAsAlt::Left) => OptionAsAlt::Left,
                Some(angelly_core::config::OptionAsAlt::Right) => OptionAsAlt::Right,
                _ => OptionAsAlt::False,
            },
            copy_on_select: config.copy_on_select(),
            right_click: match config.get("right-click-action") {
                Some("paste") => RightClick::Paste,
                Some("copy") => RightClick::Copy,
                Some("copy-or-paste") => RightClick::CopyOrPaste,
                Some("ignore") => RightClick::Ignore,
                _ => RightClick::ContextMenu,
            },
            scroll_precision,
            scroll_discrete,
            hide_while_typing: config.mouse_hide_while_typing(),
            link_url: config.link_url(),
            link_previews: match config.get("link-previews") {
                Some("false") => LinkPreviews::Never,
                Some("osc8") => LinkPreviews::Osc8,
                _ => LinkPreviews::Always,
            },
            link_osc8: bool_of(config, "link-osc8", true),
            links: config.get_all("link").into_iter().filter_map(super::links::CustomLink::parse).collect(),
            shift_captured_by_apps: matches!(config.get("mouse-shift-capture"), Some("true" | "always")),
            selection_clear_on_typing: bool_of(config, "selection-clear-on-typing", true),
            selection_clear_on_copy: bool_of(config, "selection-clear-on-copy", false),
            scroll_to_bottom: ScrollToBottom::parse(config.get("scroll-to-bottom")),
            word_boundaries: config
                .get("selection-word-chars")
                .map_or_else(|| terminal::DEFAULT_WORD_BOUNDARIES.to_string(), str::to_string),
            clipboard_trim_trailing_spaces: config.clipboard_trim_trailing_spaces(),
            scrollbar: config.get("scrollbar") != Some("never"),
            paste_protection: config.clipboard_paste_protection(),
            bell: parse_bell_features(config.get("bell-features")),
            bell_audio_path: config.get("bell-audio-path").map(str::to_string),
            bell_audio_volume: float_of(config, "bell-audio-volume", 0.5).clamp(0.0, 1.0),
            resize_overlay: match config.get("resize-overlay") {
                Some("always") => ResizeOverlay::Always,
                Some("never") => ResizeOverlay::Never,
                _ => ResizeOverlay::AfterFirst,
            },
            resize_overlay_position: match config.get("resize-overlay-position") {
                Some("top-left") => OverlayPosition::TopLeft,
                Some("top-center") => OverlayPosition::TopCenter,
                Some("top-right") => OverlayPosition::TopRight,
                Some("bottom-left") => OverlayPosition::BottomLeft,
                Some("bottom-center") => OverlayPosition::BottomCenter,
                Some("bottom-right") => OverlayPosition::BottomRight,
                _ => OverlayPosition::Center,
            },
            resize_overlay_duration: config
                .get("resize-overlay-duration")
                .and_then(parse_duration)
                .unwrap_or(Duration::from_millis(750)),
            notify_on_finish: match config.get("notify-on-command-finish") {
                Some("unfocused") => NotifyOnFinish::Unfocused,
                Some("always") => NotifyOnFinish::Always,
                _ => NotifyOnFinish::Never,
            },
            notify_after: config
                .get("notify-on-command-finish-after")
                .and_then(parse_duration)
                .unwrap_or(Duration::from_secs(5)),
            unfocused_opacity: config.unfocused_split_opacity() as f32,
            unfocused_fill: config.unfocused_split_fill().map(|c| Rgb::new(c.r, c.g, c.b)),
            abnormal_exit_runtime: config
                .get("abnormal-command-exit-runtime")
                .and_then(|v| v.trim().parse().ok())
                .map_or(Duration::from_millis(250), Duration::from_millis),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use angelly_core::config::{ColorScheme, Env};

    fn config(text: &str) -> Config {
        Config::from_str(text, &Env::from_process(), ColorScheme::Dark)
    }

    #[test]
    fn bell_features_follow_ghostty_defaults_and_overrides() {
        let d = parse_bell_features(None);
        assert!(d.attention && d.title && !d.system && !d.audio && !d.border);
        let f = parse_bell_features(Some("system,no-title,border"));
        assert!(f.system && f.border && !f.title && f.attention);
        assert_eq!(parse_bell_features(Some("false")), BellFeatures::default());
    }

    #[test]
    fn scroll_multiplier_forms() {
        assert_eq!(parse_scroll_multiplier(None), (1.0, 3.0));
        assert_eq!(parse_scroll_multiplier(Some("2")), (2.0, 2.0));
        assert_eq!(parse_scroll_multiplier(Some("precision:0.5")), (0.5, 3.0));
        assert_eq!(parse_scroll_multiplier(Some("precision:0.1,discrete:5")), (0.1, 5.0));
        assert_eq!(parse_scroll_multiplier(Some("0")), (0.01, 0.01));
    }

    #[test]
    fn reads_line_adjustments() {
        let s = Settings::from_config(&config("adjust-font-baseline = 2\nadjust-cursor-height = 50%\n"));
        assert_eq!(s.adjust.baseline, Adjust::Points(2.0));
        assert_eq!(s.adjust.cursor_height, Adjust::Percent(0.5));
        assert_eq!(s.adjust.underline_position, Adjust::None);
    }

    #[test]
    fn scroll_to_bottom_options() {
        let parse = |v| ScrollToBottom::parse(v);
        assert_eq!(parse(None), ScrollToBottom { keystroke: true, output: false });
        assert_eq!(parse(Some("no-keystroke, output")), ScrollToBottom { keystroke: false, output: true });
        assert_eq!(parse(Some("output")), ScrollToBottom { keystroke: true, output: true });
        assert_eq!(parse(Some("no-keystroke,bogus")), ScrollToBottom { keystroke: false, output: false });
    }

    #[test]
    fn adjust_values() {
        assert_eq!(Adjust::parse(Some("10%")).apply(20.0), 22.0);
        assert_eq!(Adjust::parse(Some("-2")).apply(20.0), 18.0);
        assert_eq!(Adjust::parse(None).apply(20.0), 20.0);
    }

    #[test]
    fn reads_config_values() {
        let s = Settings::from_config(&config(
            "bold-color = bright\nminimum-contrast = 3\nmacos-option-as-alt = left\n\
             right-click-action = paste\nfont-feature = -calt\nfont-feature = ss01\n\
             cursor-style = bar\nbackground-opacity = 0.8\nbackground-blur = true\n",
        ));
        assert_eq!(s.bold, BoldColor::Bright);
        assert_eq!(s.minimum_contrast, 3.0);
        assert_eq!(s.option_as_alt, OptionAsAlt::Left);
        assert_eq!(s.right_click, RightClick::Paste);
        assert_eq!(s.font_features, vec![("calt".to_string(), 0), ("ss01".to_string(), 1)]);
        assert_eq!(s.cursor_style, CursorStyle::Bar);
        assert!(s.blur);
        assert!((s.background_opacity - 0.8).abs() < 1e-6);
    }
}
