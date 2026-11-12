//! Command palette matching: case-insensitive substring first, then word
//! initials. Ranking mirrors the Swift palette: a leading-colour match beats
//! everything, then title beats subtitle beats description.

/// The searchable parts of a palette entry.
pub trait Matchable {
    fn title(&self) -> &str;
    fn subtitle(&self) -> Option<&str>;
    fn description(&self) -> Option<&str>;
    fn leading_color(&self) -> Option<[f32; 3]>;
}

fn fold(text: &str) -> Vec<char> {
    text.chars().map(|c| c.to_lowercase().next().unwrap_or(c)).collect()
}

/// Character indices of `text` that match `query`, or `None`. Tries a
/// substring match first and falls back to matching each query character
/// against the first letter of successive words.
pub fn matched_indices(text: &str, query: &str) -> Option<Vec<usize>> {
    if query.is_empty() {
        return None;
    }
    let haystack = fold(text);
    let needle = fold(query);
    if needle.len() <= haystack.len() {
        if let Some(start) = (0..=haystack.len() - needle.len()).find(|&i| haystack[i..i + needle.len()] == needle[..]) {
            return Some((start..start + needle.len()).collect());
        }
    }

    let mut next = 0;
    let mut matched = Vec::new();
    let mut at_word_start = true;
    for (index, ch) in haystack.iter().enumerate() {
        if next == needle.len() {
            break;
        }
        if ch.is_whitespace() {
            at_word_start = true;
            continue;
        }
        if at_word_start && *ch == needle[next] {
            matched.push(index);
            next += 1;
        }
        at_word_start = false;
    }
    (next == needle.len()).then_some(matched)
}

const COLOR_NAMES: &[(&str, [f32; 3])] = &[
    ("black", [0., 0., 0.]),
    ("blue", [0., 0., 1.]),
    ("brown", [0.6, 0.4, 0.2]),
    ("cyan", [0., 1., 1.]),
    ("green", [0., 1., 0.]),
    ("magenta", [1., 0., 1.]),
    ("orange", [1., 0.5, 0.]),
    ("purple", [0.5, 0., 0.5]),
    ("red", [1., 0., 0.]),
    ("white", [1., 1., 1.]),
    ("yellow", [1., 1., 0.]),
];

/// How closely `color` matches a colour name mentioned in `query`, from 0
/// (none) to 1 (exact).
pub fn color_score(color: Option<[f32; 3]>, query: &str) -> f64 {
    let Some(color) = color else { return 0.0 };
    let query = query.to_lowercase();
    let mut best: f64 = 0.0;
    for (name, reference) in COLOR_NAMES {
        if !query.contains(name) {
            continue;
        }
        let d = |i: usize| (color[i] - reference[i]) as f64;
        let distance = (2.0 * d(0).powi(2) + 4.0 * d(1).powi(2) + 3.0 * d(2).powi(2)).sqrt();
        const MAX_DISTANCE: f64 = 1.5;
        if distance < MAX_DISTANCE {
            best = best.max(1.0 - distance / MAX_DISTANCE);
        }
    }
    best
}

/// Title (3), subtitle (2), description (1) or no (0) text match.
pub fn text_score(item: &impl Matchable, query: &str) -> u8 {
    if matched_indices(item.title(), query).is_some() {
        3
    } else if item.subtitle().is_some_and(|s| matched_indices(s, query).is_some()) {
        2
    } else if item.description().is_some_and(|s| matched_indices(s, query).is_some()) {
        1
    } else {
        0
    }
}

/// Indices into `items` of the matches for `query`, best first, keeping the
/// original order among equals.
pub fn filter_sorted<T: Matchable>(items: &[T], query: &str) -> Vec<usize> {
    let mut scored: Vec<(usize, f64, u8)> = items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            let color = color_score(item.leading_color(), query);
            let text = text_score(item, query);
            (color > 0.0 || text > 0).then_some((index, color, text))
        })
        .collect();
    scored.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(b.2.cmp(&a.2))
            .then(a.0.cmp(&b.0))
    });
    scored.into_iter().map(|(index, ..)| index).collect()
}

