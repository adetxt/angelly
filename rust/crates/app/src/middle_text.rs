//! A line of text that is shortened in the middle when it does not fit, so both the
//! start and the end of a long tab title stay readable.

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::*;

const ELLIPSIS: char = '…';

/// `text` with its middle replaced by an ellipsis until `fits` accepts it. The head keeps
/// the extra character when an odd number of characters stays.
pub fn middle_truncate(text: &str, fits: impl Fn(&str) -> bool) -> String {
    if fits(text) {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let build = |keep: usize| {
        let tail = keep / 2;
        let head: String = chars[..keep - tail].iter().collect();
        let end: String = chars[chars.len() - tail..].iter().collect();
        format!("{head}{ELLIPSIS}{end}")
    };
    let (mut low, mut high) = (0, chars.len().saturating_sub(1));
    while low < high {
        let mid = (low + high).div_ceil(2);
        if fits(&build(mid)) {
            low = mid;
        } else {
            high = mid - 1;
        }
    }
    build(low)
}

pub struct MiddleText {
    text: SharedString,
    size: Pixels,
    weight: FontWeight,
    color: Hsla,
}

impl MiddleText {
    pub fn new(text: impl Into<SharedString>, size: f32, weight: FontWeight, color: Hsla) -> Self {
        Self { text: text.into(), size: px(size), weight, color }
    }
}

impl IntoElement for MiddleText {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

pub struct Shown {
    text: Rc<RefCell<SharedString>>,
    line_height: Pixels,
}

impl MiddleText {
    fn run(&self, window: &Window, len: usize) -> TextRun {
        let mut font = window.text_style().font();
        font.weight = self.weight;
        TextRun { len, font, color: self.color, background_color: None, underline: None, strikethrough: None }
    }
}

impl Element for MiddleText {
    type RequestLayoutState = Shown;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Shown) {
        let shown = Rc::new(RefCell::new(self.text.clone()));
        let line_height = window.line_height();
        let (text, size, run) = (self.text.clone(), self.size, self.run(window, self.text.len()));
        let weight = self.weight;
        let color = self.color;
        let target = shown.clone();
        let style = Style { flex_shrink: 1.0, min_size: Size { width: Length::Definite(px(0.).into()), height: Length::Auto }, ..Style::default() };
        let id = window.request_measured_layout(style, move |known, available, window, _| {
            let measure = |candidate: &str| -> Pixels {
                let mut font = run.font.clone();
                font.weight = weight;
                let run = TextRun { len: candidate.len(), font, color, ..run.clone() };
                window.text_system().shape_line(candidate.to_string().into(), size, &[run], None).width
            };
            let natural = measure(&text);
            let limit = known.width.unwrap_or(match available.width {
                AvailableSpace::Definite(width) => width,
                _ => natural,
            });
            let candidate = middle_truncate(&text, |candidate| measure(candidate) <= limit);
            let width = measure(&candidate).min(limit);
            *target.borrow_mut() = candidate.into();
            Size { width, height: line_height }
        });
        let _ = cx;
        (id, Shown { text: shown, line_height })
    }

    fn prepaint(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, _: Bounds<Pixels>, _: &mut Shown, _: &mut Window, _: &mut App) {}

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        shown: &mut Shown,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let text = shown.text.borrow().clone();
        let run = self.run(window, text.len());
        let line = window.text_system().shape_line(text, self.size, &[run], None);
        let _ = line.paint(bounds.origin, shown.line_height, TextAlign::Left, None, window, cx);
    }
}

#[cfg(test)]
mod tests {
    use std::prelude::v1::test;

    use super::middle_truncate;

    fn fits(width: usize) -> impl Fn(&str) -> bool {
        move |text| text.chars().count() <= width
    }

    #[test]
    fn short_text_is_kept() {
        assert_eq!(middle_truncate("tests", fits(10)), "tests");
    }

    #[test]
    fn the_middle_is_replaced_and_both_ends_stay() {
        assert_eq!(middle_truncate("build output with a rather long title", fits(11)), "build…title");
    }

    #[test]
    fn an_odd_width_gives_the_head_the_extra_character() {
        assert_eq!(middle_truncate("abcdefghij", fits(6)), "abc…ij");
    }

    #[test]
    fn nothing_fits_but_the_ellipsis() {
        assert_eq!(middle_truncate("abcdefghij", fits(1)), "…");
        assert_eq!(middle_truncate("abcdefghij", fits(0)), "…");
    }

    #[test]
    fn wide_characters_are_not_split() {
        assert_eq!(middle_truncate("日本語のタイトルです", fits(5)), "日本…です");
    }
}
