//! Font selection and the cell grid metrics derived from it.

use std::sync::Arc;

use gpui_kit::*;

use super::settings::{Adjust, Adjustments, Settings};

/// The four faces text can be drawn in.
#[derive(Clone)]
pub struct FontSet {
    pub regular: Font,
    pub bold: Font,
    pub italic: Font,
    pub bold_italic: Font,
}

impl FontSet {
    pub fn new(settings: &Settings, config: &angelly_core::config::Config) -> FontSet {
        let primary = settings.font_families.first().cloned().unwrap_or_else(default_family);
        let fallbacks =
            (settings.font_families.len() > 1).then(|| FontFallbacks::from_fonts(settings.font_families[1..].to_vec()));
        let features = FontFeatures(Arc::new(settings.font_features.clone()));
        let face = |key: Option<&str>, weight: FontWeight, style: FontStyle| {
            let family = key.and_then(|k| config.get(k)).map(str::to_string).unwrap_or_else(|| primary.clone());
            Font { family: family.into(), features: features.clone(), fallbacks: fallbacks.clone(), weight, style }
        };
        FontSet {
            regular: face(None, FontWeight::NORMAL, FontStyle::Normal),
            bold: face(Some("font-family-bold"), FontWeight::BOLD, FontStyle::Normal),
            italic: face(Some("font-family-italic"), FontWeight::NORMAL, FontStyle::Italic),
            bold_italic: face(Some("font-family-bold-italic"), FontWeight::BOLD, FontStyle::Italic),
        }
    }

    pub fn face(&self, bold: bool, italic: bool) -> &Font {
        match (bold, italic) {
            (false, false) => &self.regular,
            (true, false) => &self.bold,
            (false, true) => &self.italic,
            (true, true) => &self.bold_italic,
        }
    }
}

/// The monospace family used when `font-family` is not configured.
#[cfg(target_os = "macos")]
fn default_family() -> String {
    "Menlo".to_string()
}

/// The family fontconfig picks for `monospace`.
#[cfg(all(unix, not(target_os = "macos")))]
fn default_family() -> String {
    static FAMILY: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    FAMILY
        .get_or_init(|| {
            let output = std::process::Command::new("fc-match").args(["-f", "%{family}", "monospace"]).output();
            let listed = output.ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).into_owned());
            listed
                .as_deref()
                .and_then(|families| families.split(',').next())
                .map(str::trim)
                .filter(|family| !family.is_empty())
                .unwrap_or("DejaVu Sans Mono")
                .to_string()
        })
        .clone()
}

/// Cascadia Mono ships with current Windows 11 and Windows Terminal; Consolas is on every release.
#[cfg(windows)]
fn default_family() -> String {
    let fonts = std::env::var_os("WINDIR").map(std::path::PathBuf::from).unwrap_or_default().join("Fonts");
    let family = if fonts.join("CascadiaMono.ttf").is_file() { "Cascadia Mono" } else { "Consolas" };
    family.to_string()
}

/// Cell geometry in logical pixels, snapped to whole device pixels so the grid
/// and the glyphs drawn into it stay crisp.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    pub cell_width: Pixels,
    pub cell_height: Pixels,
    /// Distance from the top of a cell to the text baseline.
    pub baseline: Pixels,
    /// How far text sits below where the font puts it, from `adjust-font-baseline`.
    pub text_dy: Pixels,
    /// Stroke thickness for underlines, used by everything that draws a line of its own.
    pub thickness: Pixels,
    pub underline_y: Pixels,
    pub underline_thickness: Pixels,
    pub strike_y: Pixels,
    pub strike_thickness: Pixels,
    pub overline_y: Pixels,
    pub overline_thickness: Pixels,
    /// Width of the bar cursor and thickness of the underline cursor.
    pub cursor_thickness: Pixels,
    pub cursor_height: Pixels,
    pub scale: f32,
}

fn snap(v: f32, scale: f32, round: fn(f32) -> f32) -> f32 {
    round(v * scale) / scale
}

/// Positions and thicknesses within a cell, in logical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Lines {
    baseline: f32,
    underline_y: f32,
    underline_thickness: f32,
    strike_y: f32,
    strike_thickness: f32,
    overline_y: f32,
    overline_thickness: f32,
    cursor_thickness: f32,
    cursor_height: f32,
}

impl Lines {
    /// Applies the `adjust-*` settings. Thicknesses never drop below one device pixel.
    fn adjusted(self, adjust: &Adjustments, cell_height: f32, scale: f32) -> Lines {
        let min = 1.0 / scale;
        let thick = |adj: Adjust, v: f32| snap(adj.apply(v), scale, f32::round).max(min);
        let pos = |adj: Adjust, v: f32| snap(adj.apply(v), scale, f32::round);
        // The baseline setting measures up from the bottom of the cell.
        let baseline = cell_height - pos(adjust.baseline, cell_height - self.baseline);
        Lines {
            baseline,
            underline_y: pos(adjust.underline_position, self.underline_y).min(cell_height - min),
            underline_thickness: thick(adjust.underline_thickness, self.underline_thickness),
            strike_y: pos(adjust.strikethrough_position, self.strike_y),
            strike_thickness: thick(adjust.strikethrough_thickness, self.strike_thickness),
            overline_y: pos(adjust.overline_position, self.overline_y),
            overline_thickness: thick(adjust.overline_thickness, self.overline_thickness),
            cursor_thickness: thick(adjust.cursor_thickness, self.cursor_thickness),
            cursor_height: thick(adjust.cursor_height, self.cursor_height),
        }
    }
}

