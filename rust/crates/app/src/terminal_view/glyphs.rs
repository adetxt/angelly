//! Geometry for glyphs the terminal draws itself instead of taking them from
//! the font: box drawing, block elements, braille and Powerline symbols. Fonts
//! rarely line these up to the cell grid, which leaves visible gaps between
//! neighbouring cells. Every rectangle returned here is a multiple of one
//! device pixel and reaches the cell edges exactly.

/// A shape in cell-local logical pixels.
#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    /// An axis-aligned rectangle with an opacity multiplier.
    Rect { x0: f32, y0: f32, x1: f32, y1: f32, alpha: f32 },
    /// A filled polygon.
    Fill(Vec<(f32, f32)>),
    /// A stroked polyline.
    Stroke { points: Vec<(f32, f32)>, width: f32 },
}

/// Whether `ch` is drawn by [`shapes`].
pub fn is_custom(ch: char) -> bool {
    matches!(ch as u32, 0x2500..=0x259F | 0x2800..=0x28FF | 0xE0B0..=0xE0BF)
}

const NONE: u8 = 0;
const LIGHT: u8 = 1;
const HEAVY: u8 = 2;
const DOUBLE: u8 = 3;

/// `[left, right, up, down]` arm styles for the box drawing characters made
/// of straight arms; dashes, arcs and diagonals are handled separately.
fn arms(cp: u32) -> Option<[u8; 4]> {
    const L: u8 = LIGHT;
    const H: u8 = HEAVY;
    const D: u8 = DOUBLE;
    const N: u8 = NONE;
    Some(match cp {
        0x2500 => [L, L, N, N],
        0x2501 => [H, H, N, N],
        0x2502 => [N, N, L, L],
        0x2503 => [N, N, H, H],
        0x250C => [N, L, N, L],
        0x250D => [N, H, N, L],
        0x250E => [N, L, N, H],
        0x250F => [N, H, N, H],
        0x2510 => [L, N, N, L],
        0x2511 => [H, N, N, L],
        0x2512 => [L, N, N, H],
        0x2513 => [H, N, N, H],
        0x2514 => [N, L, L, N],
        0x2515 => [N, H, L, N],
        0x2516 => [N, L, H, N],
        0x2517 => [N, H, H, N],
        0x2518 => [L, N, L, N],
        0x2519 => [H, N, L, N],
        0x251A => [L, N, H, N],
        0x251B => [H, N, H, N],
        0x251C => [N, L, L, L],
        0x251D => [N, H, L, L],
        0x251E => [N, L, H, L],
        0x251F => [N, L, L, H],
        0x2520 => [N, L, H, H],
        0x2521 => [N, H, H, L],
        0x2522 => [N, H, L, H],
        0x2523 => [N, H, H, H],
        0x2524 => [L, N, L, L],
        0x2525 => [H, N, L, L],
        0x2526 => [L, N, H, L],
        0x2527 => [L, N, L, H],
        0x2528 => [L, N, H, H],
        0x2529 => [H, N, H, L],
        0x252A => [H, N, L, H],
        0x252B => [H, N, H, H],
        0x252C => [L, L, N, L],
        0x252D => [H, L, N, L],
        0x252E => [L, H, N, L],
        0x252F => [H, H, N, L],
        0x2530 => [L, L, N, H],
        0x2531 => [H, L, N, H],
        0x2532 => [L, H, N, H],
        0x2533 => [H, H, N, H],
        0x2534 => [L, L, L, N],
        0x2535 => [H, L, L, N],
        0x2536 => [L, H, L, N],
        0x2537 => [H, H, L, N],
        0x2538 => [L, L, H, N],
        0x2539 => [H, L, H, N],
        0x253A => [L, H, H, N],
        0x253B => [H, H, H, N],
        0x253C => [L, L, L, L],
        0x253D => [H, L, L, L],
        0x253E => [L, H, L, L],
        0x253F => [H, H, L, L],
        0x2540 => [L, L, H, L],
        0x2541 => [L, L, L, H],
        0x2542 => [L, L, H, H],
        0x2543 => [H, L, H, L],
        0x2544 => [L, H, H, L],
        0x2545 => [H, L, L, H],
        0x2546 => [L, H, L, H],
        0x2547 => [H, H, H, L],
        0x2548 => [H, H, L, H],
        0x2549 => [H, L, H, H],
        0x254A => [L, H, H, H],
        0x254B => [H, H, H, H],
        0x2550 => [D, D, N, N],
        0x2551 => [N, N, D, D],
        0x2552 => [N, D, N, L],
        0x2553 => [N, L, N, D],
        0x2554 => [N, D, N, D],
        0x2555 => [D, N, N, L],
        0x2556 => [L, N, N, D],
        0x2557 => [D, N, N, D],
        0x2558 => [N, D, L, N],
        0x2559 => [N, L, D, N],
        0x255A => [N, D, D, N],
        0x255B => [D, N, L, N],
        0x255C => [L, N, D, N],
        0x255D => [D, N, D, N],
        0x255E => [N, D, L, L],
        0x255F => [N, L, D, D],
        0x2560 => [N, D, D, D],
        0x2561 => [D, N, L, L],
        0x2562 => [L, N, D, D],
        0x2563 => [D, N, D, D],
        0x2564 => [D, D, N, L],
        0x2565 => [L, L, N, D],
        0x2566 => [D, D, N, D],
        0x2567 => [D, D, L, N],
        0x2568 => [L, L, D, N],
        0x2569 => [D, D, D, N],
        0x256A => [D, D, L, L],
        0x256B => [L, L, D, D],
        0x256C => [D, D, D, D],
        0x2574 => [L, N, N, N],
        0x2575 => [N, N, L, N],
        0x2576 => [N, L, N, N],
        0x2577 => [N, N, N, L],
        0x2578 => [H, N, N, N],
        0x2579 => [N, N, H, N],
        0x257A => [N, H, N, N],
        0x257B => [N, N, N, H],
        0x257C => [L, H, N, N],
        0x257D => [N, N, L, H],
        0x257E => [H, L, N, N],
        0x257F => [N, N, H, L],
        _ => return None,
    })
}

