use serde::{de, Deserialize, Deserializer, Serialize, Serializer};

/// A tab's color tag. Encoded as its integer raw value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TerminalTabColor {
    #[default]
    None,
    Blue,
    Purple,
    Pink,
    Red,
    Orange,
    Yellow,
    Green,
    Teal,
    Graphite,
}

impl TerminalTabColor {
    pub const ALL: [Self; 10] = [
        Self::None,
        Self::Blue,
        Self::Purple,
        Self::Pink,
        Self::Red,
        Self::Orange,
        Self::Yellow,
        Self::Green,
        Self::Teal,
        Self::Graphite,
    ];

    /// The rows the color picker lays the swatches out in.
    pub const PALETTE_ROWS: [[Self; 5]; 2] = [
        [Self::None, Self::Blue, Self::Purple, Self::Pink, Self::Red],
        [Self::Orange, Self::Yellow, Self::Green, Self::Teal, Self::Graphite],
    ];

    pub fn raw_value(self) -> u8 {
        self as u8
    }

    pub fn from_raw_value(raw: i64) -> Option<Self> {
        usize::try_from(raw).ok().and_then(|i| Self::ALL.get(i).copied())
    }

    pub fn localized_name(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Blue => "Blue",
            Self::Purple => "Purple",
            Self::Pink => "Pink",
            Self::Red => "Red",
            Self::Orange => "Orange",
            Self::Yellow => "Yellow",
            Self::Green => "Green",
            Self::Teal => "Teal",
            Self::Graphite => "Graphite",
        }
    }
}

impl Serialize for TerminalTabColor {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u8(self.raw_value())
    }
}

impl<'de> Deserialize<'de> for TerminalTabColor {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = i64::deserialize(deserializer)?;
        Self::from_raw_value(raw)
            .ok_or_else(|| de::Error::custom(format!("invalid tab color raw value {raw}")))
    }
}

/// Where the horizontal tab layout puts its new tab button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum HorizontalNewTabButtonPlacement {
    Titlebar,
    #[default]
    Tabbar,
}

impl HorizontalNewTabButtonPlacement {
    pub const DEFAULTS_KEY: &'static str = "HorizontalNewTabButtonPlacement";
    pub const ALL: [Self; 2] = [Self::Titlebar, Self::Tabbar];

    pub fn raw_value(self) -> &'static str {
        match self {
            Self::Titlebar => "titlebar",
            Self::Tabbar => "tabbar",
        }
    }

    pub fn from_raw_value(raw: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.raw_value() == raw)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Titlebar => "Title Bar",
            Self::Tabbar => "Tab Bar",
        }
    }

    pub fn shows_in_titlebar(self, sidebar_visible: bool, titlebar_available: bool) -> bool {
        titlebar_available && (sidebar_visible || self == Self::Titlebar)
    }
}

/// Title shown for a tab with no title.
pub const EMPTY_TAB_TITLE: &str = "Terminal";

/// Longest title kept in a closed-window snapshot, in characters.
pub const CLOSED_WINDOW_TITLE_LIMIT: usize = 120;

const BELL_PREFIX: &str = "🔔 ";

/// A user-entered title override. Blank input clears the override.
pub fn normalize_title_override(input: &str) -> Option<String> {
    (!input.is_empty()).then(|| input.to_string())
}

/// The title with a bell marker in front when the surface rang and the `title` bell feature is on.
pub fn title_with_bell(title: &str, bell: bool, bell_feature_title: bool) -> String {
    if bell && bell_feature_title {
        format!("{BELL_PREFIX}{title}")
    } else {
        title.to_string()
    }
}

/// The title the window shows: the tab title plus the git branch, when known.
pub fn window_title(title: &str, branch: Option<&str>) -> String {
    match branch {
        Some(branch) => format!("{title} [{branch}]"),
        None => title.to_string(),
    }
}

/// The title a tab row shows. A shell's default `user@host:` prefix is dropped when the
/// path follows it; a title the user set is shown untouched.
pub fn tab_title(title: &str, has_override: bool) -> String {
    if has_override {
        return title.to_string();
    }
    let (bell, rest) = match title.strip_prefix(BELL_PREFIX) {
        Some(rest) => (BELL_PREFIX, rest),
        None => ("", title),
    };
    match strip_user_host(rest) {
        Some(path) => format!("{bell}{path}"),
        None => title.to_string(),
    }
}

/// Matches `user@host:` followed by `/` or `~` (and then `/` or the end), returning what follows the colon.
fn strip_user_host(text: &str) -> Option<&str> {
    let at = text.find('@')?;
    let user = &text[..at];
    if user.is_empty() || user.chars().any(char::is_whitespace) {
        return None;
    }
    let after_user = &text[at + 1..];
    let colon = after_user.find(':')?;
    let host = &after_user[..colon];
    if host.is_empty() || host.chars().any(char::is_whitespace) {
        return None;
    }
    let path = &after_user[colon + 1..];
    let follows_path = path.starts_with('/')
        || path
            .strip_prefix('~')
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'));
    follows_path.then_some(path)
}

/// The row title, falling back to "Terminal" when empty.
pub fn tab_item_title(title: &str) -> &str {
    if title.is_empty() {
        EMPTY_TAB_TITLE
    } else {
        title
    }
}

