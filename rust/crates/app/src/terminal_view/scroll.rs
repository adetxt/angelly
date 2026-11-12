//! Scroll bookkeeping: turning wheel and trackpad deltas into whole rows and
//! mapping the viewport onto a scrollbar thumb.

use std::time::{Duration, Instant};

/// Collects fractional rows from precision scrolling so slow trackpad
/// movement still scrolls, one row at a time, instead of being rounded away.
#[derive(Debug, Default)]
pub struct RowAccumulator {
    remainder: f32,
}

impl RowAccumulator {
    /// Adds `rows` (positive scrolls toward older output) and returns the
    /// whole rows to scroll now.
    pub fn push(&mut self, rows: f32) -> i32 {
        if rows != 0.0 && rows.signum() != self.remainder.signum() && self.remainder != 0.0 {
            self.remainder = 0.0;
        }
        self.remainder += rows;
        let whole = self.remainder.trunc();
        self.remainder -= whole;
        whole as i32
    }

    pub fn reset(&mut self) {
        self.remainder = 0.0;
    }
}

/// The scrollbar thumb for a viewport of `len` rows at row `offset` out of
/// `total`, inside a track of `track` pixels: `(top, height)`. `None` when
/// everything fits.
pub fn thumb(track: f32, total: u64, offset: u64, len: u64, min_height: f32) -> Option<(f32, f32)> {
    if total <= len || track <= 0.0 {
        return None;
    }
    let height = (track * len as f32 / total as f32).max(min_height).min(track);
    let travel = track - height;
    let max_offset = (total - len) as f32;
    let top = travel * (offset.min(total - len) as f32 / max_offset);
    Some((top, height))
}

/// The viewport row that puts the thumb's top edge at `top`.
pub fn offset_for_thumb(track: f32, total: u64, len: u64, thumb_height: f32, top: f32) -> u64 {
    let travel = (track - thumb_height).max(1.0);
    let fraction = (top / travel).clamp(0.0, 1.0);
    (fraction * (total.saturating_sub(len)) as f32).round() as u64
}

const VISIBLE_FOR: Duration = Duration::from_millis(900);
const FADE: Duration = Duration::from_millis(250);

/// Auto-hiding overlay scroller state, macOS style: the thumb appears when
/// the viewport moves or the pointer rests on the scroller, then fades out.
#[derive(Debug, Default)]
pub struct ScrollerVisibility {
    last_activity: Option<Instant>,
    pub hovering: bool,
    pub dragging: bool,
}

impl ScrollerVisibility {
    pub fn poke(&mut self) {
        self.last_activity = Some(Instant::now());
    }

    /// Opacity from 0 to 1 at `now`, and whether it is still animating.
    pub fn opacity(&self, now: Instant) -> (f32, bool) {
        if self.hovering || self.dragging {
            return (1.0, false);
        }
        let Some(at) = self.last_activity else { return (0.0, false) };
        let idle = now.saturating_duration_since(at);
        if idle < VISIBLE_FOR {
            (1.0, true)
        } else if idle < VISIBLE_FOR + FADE {
            (1.0 - (idle - VISIBLE_FOR).as_secs_f32() / FADE.as_secs_f32(), true)
        } else {
            (0.0, false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accumulator_keeps_fractions() {
        let mut a = RowAccumulator::default();
        assert_eq!(a.push(0.4), 0);
        assert_eq!(a.push(0.4), 0);
        assert_eq!(a.push(0.4), 1);
        assert_eq!(a.push(-0.3), 0, "reversing drops the stale remainder");
        assert_eq!(a.push(-1.5), -1);
        assert_eq!(a.push(-0.4), -1);
    }

    #[test]
    fn thumb_covers_viewport_fraction() {
        assert_eq!(thumb(100.0, 50, 0, 50, 10.0), None);
        let (top, h) = thumb(100.0, 200, 0, 50, 10.0).unwrap();
        assert_eq!((top, h), (0.0, 25.0));
        let (top, h) = thumb(100.0, 200, 150, 50, 10.0).unwrap();
        assert_eq!((top, h), (75.0, 25.0));
        let (_, h) = thumb(100.0, 100_000, 0, 50, 20.0).unwrap();
        assert_eq!(h, 20.0);
    }

    #[test]
    fn dragging_inverts_the_thumb_mapping() {
        let (top, h) = thumb(100.0, 200, 90, 50, 10.0).unwrap();
        assert_eq!(offset_for_thumb(100.0, 200, 50, h, top), 90);
        assert_eq!(offset_for_thumb(100.0, 200, 50, h, -5.0), 0);
        assert_eq!(offset_for_thumb(100.0, 200, 50, h, 500.0), 150);
    }

    #[test]
    fn visibility_fades_after_activity() {
        let mut v = ScrollerVisibility::default();
        let t0 = Instant::now();
        assert_eq!(v.opacity(t0), (0.0, false));
        v.poke();
        assert_eq!(v.opacity(Instant::now()).0, 1.0);
        v.last_activity = Some(t0 - Duration::from_millis(1000));
        let (o, animating) = v.opacity(t0);
        assert!(o > 0.0 && o < 1.0 && animating);
        v.last_activity = Some(t0 - Duration::from_secs(5));
        assert_eq!(v.opacity(t0), (0.0, false));
        v.hovering = true;
        assert_eq!(v.opacity(t0), (1.0, false));
    }
}