/// One cell on an integer device-pixel grid.
struct Canvas {
    w: i32,
    h: i32,
    scale: f32,
    light: i32,
    heavy: i32,
    out: Vec<Shape>,
}

impl Canvas {
    fn px(&self, v: i32) -> f32 {
        v as f32 / self.scale
    }

    fn pt(&self, x: f32, y: f32) -> (f32, f32) {
        (x / self.scale, y / self.scale)
    }

    fn rect(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, alpha: f32) {
        let (x0, x1) = (x0.clamp(0, self.w), x1.clamp(0, self.w));
        let (y0, y1) = (y0.clamp(0, self.h), y1.clamp(0, self.h));
        if x1 > x0 && y1 > y0 {
            self.out.push(Shape::Rect { x0: self.px(x0), y0: self.px(y0), x1: self.px(x1), y1: self.px(y1), alpha });
        }
    }

    fn thickness(&self, style: u8) -> i32 {
        if style == HEAVY {
            self.heavy
        } else {
            self.light
        }
    }

    /// Distance from the center line to the outer edge of an arm's bands.
    fn half_width(&self, style: u8) -> i32 {
        match style {
            NONE => 0,
            DOUBLE => self.light * 3 / 2,
            s => self.thickness(s) / 2,
        }
    }

    /// Bands of an arm across its axis as `(lo, hi, side)`.
    fn bands(&self, style: u8, center: i32) -> Vec<(i32, i32, i32)> {
        let t = self.light;
        if style == DOUBLE {
            return vec![(center - t * 3 / 2, center - t / 2, -1), (center + t - t / 2, center + t * 3 / 2, 1)];
        }
        let th = self.thickness(style);
        let lo = center - th / 2;
        vec![(lo, lo + th, 0)]
    }

