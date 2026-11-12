//! Where the quick terminal sits on a screen and how big it is (spec section 15).
//!
//! Rectangles use Cocoa's convention: the origin is the bottom-left corner and `y` grows
//! upward. The functions are pure so the layout can be tested without a display.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self { x, y, width, height }
    }

    pub fn max_x(&self) -> f64 {
        self.x + self.width
    }

    pub fn max_y(&self) -> f64 {
        self.y + self.height
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Position {
    #[default]
    Top,
    Bottom,
    Left,
    Right,
    Center,
}

impl Position {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "top" => Some(Self::Top),
            "bottom" => Some(Self::Bottom),
            "left" => Some(Self::Left),
            "right" => Some(Self::Right),
            "center" => Some(Self::Center),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Length {
    Percent(f64),
    Pixels(f64),
}

impl Length {
    fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        if let Some(percent) = value.strip_suffix('%') {
            return percent.trim().parse().ok().map(Self::Percent);
        }
        value.strip_suffix("px")?.trim().parse().ok().map(Self::Pixels)
    }

    fn resolve(self, parent: f64) -> f64 {
        match self {
            Self::Percent(percent) => parent * percent / 100.0,
            Self::Pixels(pixels) => pixels,
        }
    }
}

/// `quick-terminal-size`: the length along the position's primary axis, then optionally
/// along the secondary one. Either may be missing.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Size {
    pub primary: Option<Length>,
    pub secondary: Option<Length>,
}

impl Size {
    pub fn parse(value: &str) -> Self {
        let mut parts = value.split(',').map(Length::parse);
        Self { primary: parts.next().flatten(), secondary: parts.next().flatten() }
    }

    /// The window size on a screen whose usable area is `screen`.
    pub fn calculate(&self, position: Position, screen: Rect) -> (f64, f64) {
        let primary = |parent: f64, default: f64| self.primary.map_or(default, |length| length.resolve(parent));
        let secondary = |parent: f64, default: f64| self.secondary.map_or(default, |length| length.resolve(parent));
        match position {
            Position::Left | Position::Right => {
                (primary(screen.width, 400.0), secondary(screen.height, screen.height))
            }
            Position::Top | Position::Bottom => {
                (secondary(screen.width, screen.width), primary(screen.height, 400.0))
            }
            Position::Center if screen.width >= screen.height => {
                (primary(screen.width, 800.0), secondary(screen.height, 400.0))
            }
            Position::Center => (secondary(screen.width, 400.0), primary(screen.height, 800.0)),
        }
    }
}

fn centered_x(screen: Rect, width: f64) -> f64 {
    (screen.x + (screen.width - width) / 2.0).round()
}

fn centered_y(screen: Rect, height: f64) -> f64 {
    (screen.y + (screen.height - height) / 2.0).round()
}

impl Position {
    /// The origin the window rests at while shown.
    pub fn final_origin(self, (width, height): (f64, f64), screen: Rect) -> (f64, f64) {
        match self {
            Self::Top => (centered_x(screen, width), screen.max_y() - height),
            Self::Bottom => (centered_x(screen, width), screen.y),
            Self::Left => (screen.x, centered_y(screen, height)),
            Self::Right => (screen.max_x() - width, centered_y(screen, height)),
            Self::Center => (centered_x(screen, width), centered_y(screen, height)),
        }
    }

    /// The origin the window slides in from and out to: just past the screen edge it is
    /// attached to. A centered window fades in place.
    pub fn hidden_origin(self, (width, height): (f64, f64), screen: Rect) -> (f64, f64) {
        match self {
            Self::Top => (centered_x(screen, width), screen.max_y()),
            Self::Bottom => (centered_x(screen, width), screen.y - height),
            Self::Left => (screen.x - width, centered_y(screen, height)),
            Self::Right => (screen.max_x(), centered_y(screen, height)),
            Self::Center => self.final_origin((width, height), screen),
        }
    }
}

/// Eases an animation in: slow start, fast finish.
pub fn ease_in(t: f64) -> f64 {
    t.clamp(0.0, 1.0).powi(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Rect = Rect::new(0.0, 0.0, 1600.0, 900.0);

    #[test]
    fn size_accepts_percentages_and_pixels() {
        let size = Size::parse("50%,600px");
        assert_eq!(size.primary, Some(Length::Percent(50.0)));
        assert_eq!(size.secondary, Some(Length::Pixels(600.0)));
        assert_eq!(Size::parse("300px").secondary, None);
        assert_eq!(Size::parse("wide"), Size::default());
    }

    #[test]
    fn default_sizes_follow_the_position() {
        let size = Size::default();
        assert_eq!(size.calculate(Position::Top, SCREEN), (1600.0, 400.0));
        assert_eq!(size.calculate(Position::Left, SCREEN), (400.0, 900.0));
        assert_eq!(size.calculate(Position::Center, SCREEN), (800.0, 400.0));
        let portrait = Rect::new(0.0, 0.0, 900.0, 1600.0);
        assert_eq!(size.calculate(Position::Center, portrait), (400.0, 800.0));
    }

    #[test]
    fn configured_sizes_apply_to_their_axis() {
        let size = Size::parse("25%,50%");
        assert_eq!(size.calculate(Position::Top, SCREEN), (800.0, 225.0));
        assert_eq!(size.calculate(Position::Right, SCREEN), (400.0, 450.0));
    }

    #[test]
    fn windows_rest_against_their_edge_and_start_outside_it() {
        let size = (1600.0, 400.0);
        assert_eq!(Position::Top.final_origin(size, SCREEN), (0.0, 500.0));
        assert_eq!(Position::Top.hidden_origin(size, SCREEN), (0.0, 900.0));
        let side = (400.0, 900.0);
        assert_eq!(Position::Right.final_origin(side, SCREEN), (1200.0, 0.0));
        assert_eq!(Position::Right.hidden_origin(side, SCREEN), (1600.0, 0.0));
        assert_eq!(Position::Left.hidden_origin(side, SCREEN), (-400.0, 0.0));
        assert_eq!(Position::Bottom.hidden_origin(size, SCREEN), (0.0, -400.0));
    }

    #[test]
    fn a_centered_window_stays_put() {
        let size = (800.0, 400.0);
        assert_eq!(Position::Center.final_origin(size, SCREEN), (400.0, 250.0));
        assert_eq!(Position::Center.hidden_origin(size, SCREEN), (400.0, 250.0));
    }

    #[test]
    fn easing_runs_from_zero_to_one() {
        assert_eq!(ease_in(0.0), 0.0);
        assert_eq!(ease_in(1.0), 1.0);
        assert!(ease_in(0.5) < 0.5);
    }
}
