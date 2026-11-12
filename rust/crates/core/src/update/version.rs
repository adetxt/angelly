use std::cmp::Ordering;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Number,
    Text,
    Separator,
}

fn kind(c: char) -> Kind {
    if c.is_ascii_digit() {
        Kind::Number
    } else if c == '.' || c.is_whitespace() || c.is_ascii_punctuation() {
        Kind::Separator
    } else {
        Kind::Text
    }
}

fn split(version: &str) -> Vec<String> {
    let mut parts: Vec<String> = Vec::new();
    let mut previous: Option<Kind> = None;
    for c in version.chars() {
        let current = kind(c);
        match (parts.last_mut(), previous) {
            (Some(part), Some(prev)) if prev == current && current != Kind::Separator => part.push(c),
            _ => parts.push(c.to_string()),
        }
        previous = Some(current);
    }
    parts
}

fn part_kind(part: &str) -> Kind {
    part.chars().next().map(kind).unwrap_or(Kind::Separator)
}

/// Orders version strings the way Sparkle's standard comparator does: runs of digits
/// compare numerically, runs of letters compare as text and sort below numbers, and a
/// version that continues with letters (`1.0b1`) sorts below the one that stops (`1.0`).
pub fn compare_versions(a: &str, b: &str) -> Ordering {
    let (parts_a, parts_b) = (split(a), split(b));
    for (pa, pb) in parts_a.iter().zip(&parts_b) {
        let (ka, kb) = (part_kind(pa), part_kind(pb));
        if ka == kb {
            let ordering = match ka {
                Kind::Number => {
                    let value = |p: &str| p.trim_start_matches('0').parse::<u128>().unwrap_or(0);
                    let by_length = pa.trim_start_matches('0').len().cmp(&pb.trim_start_matches('0').len());
                    by_length.then_with(|| value(pa).cmp(&value(pb)))
                }
                Kind::Text => pa.cmp(pb),
                Kind::Separator => Ordering::Equal,
            };
            if ordering != Ordering::Equal {
                return ordering;
            }
        } else if ka != Kind::Text && kb == Kind::Text {
            return Ordering::Greater;
        } else if ka == Kind::Text && kb != Kind::Text {
            return Ordering::Less;
        } else if ka == Kind::Number {
            return Ordering::Greater;
        } else {
            return Ordering::Less;
        }
    }
    match parts_a.len().cmp(&parts_b.len()) {
        Ordering::Equal => Ordering::Equal,
        longer => {
            let next = if longer == Ordering::Greater { &parts_a[parts_b.len()] } else { &parts_b[parts_a.len()] };
            if part_kind(next) == Kind::Text {
                longer.reverse()
            } else {
                longer
            }
        }
    }
}

/// Whether `candidate` is strictly newer than `current`.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    compare_versions(candidate, current) == Ordering::Greater
}

#[cfg(test)]
mod tests {
    use super::*;
    use Ordering::{Equal, Greater, Less};

    #[test]
    fn numeric_components_compare_numerically() {
        assert_eq!(compare_versions("0.15.0", "0.14.0"), Greater);
        assert_eq!(compare_versions("0.9.0", "0.10.0"), Less);
        assert_eq!(compare_versions("1.2.3", "1.2.3"), Equal);
        assert_eq!(compare_versions("1.02", "1.2"), Equal);
        assert_eq!(compare_versions("10", "9"), Greater);
    }

    #[test]
    fn longer_numeric_versions_win_over_prefixes() {
        assert_eq!(compare_versions("1.0.1", "1.0"), Greater);
        assert_eq!(compare_versions("1.0", "1.0.1"), Less);
        assert_eq!(compare_versions("1.0.0", "1.0"), Greater);
    }

    #[test]
    fn prerelease_suffixes_sort_below_the_release() {
        assert_eq!(compare_versions("1.0b1", "1.0"), Less);
        assert_eq!(compare_versions("1.0", "1.0b1"), Greater);
        assert_eq!(compare_versions("1.0beta", "1.0alpha"), Greater);
        assert_eq!(compare_versions("1.0b2", "1.0b10"), Less);
        assert_eq!(compare_versions("1.0rc1", "1.0.1"), Less);
    }

    #[test]
    fn newer_is_strict() {
        assert!(is_newer("0.16.0", "0.15.0"));
        assert!(!is_newer("0.15.0", "0.15.0"));
        assert!(!is_newer("0.14.9", "0.15.0"));
    }
}