    /// Where an arm's band starts, measured outward from the center, given
    /// the perpendicular arms on its negative and positive side. Double bands
    /// stop at the inner edge of a perpendicular double line and run through
    /// the corner on the side with no perpendicular arm.
    fn reach(&self, style: u8, side: i32, minus: u8, plus: u8) -> i32 {
        let perp = |s: i32| if s < 0 { minus } else { plus };
        if style == DOUBLE {
            let same = perp(side);
            if same != NONE {
                return if same == DOUBLE { self.light - self.light / 2 } else { -self.half_width(same) };
            }
            return -self.half_width(perp(-side));
        }
        -self.half_width(minus).max(self.half_width(plus))
    }

    fn box_arms(&mut self, arms: [u8; 4]) {
        let [l, r, u, d] = arms;
        let (cx, cy) = (self.w / 2, self.h / 2);
        for (style, minus, plus, horizontal, outward) in
            [(l, u, d, true, -1), (r, u, d, true, 1), (u, l, r, false, -1), (d, l, r, false, 1)]
        {
            if style == NONE {
                continue;
            }
            let (across, along, extent) = if horizontal { (cy, cx, self.w) } else { (cx, cy, self.h) };
            for (lo, hi, side) in self.bands(style, across) {
                let start = along + outward * self.reach(style, side, minus, plus);
                let (a, b) = if outward < 0 { (0, start) } else { (start, extent) };
                if horizontal {
                    self.rect(a, lo, b, hi, 1.0);
                } else {
                    self.rect(lo, a, hi, b, 1.0);
                }
            }
        }
        self.coalesce();
    }

    /// Joins rectangles that touch edge to edge, so a straight line made of
    /// two arms is one rectangle and cannot show a seam.
    fn coalesce(&mut self) {
        let mut i = 0;
        while i < self.out.len() {
            let mut merged = false;
            for j in (i + 1)..self.out.len() {
                if let (
                    Shape::Rect { x0, y0, x1, y1, alpha },
                    Shape::Rect { x0: bx0, y0: by0, x1: bx1, y1: by1, alpha: ba },
                ) = (&self.out[i], &self.out[j])
                {
                    let same_rows = y0 == by0 && y1 == by1 && (x1 == bx0 || bx1 == x0);
                    let same_cols = x0 == bx0 && x1 == bx1 && (y1 == by0 || by1 == y0);
                    if alpha == ba && (same_rows || same_cols) {
                        let joined = Shape::Rect {
                            x0: x0.min(*bx0),
                            y0: y0.min(*by0),
                            x1: x1.max(*bx1),
                            y1: y1.max(*by1),
                            alpha: *alpha,
                        };
                        self.out[i] = joined;
                        self.out.remove(j);
                        merged = true;
                        break;
                    }
                }
            }
            if !merged {
                i += 1;
            }
        }
    }

    fn dashes(&mut self, count: i32, horizontal: bool, style: u8) {
        let t = self.thickness(style);
        let (along, across) = if horizontal { (self.w, self.h) } else { (self.h, self.w) };
        let pitch = along as f32 / count as f32;
        let dash = (pitch * 0.6).round().max(1.0);
        let lo = across / 2 - t / 2;
        for i in 0..count {
            let start = (pitch * i as f32 + (pitch - dash) / 2.0).round() as i32;
            let end = start + dash as i32;
            if horizontal {
                self.rect(start, lo, end, lo + t, 1.0);
            } else {
                self.rect(lo, start, lo + t, end, 1.0);
            }
        }
    }

    /// Center of a light line across an axis of `extent` pixels.
    fn line_center(&self, extent: i32) -> f32 {
        (extent / 2 - self.light / 2) as f32 + self.light as f32 / 2.0
    }

    fn arc(&mut self, right: bool, down: bool) {
        let (cx, cy) = (self.line_center(self.w), self.line_center(self.h));
        let r = (self.w.min(self.h) as f32 / 2.0).floor();
        let (edge_x, edge_y) = (if right { self.w as f32 } else { 0.0 }, if down { self.h as f32 } else { 0.0 });
        let sx = if right { 1.0 } else { -1.0 };
        let sy = if down { 1.0 } else { -1.0 };
        let mut points = vec![self.pt(edge_x, cy)];
        let steps = 10;
        for i in 0..=steps {
            let a = std::f32::consts::FRAC_PI_2 * i as f32 / steps as f32;
            points.push(self.pt(cx + sx * r * (1.0 - a.sin()), cy + sy * r * (1.0 - a.cos())));
        }
        points.push(self.pt(cx, edge_y));
        self.out.push(Shape::Stroke { points, width: self.px(self.light) });
    }

