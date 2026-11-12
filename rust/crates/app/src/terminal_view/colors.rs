//! Color adjustments applied while resolving cell colors: contrast
//! enforcement, bright bold and faint text.

use terminal::Rgb;

fn linear(channel: u8) -> f32 {
    let c = f32::from(channel) / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// WCAG relative luminance.
pub fn luminance(c: Rgb) -> f32 {
    0.2126 * linear(c.r) + 0.7152 * linear(c.g) + 0.0722 * linear(c.b)
}

/// WCAG contrast ratio, 1 (identical) to 21 (black on white).
pub fn contrast(a: Rgb, b: Rgb) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

/// Linear interpolation from `a` to `b`.
pub fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let m = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round().clamp(0.0, 255.0) as u8;
    Rgb::new(m(a.r, b.r), m(a.g, b.g), m(a.b, b.b))
}

/// Moves `fg` toward white or black, whichever contrasts more with `bg`, by
/// the smallest amount that reaches the `minimum` ratio.
pub fn ensure_contrast(fg: Rgb, bg: Rgb, minimum: f32) -> Rgb {
    if minimum <= 1.0 || contrast(fg, bg) >= minimum {
        return fg;
    }
    let white = Rgb::new(255, 255, 255);
    let black = Rgb::new(0, 0, 0);
    let target = if contrast(white, bg) >= contrast(black, bg) { white } else { black };
    if contrast(target, bg) < minimum {
        return target;
    }
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..12 {
        let mid = (lo + hi) / 2.0;
        if contrast(mix(fg, target, mid), bg) >= minimum {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    mix(fg, target, hi)
}

/// The bright variant of a bold, normal-intensity palette color.
pub fn bright_variant(fg: Rgb, palette: &[Rgb; 256]) -> Rgb {
    match palette[..8].iter().position(|c| *c == fg) {
        Some(i) => palette[i + 8],
        None => fg,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contrast_extremes() {
        let white = Rgb::new(255, 255, 255);
        let black = Rgb::new(0, 0, 0);
        assert!((contrast(white, black) - 21.0).abs() < 0.01);
        assert!((contrast(white, white) - 1.0).abs() < 0.001);
    }

    #[test]
    fn enforces_minimum_ratio_without_overshooting() {
        let bg = Rgb::new(0x28, 0x2c, 0x34);
        let fg = Rgb::new(0x40, 0x44, 0x4c);
        let fixed = ensure_contrast(fg, bg, 4.5);
        assert!(contrast(fixed, bg) >= 4.5);
        assert!(contrast(fixed, bg) < 5.2, "adjusted only as far as needed");
        assert_eq!(ensure_contrast(Rgb::new(255, 255, 255), bg, 4.5), Rgb::new(255, 255, 255));
        assert_eq!(ensure_contrast(fg, bg, 1.0), fg);
    }

    #[test]
    fn impossible_ratio_falls_back_to_extreme() {
        let mid = Rgb::new(0x80, 0x80, 0x80);
        let fixed = ensure_contrast(Rgb::new(0x7f, 0x7f, 0x7f), mid, 21.0);
        assert!(fixed == Rgb::new(0, 0, 0) || fixed == Rgb::new(255, 255, 255));
    }

    #[test]
    fn bold_picks_bright_palette_entry() {
        let mut palette = [Rgb::default(); 256];
        palette[1] = Rgb::new(200, 0, 0);
        palette[9] = Rgb::new(255, 50, 50);
        assert_eq!(bright_variant(Rgb::new(200, 0, 0), &palette), Rgb::new(255, 50, 50));
        assert_eq!(bright_variant(Rgb::new(1, 2, 3), &palette), Rgb::new(1, 2, 3));
    }

    #[test]
    fn mix_endpoints() {
        let (a, b) = (Rgb::new(10, 20, 30), Rgb::new(110, 120, 130));
        assert_eq!(mix(a, b, 0.0), a);
        assert_eq!(mix(a, b, 1.0), b);
        assert_eq!(mix(a, b, 0.5), Rgb::new(60, 70, 80));
    }
}
