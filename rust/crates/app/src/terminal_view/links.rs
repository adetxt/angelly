//! URL and file path detection in terminal text, including links that wrap onto the next
//! row, and the user's own `link` patterns.

use std::ops::Range;

use regex::Regex;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkKind {
    /// An explicit scheme: `https://`, `mailto:`, `file://`, ...
    Url,
    /// `/abs`, `~/home`, `./rel` or `../rel`, optionally with `:line[:col]`.
    Path,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkMatch {
    /// Range of char indexes into the scanned slice.
    pub range: Range<usize>,
    pub kind: LinkKind,
}

const SCHEMES: &[&str] = &[
    "https://",
    "http://",
    "ftp://",
    "ssh://",
    "git://",
    "file://",
    "gemini://",
    "gopher://",
    "news://",
    "ipfs://",
    "ipns://",
    "magnet:",
    "mailto:",
];

fn is_url_char(c: char) -> bool {
    c.is_alphanumeric()
        || matches!(
            c,
            '-' | '.'
                | '_'
                | '~'
                | ':'
                | '/'
                | '?'
                | '#'
                | '@'
                | '!'
                | '$'
                | '&'
                | '\''
                | '('
                | ')'
                | '*'
                | '+'
                | ','
                | ';'
                | '='
                | '%'
        )
}

fn is_path_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '-' | '.' | '_' | '~' | '/' | '@' | '+' | ',' | '=' | '%' | ':')
}

/// Drops trailing punctuation and closing brackets without an opener.
fn trim_end(chars: &[char], start: usize, mut end: usize) -> usize {
    while end > start {
        let c = chars[end - 1];
        let drop = match c {
            '.' | ',' | ';' | ':' | '!' | '?' | '\'' | '*' => true,
            ')' => {
                let span = &chars[start..end];
                span.iter().filter(|c| **c == ')').count() > span.iter().filter(|c| **c == '(').count()
            }
            _ => false,
        };
        if !drop {
            break;
        }
        end -= 1;
    }
    end
}

fn starts_with(chars: &[char], at: usize, pattern: &str) -> bool {
    pattern.chars().enumerate().all(|(i, p)| chars.get(at + i).is_some_and(|c| c.to_ascii_lowercase() == p))
}

fn boundary_before(chars: &[char], at: usize) -> bool {
    at == 0 || !chars[at - 1].is_alphanumeric() && !matches!(chars[at - 1], '/' | '.' | '-' | '_' | '~' | ':')
}