    fn line(&mut self, a: (f32, f32), b: (f32, f32)) {
        self.out.push(Shape::Stroke { points: vec![self.pt(a.0, a.1), self.pt(b.0, b.1)], width: self.px(self.light) });
    }

    fn fill(&mut self, points: &[(f32, f32)]) {
        let pts = points.iter().map(|(x, y)| self.pt(*x, *y)).collect();
        self.out.push(Shape::Fill(pts));
    }

    fn eighths(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, alpha: f32) {
        let f = |v: i32, extent: i32| ((v as f32 / 8.0) * extent as f32).round() as i32;
        self.rect(f(x0, self.w), f(y0, self.h), f(x1, self.w), f(y1, self.h), alpha);
    }

    fn block(&mut self, cp: u32) {
        match cp {
            0x2580 => self.eighths(0, 0, 8, 4, 1.0),
            0x2581..=0x2588 => self.eighths(0, 8 - (cp - 0x2580) as i32, 8, 8, 1.0),
            0x2589..=0x258F => self.eighths(0, 0, 8 - (cp - 0x2588) as i32, 8, 1.0),
            0x2590 => self.eighths(4, 0, 8, 8, 1.0),
            0x2591 => self.eighths(0, 0, 8, 8, 0.25),
            0x2592 => self.eighths(0, 0, 8, 8, 0.5),
            0x2593 => self.eighths(0, 0, 8, 8, 0.75),
            0x2594 => self.eighths(0, 0, 8, 1, 1.0),
            0x2595 => self.eighths(7, 0, 8, 8, 1.0),
            0x2596..=0x259F => {
                // Bits: upper-left, upper-right, lower-left, lower-right.
                const QUADRANTS: [u8; 10] =
                    [0b0100, 0b1000, 0b0001, 0b1101, 0b1001, 0b0111, 0b1011, 0b0010, 0b0110, 0b1110];
                let mask = QUADRANTS[(cp - 0x2596) as usize];
                for (bit, (x0, y0)) in [(0, 0), (4, 0), (0, 4), (4, 4)].into_iter().enumerate() {
                    if mask & (1 << bit) != 0 {
                        self.eighths(x0, y0, x0 + 4, y0 + 4, 1.0);
                    }
                }
            }
            _ => {}
        }
    }

    fn braille(&mut self, cp: u32) {
        let bits = cp - 0x2800;
        let dot = ((self.w as f32 / 4.0).round() as i32).max(1);
        // Dot numbers 1-8 in bit order: the two columns top to bottom, then
        // the bottom row.
        const DOTS: [(u32, u32); 8] = [(0, 0), (0, 1), (0, 2), (1, 0), (1, 1), (1, 2), (0, 3), (1, 3)];
        for (i, (col, row)) in DOTS.into_iter().enumerate() {
            if bits & (1 << i) == 0 {
                continue;
            }
            let cx = (self.w as f32 * (0.25 + 0.5 * col as f32)).round() as i32;
            let cy = (self.h as f32 * (0.125 + 0.25 * row as f32)).round() as i32;
            self.rect(cx - dot / 2, cy - dot / 2, cx - dot / 2 + dot, cy - dot / 2 + dot, 1.0);
        }
    }

    fn half_ellipse(&self, left: bool) -> Vec<(f32, f32)> {
        let (w, h) = (self.w as f32, self.h as f32);
        let steps = 16;
        (0..=steps)
            .map(|i| {
                let a = std::f32::consts::PI * i as f32 / steps as f32;
                let x = if left { w - w * a.sin() } else { w * a.sin() };
                (x, h / 2.0 - h / 2.0 * a.cos())
            })
            .collect()
    }