/// Terminal command ordering: ":" sorts before everything so "Foo:" precedes
/// "Foo Bar:", compared case-insensitively.
pub fn palette_sort_key(title: &str) -> String {
    title.replace(':', "\t").to_lowercase()
}

#[cfg(test)]
mod tests {
    use std::prelude::v1::test;

    use super::*;

    struct Item {
        title: &'static str,
        subtitle: Option<&'static str>,
        description: Option<&'static str>,
        color: Option<[f32; 3]>,
    }

    impl Matchable for Item {
        fn title(&self) -> &str {
            self.title
        }
        fn subtitle(&self) -> Option<&str> {
            self.subtitle
        }
        fn description(&self) -> Option<&str> {
            self.description
        }
        fn leading_color(&self) -> Option<[f32; 3]> {
            self.color
        }
    }

    fn item(title: &'static str) -> Item {
        Item { title, subtitle: None, description: None, color: None }
    }

    #[test]
    fn substring_match_is_case_insensitive_and_contiguous() {
        assert_eq!(matched_indices("Reset Terminal", "term"), Some(vec![6, 7, 8, 9]));
        assert_eq!(matched_indices("Reset Terminal", "TERM"), Some(vec![6, 7, 8, 9]));
    }

    #[test]
    fn falls_back_to_word_initials() {
        assert_eq!(matched_indices("Reset Terminal", "rt"), Some(vec![0, 6]));
        assert_eq!(matched_indices("Copy to Clipboard", "ctc"), Some(vec![0, 5, 8]));
    }

    #[test]
    fn initials_must_consume_the_whole_query() {
        assert_eq!(matched_indices("Reset Terminal", "rtz"), None);
        assert_eq!(matched_indices("Reset", "rr"), None);
        assert_eq!(matched_indices("Reset", ""), None);
    }

    #[test]
    fn query_longer_than_text_does_not_match() {
        assert_eq!(matched_indices("ab", "abc"), None);
    }

    #[test]
    fn title_beats_subtitle_beats_description() {
        let items = [
            Item { title: "Other", subtitle: None, description: Some("scroll things"), color: None },
            Item { title: "Other", subtitle: Some("scroll things"), description: None, color: None },
            item("Scroll Page"),
        ];
        assert_eq!(filter_sorted(&items, "scroll"), vec![2, 1, 0]);
    }

    #[test]
    fn equal_scores_keep_original_order() {
        let items = [item("Focus: a"), item("Focus: b"), item("Focus: c")];
        assert_eq!(filter_sorted(&items, "focus"), vec![0, 1, 2]);
    }

    #[test]
    fn non_matching_items_are_dropped() {
        let items = [item("New Tab"), item("Quit")];
        assert_eq!(filter_sorted(&items, "quit"), vec![1]);
        assert!(filter_sorted(&items, "zzz").is_empty());
    }

    #[test]
    fn colour_match_outranks_text_match() {
        let items = [
            item("Red Alert"),
            Item { title: "Focus: build", subtitle: None, description: None, color: Some([1.0, 0.0, 0.0]) },
        ];
        assert_eq!(filter_sorted(&items, "red"), vec![1, 0]);
    }

    #[test]
    fn distant_colours_do_not_match() {
        assert_eq!(color_score(Some([0.0, 0.0, 1.0]), "red"), 0.0);
        assert_eq!(color_score(None, "red"), 0.0);
        assert!(color_score(Some([1.0, 0.0, 0.0]), "make it red") > 0.99);
    }

    #[test]
    fn colon_sorts_before_space() {
        let mut titles = ["Focus Split: Left", "Focus: web", "Focus Tab"];
        titles.sort_by_key(|t| palette_sort_key(t));
        assert_eq!(titles, ["Focus: web", "Focus Split: Left", "Focus Tab"]);
    }
}