/// The title stored for a closed window: the override when set, cut to the title limit.
pub fn closed_window_title(title_override: Option<&str>, window_title: &str) -> String {
    title_override
        .unwrap_or(window_title)
        .chars()
        .take(CLOSED_WINDOW_TITLE_LIMIT)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_raw_values_follow_declaration_order() {
        for (index, color) in TerminalTabColor::ALL.into_iter().enumerate() {
            assert_eq!(color.raw_value() as usize, index);
            assert_eq!(TerminalTabColor::from_raw_value(index as i64), Some(color));
        }
        assert_eq!(TerminalTabColor::from_raw_value(10), None);
        assert_eq!(TerminalTabColor::from_raw_value(-1), None);
        assert_eq!(TerminalTabColor::Graphite.raw_value(), 9);
    }

    #[test]
    fn color_serializes_as_an_integer() {
        assert_eq!(serde_json::to_string(&TerminalTabColor::Purple).unwrap(), "2");
        assert_eq!(serde_json::from_str::<TerminalTabColor>("8").unwrap(), TerminalTabColor::Teal);
        assert!(serde_json::from_str::<TerminalTabColor>("10").is_err());
        assert!(serde_json::from_str::<TerminalTabColor>("\"blue\"").is_err());
    }

    #[test]
    fn palette_rows_cover_every_color_once() {
        let mut shown: Vec<_> = TerminalTabColor::PALETTE_ROWS.iter().flatten().copied().collect();
        shown.sort_by_key(|c| c.raw_value());
        assert_eq!(shown, TerminalTabColor::ALL.to_vec());
        assert_eq!(TerminalTabColor::Teal.localized_name(), "Teal");
    }

    #[test]
    fn new_tab_button_placement_defaults_to_the_tab_bar() {
        assert_eq!(HorizontalNewTabButtonPlacement::default(), HorizontalNewTabButtonPlacement::Tabbar);
        assert_eq!(HorizontalNewTabButtonPlacement::from_raw_value("titlebar"), Some(HorizontalNewTabButtonPlacement::Titlebar));
        assert_eq!(HorizontalNewTabButtonPlacement::from_raw_value("other"), None);
        assert_eq!(HorizontalNewTabButtonPlacement::Titlebar.label(), "Title Bar");
        assert_eq!(HorizontalNewTabButtonPlacement::Tabbar.label(), "Tab Bar");
    }

    #[test]
    fn new_tab_button_shows_in_the_titlebar_only_when_available() {
        use HorizontalNewTabButtonPlacement::{Tabbar, Titlebar};
        assert!(Titlebar.shows_in_titlebar(false, true));
        assert!(Titlebar.shows_in_titlebar(true, true));
        assert!(Tabbar.shows_in_titlebar(true, true));
        assert!(!Tabbar.shows_in_titlebar(false, true));
        assert!(!Titlebar.shows_in_titlebar(true, false));
        assert!(!Tabbar.shows_in_titlebar(true, false));
    }

    #[test]
    fn shell_tab_title_shows_only_the_path() {
        let cases = [
            (
                "engineering@Administrators-MacBook-Air:~/worktrees/angelly/feat-agentic",
                "~/worktrees/angelly/feat-agentic",
            ),
            ("user@host:/tmp/project with spaces", "/tmp/project with spaces"),
            ("user@host:~", "~"),
            ("🔔 user@host:~/project", "🔔 ~/project"),
            ("~/project", "~/project"),
            ("vim main.swift", "vim main.swift"),
            ("Build: running", "Build: running"),
            ("user@host: running", "user@host: running"),
            ("user@host:~foo", "user@host:~foo"),
            ("@host:/tmp", "@host:/tmp"),
            ("user name@host:/tmp", "user name@host:/tmp"),
        ];
        for (title, expected) in cases {
            assert_eq!(tab_title(title, false), expected, "{title}");
        }
    }

    #[test]
    fn custom_tab_title_keeps_its_user_and_host_text() {
        assert_eq!(tab_title("user@host:~/my task", true), "user@host:~/my task");
    }

    #[test]
    fn empty_tab_titles_fall_back_to_terminal() {
        assert_eq!(tab_item_title(""), "Terminal");
        assert_eq!(tab_item_title("vim"), "vim");
    }

    #[test]
    fn blank_overrides_are_cleared() {
        assert_eq!(normalize_title_override(""), None);
        assert_eq!(normalize_title_override("Work"), Some("Work".to_string()));
    }

    #[test]
    fn window_and_bell_titles() {
        assert_eq!(window_title("zsh", Some("main")), "zsh [main]");
        assert_eq!(window_title("zsh", None), "zsh");
        assert_eq!(title_with_bell("zsh", true, true), "🔔 zsh");
        assert_eq!(title_with_bell("zsh", true, false), "zsh");
        assert_eq!(title_with_bell("zsh", false, true), "zsh");
    }

    #[test]
    fn closed_window_titles_prefer_the_override_and_are_truncated() {
        assert_eq!(closed_window_title(Some("Work"), "zsh"), "Work");
        assert_eq!(closed_window_title(None, "zsh"), "zsh");
        let long = "é".repeat(200);
        assert_eq!(closed_window_title(None, &long).chars().count(), 120);
    }
}