    fn powerline(&mut self, cp: u32) {
        let (w, h) = (self.w as f32, self.h as f32);
        let t = self.light as f32 / 2.0;
        match cp {
            0xE0B0 => self.fill(&[(0.0, 0.0), (w, h / 2.0), (0.0, h)]),
            0xE0B1 => self.out.push(Shape::Stroke {
                points: vec![self.pt(-t, -t), self.pt(w - t, h / 2.0), self.pt(-t, h + t)],
                width: self.px(self.light),
            }),
            0xE0B2 => self.fill(&[(w, 0.0), (0.0, h / 2.0), (w, h)]),
            0xE0B3 => self.out.push(Shape::Stroke {
                points: vec![self.pt(w + t, -t), self.pt(t, h / 2.0), self.pt(w + t, h + t)],
                width: self.px(self.light),
            }),
            0xE0B4 | 0xE0B6 => {
                let pts = self.half_ellipse(cp == 0xE0B6);
                self.fill(&pts);
            }
            0xE0B5 | 0xE0B7 => {
                let points = self.half_ellipse(cp == 0xE0B7).into_iter().map(|(x, y)| self.pt(x, y)).collect();
                self.out.push(Shape::Stroke { points, width: self.px(self.light) });
            }
            0xE0B8 => self.fill(&[(0.0, 0.0), (0.0, h), (w, h)]),
            0xE0BA => self.fill(&[(w, 0.0), (w, h), (0.0, h)]),
            0xE0BC => self.fill(&[(0.0, 0.0), (w, 0.0), (0.0, h)]),
            0xE0BE => self.fill(&[(0.0, 0.0), (w, 0.0), (w, h)]),
            0xE0B9 | 0xE0BF => self.line((0.0, 0.0), (w, h)),
            0xE0BB | 0xE0BD => self.line((w, 0.0), (0.0, h)),
            _ => {}
        }
    }
}

