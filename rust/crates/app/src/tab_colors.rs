//! Display colors of the tab color tags: Apple's system colors, light and dark.

use angelly_core::persist::TerminalTabColor;
use gpui_kit::*;

/// `None` for the "no color" tag.
pub fn display_color(color: TerminalTabColor, dark: bool) -> Option<Hsla> {
    let (light, dark_value) = match color {
        TerminalTabColor::None => return None,
        TerminalTabColor::Blue => (0x007AFF, 0x0A84FF),
        TerminalTabColor::Purple => (0xAF52DE, 0xBF5AF2),
        TerminalTabColor::Pink => (0xFF2D55, 0xFF375F),
        TerminalTabColor::Red => (0xFF3B30, 0xFF453A),
        TerminalTabColor::Orange => (0xFF9500, 0xFF9F0A),
        TerminalTabColor::Yellow => (0xFFCC00, 0xFFD60A),
        TerminalTabColor::Green => (0x34C759, 0x30D158),
        TerminalTabColor::Teal => (0x00C7BE, 0x63E6E2),
        TerminalTabColor::Graphite => (0x8E8E93, 0x8E8E93),
    };
    Some(rgb(if dark { dark_value } else { light }).into())
}

/// System green, red and purple used by the titlebar git indicator and line counts.
pub struct SystemColors;

impl SystemColors {
    pub fn green(dark: bool) -> Hsla {
        rgb(if dark { 0x30D158 } else { 0x34C759 }).into()
    }

    pub fn red(dark: bool) -> Hsla {
        rgb(if dark { 0xFF453A } else { 0xFF3B30 }).into()
    }

    pub fn purple(dark: bool) -> Hsla {
        rgb(if dark { 0xBF5AF2 } else { 0xAF52DE }).into()
    }

    pub fn orange(dark: bool) -> Hsla {
        rgb(if dark { 0xFF9F0A } else { 0xFF9500 }).into()
    }
}