/// Finds the links in one row of text.
pub fn scan(chars: &[char]) -> Vec<LinkMatch> {
    let mut out: Vec<LinkMatch> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if !boundary_before(chars, i) {
            i += 1;
            continue;
        }
        if let Some(scheme) = SCHEMES.iter().find(|s| starts_with(chars, i, s)) {
            let body = i + scheme.len();
            let mut end = body;
            while end < chars.len() && is_url_char(chars[end]) {
                end += 1;
            }
            let end = trim_end(chars, i, end);
            if end > body {
                out.push(LinkMatch { range: i..end, kind: LinkKind::Url });
                i = end;
                continue;
            }
        }
        let path_start = match chars[i] {
            '/' => Some(1),
            '~' if chars.get(i + 1) == Some(&'/') => Some(2),
            '.' if chars.get(i + 1) == Some(&'/') => Some(2),
            '.' if chars.get(i + 1) == Some(&'.') && chars.get(i + 2) == Some(&'/') => Some(3),
            _ => None,
        };
        if let Some(prefix) = path_start {
            let mut end = i + prefix;
            while end < chars.len() && is_path_char(chars[end]) {
                end += 1;
            }
            let end = trim_end(chars, i, end);
            if end > i + prefix {
                out.push(LinkMatch { range: i..end, kind: LinkKind::Path });
                i = end;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// A run of cells on one row; `end` is inclusive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub row: u16,
    pub start: u16,
    pub end: u16,
}

impl Span {
    pub fn contains(&self, col: u16, row: u16) -> bool {
        self.row == row && (self.start..=self.end).contains(&col)
    }
}

/// Splits a range of chars of consecutive `cols`-wide rows starting at `first_row` into one
/// span per row it touches.
pub fn spans(range: Range<usize>, cols: usize, first_row: usize) -> Vec<Span> {
    let mut out = Vec::new();
    let mut at = range.start;
    while at < range.end {
        let row = at / cols;
        let end = range.end.min((row + 1) * cols);
        out.push(Span {
            row: (first_row + row) as u16,
            start: (at % cols) as u16,
            end: ((end - 1) % cols) as u16,
        });
        at = end;
    }
    out
}

/// The run of consecutive indexes around `at` (below `len`) that all satisfy `same`.
pub fn run_around(at: usize, len: usize, same: impl Fn(usize) -> bool) -> Range<usize> {
    let mut start = at;
    while start > 0 && same(start - 1) {
        start -= 1;
    }
    let mut end = at + 1;
    while end < len && same(end) {
        end += 1;
    }
    start..end
}

/// What clicking a user-defined link does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkAction {
    /// Open the matched text like any other link.
    Open,
    /// Run a keybind action such as `text:...` or `new_split:right`.
    Run(String),
}

/// A `link = regex:<pattern>[,action:<open|keybind action>]` entry.
#[derive(Clone, Debug)]
pub struct CustomLink {
    regex: Regex,
    pub action: LinkAction,
}

impl PartialEq for CustomLink {
    fn eq(&self, other: &Self) -> bool {
        self.regex.as_str() == other.regex.as_str() && self.action == other.action
    }
}

impl CustomLink {
    /// Parses one `link` value. The action, when given, comes after the last `,action:` so the
    /// pattern may contain commas. `None` when the pattern is missing or not a valid regex.
    pub fn parse(value: &str) -> Option<CustomLink> {
        let value = value.trim().strip_prefix("regex:")?;
        let (pattern, action) = match value.rfind(",action:") {
            Some(at) => (&value[..at], value[at + ",action:".len()..].trim()),
            None => (value, "open"),
        };
        let action = match action {
            "open" | "" => LinkAction::Open,
            other => LinkAction::Run(other.to_string()),
        };
        Some(CustomLink { regex: Regex::new(pattern).ok()?, action })
    }

    /// The match covering char index `at` of `chars`, as a range of char indexes.
    pub fn find_at(&self, chars: &[char], at: usize) -> Option<Range<usize>> {
        let text: String = chars.iter().collect();
        // Byte offset of every char, so matches map back to cells.
        let mut offsets: Vec<usize> = text.char_indices().map(|(i, _)| i).collect();
        offsets.push(text.len());
        self.regex.find_iter(&text).find_map(|m| {
            let start = offsets.binary_search(&m.start()).ok()?;
            let end = offsets.binary_search(&m.end()).ok()?;
            (start <= at && at < end).then_some(start..end)
        })
    }
}

/// The link covering char index `at`, if any.
pub fn link_at(chars: &[char], at: usize) -> Option<LinkMatch> {
    scan(chars).into_iter().find(|m| m.range.contains(&at))
}

/// Splits a trailing `:line` or `:line:col` suffix off a path match.
pub fn split_position(path: &str) -> (&str, Option<(u32, Option<u32>)>) {
    let number = |s: &str| s.parse::<u32>().ok();
    let Some((rest, last)) = path.rsplit_once(':') else { return (path, None) };
    let Some(last) = number(last) else { return (path, None) };
    if let Some((head, middle)) = rest.rsplit_once(':') {
        if let Some(line) = number(middle) {
            return (head, Some((line, Some(last))));
        }
    }
    (rest, Some((last, None)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(text: &str) -> Vec<(String, LinkKind)> {
        let chars: Vec<char> = text.chars().collect();
        scan(&chars).into_iter().map(|m| (chars[m.range].iter().collect(), m.kind)).collect()
    }

    #[test]
    fn finds_urls_and_trims_punctuation() {
        assert_eq!(
            found("see https://example.com/a?b=1#c, then"),
            vec![("https://example.com/a?b=1#c".into(), LinkKind::Url)]
        );
        assert_eq!(
            found("(https://en.wikipedia.org/wiki/Rust_(language))."),
            vec![("https://en.wikipedia.org/wiki/Rust_(language)".into(), LinkKind::Url)]
        );
        assert_eq!(found("mail mailto:a@b.co now"), vec![("mailto:a@b.co".into(), LinkKind::Url)]);
        assert_eq!(found("no scheme example.com"), vec![]);
        assert_eq!(found("https://"), vec![]);
    }

    #[test]
    fn finds_paths() {
        assert_eq!(
            found("open ./src/main.rs:12:3 and ~/notes.txt"),
            vec![("./src/main.rs:12:3".into(), LinkKind::Path), ("~/notes.txt".into(), LinkKind::Path)]
        );
        assert_eq!(found("cd /usr/local/bin"), vec![("/usr/local/bin".into(), LinkKind::Path)]);
        assert_eq!(found("a/b is not a path"), vec![]);
        assert_eq!(found("../up"), vec![("../up".into(), LinkKind::Path)]);
    }

    #[test]
    fn url_wins_over_path_inside_it() {
        assert_eq!(found("https://x.io/a/b/c"), vec![("https://x.io/a/b/c".into(), LinkKind::Url)]);
    }

    #[test]
    fn link_at_index() {
        let chars: Vec<char> = "go http://a.b/c ok".chars().collect();
        assert!(link_at(&chars, 2).is_none());
        assert_eq!(link_at(&chars, 5).unwrap().range, 3..15);
        assert!(link_at(&chars, 16).is_none());
    }

    #[test]
    fn spans_follow_the_rows_a_range_wraps_over() {
        let span = |row, start, end| Span { row, start, end };
        assert_eq!(spans(3..8, 10, 4), vec![span(4, 3, 7)]);
        assert_eq!(spans(7..24, 10, 4), vec![span(4, 7, 9), span(5, 0, 9), span(6, 0, 3)]);
        assert_eq!(spans(10..20, 10, 0), vec![span(1, 0, 9)]);
        assert!(span(1, 2, 4).contains(3, 1) && !span(1, 2, 4).contains(3, 2) && !span(1, 2, 4).contains(5, 1));
    }

    #[test]
    fn a_url_wrapping_onto_the_next_row_is_one_link() {
        let line: Vec<char> = "see https://example.com/a/long/pa th".chars().collect();
        let m = link_at(&line, 10).unwrap();
        assert_eq!(line[m.range.clone()].iter().collect::<String>(), "https://example.com/a/long/pa");
        assert_eq!(spans(m.range, 20, 0).len(), 2);
    }

    #[test]
    fn runs_stop_where_the_predicate_fails() {
        let flags = [false, true, true, true, false, true];
        assert_eq!(run_around(2, flags.len(), |i| flags[i]), 1..4);
        assert_eq!(run_around(5, flags.len(), |i| flags[i]), 5..6);
    }

    #[test]
    fn custom_links_parse_pattern_and_action() {
        let open = CustomLink::parse("regex:JIRA-\\d{1,5}").unwrap();
        assert_eq!(open.action, LinkAction::Open);
        let run = CustomLink::parse("regex:foo,bar,action:text:echo hi\\n").unwrap();
        assert_eq!(run.action, LinkAction::Run("text:echo hi\\n".into()));
        assert!(CustomLink::parse("regex:(unclosed").is_none());
        assert!(CustomLink::parse("JIRA-1").is_none());
    }

    #[test]
    fn custom_links_match_by_char_index_even_after_wide_text() {
        let link = CustomLink::parse("regex:\\bBUG-\\d+").unwrap();
        let chars: Vec<char> = "\u{e9}\u{e9} fixes BUG-42 now".chars().collect();
        let at = chars.iter().position(|c| *c == 'B').unwrap();
        let range = link.find_at(&chars, at + 2).unwrap();
        assert_eq!(chars[range].iter().collect::<String>(), "BUG-42");
        assert!(link.find_at(&chars, 0).is_none());
    }

    #[test]
    fn splits_line_and_column() {
        assert_eq!(split_position("a/b.rs:12:3"), ("a/b.rs", Some((12, Some(3)))));
        assert_eq!(split_position("a/b.rs:12"), ("a/b.rs", Some((12, None))));
        assert_eq!(split_position("a/b.rs"), ("a/b.rs", None));
    }
}