/// The shapes that draw `ch` in a `w` x `h` cell on a display with the given
/// scale factor, or `None` when the font should draw it.
pub fn shapes(ch: char, w: f32, h: f32, scale: f32) -> Option<Vec<Shape>> {
    if !is_custom(ch) {
        return None;
    }
    let cp = ch as u32;
    let (wd, hd) = ((w * scale).round() as i32, (h * scale).round() as i32);
    let light = (hd as f32 / 14.0).round().max(1.0) as i32;
    let mut c = Canvas { w: wd, h: hd, scale, light, heavy: (light * 2).max(light + 1), out: Vec::new() };
    let (wf, hf) = (wd as f32, hd as f32);
    match cp {
        0x2500..=0x257F => {
            if let Some(a) = arms(cp) {
                c.box_arms(a);
            } else {
                let style = |light_cp: u32| if cp == light_cp { LIGHT } else { HEAVY };
                match cp {
                    0x2504 | 0x2505 => c.dashes(3, true, style(0x2504)),
                    0x2506 | 0x2507 => c.dashes(3, false, style(0x2506)),
                    0x2508 | 0x2509 => c.dashes(4, true, style(0x2508)),
                    0x250A | 0x250B => c.dashes(4, false, style(0x250A)),
                    0x254C | 0x254D => c.dashes(2, true, style(0x254C)),
                    0x254E | 0x254F => c.dashes(2, false, style(0x254E)),
                    0x256D => c.arc(true, true),
                    0x256E => c.arc(false, true),
                    0x256F => c.arc(false, false),
                    0x2570 => c.arc(true, false),
                    0x2571 => c.line((wf, 0.0), (0.0, hf)),
                    0x2572 => c.line((0.0, 0.0), (wf, hf)),
                    0x2573 => {
                        c.line((wf, 0.0), (0.0, hf));
                        c.line((0.0, 0.0), (wf, hf));
                    }
                    _ => return None,
                }
            }
        }
        0x2580..=0x259F => c.block(cp),
        0x2800..=0x28FF => c.braille(cp),
        _ => c.powerline(cp),
    }
    Some(c.out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: f32 = 8.0;
    const H: f32 = 16.0;

    fn rects(ch: char, scale: f32) -> Vec<(f32, f32, f32, f32)> {
        shapes(ch, W, H, scale)
            .unwrap()
            .into_iter()
            .filter_map(|s| match s {
                Shape::Rect { x0, y0, x1, y1, .. } => Some((x0, y0, x1, y1)),
                _ => None,
            })
            .collect()
    }

    fn covers(rects: &[(f32, f32, f32, f32)], x: f32, y: f32) -> bool {
        rects.iter().any(|(x0, y0, x1, y1)| x >= *x0 && x < *x1 && y >= *y0 && y < *y1)
    }

    #[test]
    fn every_box_drawing_character_is_covered() {
        for cp in 0x2500..=0x257F {
            let ch = char::from_u32(cp).unwrap();
            assert!(shapes(ch, W, H, 2.0).is_some_and(|s| !s.is_empty()), "{cp:#x}");
        }
        assert!(shapes('a', W, H, 2.0).is_none());
        assert!(shapes('\u{E0A0}', W, H, 2.0).is_none());
    }

    #[test]
    fn horizontal_line_spans_the_cell_without_gaps() {
        let r = rects('─', 2.0);
        assert_eq!(r.len(), 1);
        let (x0, y0, x1, y1) = r[0];
        assert_eq!((x0, x1), (0.0, W));
        assert!(y0 > 0.0 && y1 < H);
        assert!(((y0 + y1) / 2.0 - H / 2.0).abs() <= 0.5);
    }

    #[test]
    fn corners_fill_the_junction() {
        let r = rects('┌', 2.0);
        assert!(covers(&r, W / 2.0, H / 2.0));
        assert!(covers(&r, W - 0.25, H / 2.0));
        assert!(covers(&r, W / 2.0, H - 0.25));
        assert!(!covers(&r, 0.25, H / 2.0));
    }

    #[test]
    fn double_corner_leaves_the_inner_gap_open() {
        let r = rects('╔', 2.0);
        let (cx, cy) = (W / 2.0, H / 2.0);
        assert!(!covers(&r, cx, cy), "the centre of a double corner is the gap");
        assert!(covers(&r, cx - 0.75, cy - 0.75));
        assert!(covers(&r, cx + 0.5, cy + 0.5));
        assert!(!covers(&r, cx - 2.0, cy));
    }

    #[test]
    fn blocks_cover_exact_fractions() {
        assert_eq!(rects('█', 2.0), vec![(0.0, 0.0, W, H)]);
        assert_eq!(rects('▀', 2.0), vec![(0.0, 0.0, W, H / 2.0)]);
        assert_eq!(rects('▄', 2.0), vec![(0.0, H / 2.0, W, H)]);
        assert_eq!(rects('▌', 2.0), vec![(0.0, 0.0, W / 2.0, H)]);
        assert_eq!(rects('▚', 2.0).len(), 2);
        let shade = shapes('▒', W, H, 2.0).unwrap();
        assert!(matches!(shade[0], Shape::Rect { alpha, .. } if (alpha - 0.5).abs() < 1e-6));
    }

    #[test]
    fn braille_draws_one_dot_per_bit() {
        assert!(shapes('\u{2800}', W, H, 2.0).unwrap().is_empty());
        assert_eq!(rects('\u{28FF}', 2.0).len(), 8);
        assert_eq!(rects('\u{2801}', 2.0).len(), 1);
    }

    #[test]
    fn powerline_triangles_reach_the_cell_edges() {
        let Some(shapes) = shapes('\u{E0B0}', W, H, 2.0) else { panic!() };
        let Shape::Fill(points) = &shapes[0] else { panic!() };
        assert_eq!(points, &vec![(0.0, 0.0), (W, H / 2.0), (0.0, H)]);
    }

    #[test]
    fn rectangles_snap_to_device_pixels() {
        for ch in ['┼', '╬', '━', '▙', '\u{28AA}'] {
            for (x0, y0, x1, y1) in rects(ch, 2.0) {
                for v in [x0, y0, x1, y1] {
                    assert_eq!((v * 2.0).fract(), 0.0, "{ch} {v}");
                }
            }
        }
    }
}