impl Metrics {
    pub fn measure(window: &Window, fonts: &FontSet, size: f32, settings: &Settings) -> Metrics {
        let scale = window.scale_factor();
        let text_system = window.text_system();
        let size_px = px(size);
        let id = text_system.resolve_font(&fonts.regular);
        let advance = text_system.advance(id, size_px, 'm').map(|s| f32::from(s.width)).unwrap_or(size * 0.6);
        let ascent = f32::from(text_system.ascent(id, size_px));
        let descent = f32::from(text_system.descent(id, size_px).abs());
        let x_height = f32::from(text_system.x_height(id, size_px));

        let cell_width = snap(settings.adjust_cell_width.apply(advance), scale, f32::round).max(1.0 / scale);
        let cell_height = snap(settings.adjust_cell_height.apply(ascent + descent), scale, f32::ceil).max(1.0 / scale);
        let padding = (cell_height - (ascent + descent)) / 2.0;
        let baseline = padding + ascent;
        let thickness = snap((size / 14.0).max(1.0 / scale), scale, f32::round).max(1.0 / scale);
        let lines = Lines {
            baseline,
            underline_y: snap(baseline + descent * 0.45, scale, f32::round).min(cell_height - thickness),
            underline_thickness: thickness,
            strike_y: snap(baseline - x_height / 2.0, scale, f32::round),
            strike_thickness: thickness,
            overline_y: 0.0,
            overline_thickness: thickness,
            cursor_thickness: thickness,
            cursor_height: cell_height,
        }
        .adjusted(&settings.adjust, cell_height, scale);
        Metrics {
            cell_width: px(cell_width),
            cell_height: px(cell_height),
            baseline: px(lines.baseline),
            text_dy: px(lines.baseline - baseline),
            thickness: px(thickness),
            underline_y: px(lines.underline_y),
            underline_thickness: px(lines.underline_thickness),
            strike_y: px(lines.strike_y),
            strike_thickness: px(lines.strike_thickness),
            overline_y: px(lines.overline_y),
            overline_thickness: px(lines.overline_thickness),
            cursor_thickness: px(lines.cursor_thickness),
            cursor_height: px(lines.cursor_height),
            scale,
        }
    }

    /// Cell size in whole device pixels, as the terminal core expects.
    pub fn device_cell(&self) -> (u32, u32) {
        (
            (f32::from(self.cell_width) * self.scale).round() as u32,
            (f32::from(self.cell_height) * self.scale).round() as u32,
        )
    }
}

#[cfg(test)]
mod tests {
    use std::prelude::v1::test;

    use super::*;

    fn defaults() -> Lines {
        Lines {
            baseline: 15.0,
            underline_y: 17.0,
            underline_thickness: 1.0,
            strike_y: 9.0,
            strike_thickness: 1.0,
            overline_y: 0.0,
            overline_thickness: 1.0,
            cursor_thickness: 1.0,
            cursor_height: 20.0,
        }
    }

    #[test]
    fn no_adjustment_changes_nothing() {
        assert_eq!(defaults().adjusted(&Adjustments::default(), 20.0, 2.0), defaults());
    }

    #[test]
    fn baseline_moves_up_when_increased() {
        let adjust = Adjustments { baseline: Adjust::Points(2.0), ..Adjustments::default() };
        assert_eq!(defaults().adjusted(&adjust, 20.0, 1.0).baseline, 13.0);
        let adjust = Adjustments { baseline: Adjust::Points(-2.0), ..Adjustments::default() };
        assert_eq!(defaults().adjusted(&adjust, 20.0, 1.0).baseline, 17.0);
    }

    #[test]
    fn positions_move_down_when_increased_and_thickness_has_a_floor() {
        let adjust = Adjustments {
            underline_position: Adjust::Points(1.0),
            strikethrough_position: Adjust::Points(-1.0),
            overline_position: Adjust::Points(2.0),
            underline_thickness: Adjust::Points(-5.0),
            cursor_thickness: Adjust::Percent(2.0),
            cursor_height: Adjust::Percent(-0.5),
            ..Adjustments::default()
        };
        let l = defaults().adjusted(&adjust, 20.0, 1.0);
        assert_eq!((l.underline_y, l.strike_y, l.overline_y), (18.0, 8.0, 2.0));
        assert_eq!(l.underline_thickness, 1.0);
        assert_eq!(l.cursor_thickness, 3.0);
        assert_eq!(l.cursor_height, 10.0);
    }

    #[test]
    fn the_underline_stays_inside_the_cell() {
        let adjust = Adjustments { underline_position: Adjust::Points(50.0), ..Adjustments::default() };
        assert_eq!(defaults().adjusted(&adjust, 20.0, 1.0).underline_y, 19.0);
    }
}
