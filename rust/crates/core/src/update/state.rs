use super::release::ReleaseInfo;

/// A semantic colour role for the update pill; the UI maps it to its palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Primary,
    Secondary,
    OnAccent,
    Accent,
    /// System blue darkened 30%.
    BlueDark,
    /// System blue darkened 50%.
    BlueDarker,
    Warning,
    /// Warning at 20% opacity.
    WarningTint,
    Control,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateIcon {
    Question,
    Refresh,
    Package,
    PackageFilled,
    ArrowDown,
    Power,
    Info,
    Warning,
}

/// What the update pill is currently showing. Actions that Sparkle modelled as closures
/// held in each state (cancel, retry, reply) live on the controller instead.
#[derive(Debug, Clone)]
pub enum UpdateState {
    Idle,
    PermissionRequest,
    Checking,
    UpdateAvailable(ReleaseInfo),
    NotFound,
    Error { message: String },
    Downloading { expected_length: Option<u64>, progress: u64 },
    Extracting { progress: f64 },
    /// `release` is `Some` when an automatic update was installed silently and is
    /// waiting for a relaunch.
    Installing { release: Option<ReleaseInfo> },
}

impl PartialEq for UpdateState {
    fn eq(&self, other: &Self) -> bool {
        use UpdateState::*;
        match (self, other) {
            (Idle, Idle) | (PermissionRequest, PermissionRequest) | (Checking, Checking) | (NotFound, NotFound) => true,
            (UpdateAvailable(a), UpdateAvailable(b)) => a.display_version() == b.display_version(),
            (Error { message: a }, Error { message: b }) => a == b,
            (
                Downloading { expected_length: ea, progress: pa },
                Downloading { expected_length: eb, progress: pb },
            ) => ea == eb && pa == pb,
            (Extracting { progress: a }, Extracting { progress: b }) => a == b,
            (Installing { release: a }, Installing { release: b }) => {
                a.as_ref().map(ReleaseInfo::display_version) == b.as_ref().map(ReleaseInfo::display_version)
            }
            _ => false,
        }
    }
}

impl UpdateState {
    /// True if we're installing an update triggered manually.
    pub fn should_terminate_without_warning(&self) -> bool {
        matches!(self, Self::Installing { release: None })
    }

    /// Hidden when idle, and when installing after an automatic update: users who check
    /// manually get an alert instead.
    pub fn is_hidden(&self) -> bool {
        matches!(self, Self::Idle | Self::Installing { release: Some(_) })
    }

    /// True in states that can be force installed.
    pub fn is_installable(&self) -> bool {
        matches!(
            self,
            Self::Checking | Self::UpdateAvailable(_) | Self::Downloading { .. } | Self::Extracting { .. } | Self::Installing { .. }
        )
    }

    /// Whether the user can back out of this state.
    pub fn is_cancellable(&self) -> bool {
        matches!(
            self,
            Self::Checking | Self::UpdateAvailable(_) | Self::Downloading { .. } | Self::NotFound | Self::Error { .. }
        )
    }

    /// The text to display. Empty when idle.
    pub fn text(&self) -> String {
        match self {
            Self::Idle => String::new(),
            Self::PermissionRequest => "Enable Automatic Updates?".to_string(),
            Self::Checking => "Checking for Updates…".to_string(),
            Self::UpdateAvailable(release) => match release.display_version() {
                "" => "Update Available".to_string(),
                version => format!("Update Available: {version}"),
            },
            Self::Downloading { expected_length, progress } => match download_fraction(*expected_length, *progress) {
                Some(fraction) => format!("Downloading: {:.0}%", fraction * 100.0),
                None => "Downloading…".to_string(),
            },
            Self::Extracting { progress } => format!("Preparing: {:.0}%", progress * 100.0),
            Self::Installing { release } => {
                if release.is_some() { "Restart to Complete Update" } else { "Installing…" }.to_string()
            }
            Self::NotFound => "No Updates Available".to_string(),
            Self::Error { message } => message.clone(),
        }
    }

    /// The widest text of states that show progress, so the pill doesn't resize as
    /// percentages change.
    pub fn max_width_text(&self) -> String {
        match self {
            Self::Downloading { .. } => "Downloading: 100%".to_string(),
            Self::Extracting { .. } => "Preparing: 100%".to_string(),
            _ => self.text(),
        }
    }

    pub fn icon(&self) -> Option<UpdateIcon> {
        match self {
            Self::Idle => None,
            Self::PermissionRequest => Some(UpdateIcon::Question),
            Self::Checking => Some(UpdateIcon::Refresh),
            Self::UpdateAvailable(_) => Some(UpdateIcon::PackageFilled),
            Self::Downloading { .. } => Some(UpdateIcon::ArrowDown),
            Self::Extracting { .. } => Some(UpdateIcon::Package),
            Self::Installing { .. } => Some(UpdateIcon::Power),
            Self::NotFound => Some(UpdateIcon::Info),
            Self::Error { .. } => Some(UpdateIcon::Warning),
        }
    }

    /// A longer description, for contexts like the command palette.
    pub fn description(&self) -> String {
        match self {
            Self::Idle => String::new(),
            Self::PermissionRequest => "Configure automatic update preferences".to_string(),
            Self::Checking => "Please wait while we check for available updates".to_string(),
            Self::UpdateAvailable(release) => match release.notes() {
                Some(notes) => notes.label().to_string(),
                None => "Download and install the latest version".to_string(),
            },
            Self::Downloading { .. } => "Downloading the update package".to_string(),
            Self::Extracting { .. } => "Extracting and preparing the update".to_string(),
            Self::Installing { release: Some(release) } => {
                format!("The update is ready. Version: {}", release.display_version())
            }
            Self::Installing { release: None } => "Installing update and preparing to restart".to_string(),
            Self::NotFound => "You are running the latest version".to_string(),
            Self::Error { .. } => "An error occurred during the update process".to_string(),
        }
    }

    /// A version number or progress percentage to badge the state with.
    pub fn badge(&self) -> Option<String> {
        match self {
            Self::UpdateAvailable(release) => Some(release.display_version().to_string()).filter(|v| !v.is_empty()),
            Self::Downloading { expected_length, progress } => {
                download_fraction(*expected_length, *progress).map(|f| format!("{:.0}%", f * 100.0))
            }
            Self::Extracting { progress } => Some(format!("{:.0}%", progress * 100.0)),
            _ => None,
        }
    }

    pub fn icon_tone(&self) -> Tone {
        match self {
            Self::PermissionRequest => Tone::OnAccent,
            Self::UpdateAvailable(_) => Tone::Accent,
            Self::Error { .. } => Tone::Warning,
            _ => Tone::Secondary,
        }
    }

    pub fn background_tone(&self) -> Tone {
        match self {
            Self::PermissionRequest => Tone::BlueDark,
            Self::UpdateAvailable(_) => Tone::Accent,
            Self::NotFound => Tone::BlueDarker,
            Self::Error { .. } => Tone::WarningTint,
            _ => Tone::Control,
        }
    }

    pub fn foreground_tone(&self) -> Tone {
        match self {
            Self::PermissionRequest | Self::UpdateAvailable(_) | Self::NotFound => Tone::OnAccent,
            Self::Error { .. } => Tone::Warning,
            _ => Tone::Primary,
        }
    }
}

fn download_fraction(expected_length: Option<u64>, progress: u64) -> Option<f64> {
    expected_length.filter(|length| *length > 0).map(|length| progress as f64 / length as f64)
}

/// Where to read about a release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReleaseNotes {
    Commit(String),
    CompareTip(String),
    Tagged(String),
}

impl ReleaseNotes {
    pub fn new(display_version: &str, current_commit: Option<&str>) -> Option<Self> {
        if let Some(slug) = semantic_version_slug(display_version) {
            return Some(Self::Tagged(format!("https://ghostty.org/docs/install/release-notes/{slug}")));
        }
        let new_hash = git_hash(display_version)?;
        Some(match current_commit.filter(|c| !c.is_empty()) {
            Some(current) => Self::CompareTip(format!("https://github.com/ghostty-org/ghostty/compare/{current}...{new_hash}")),
            None => Self::Commit(format!("https://github.com/ghostty-org/ghostty/commit/{new_hash}")),
        })
    }

    pub fn url(&self) -> &str {
        match self {
            Self::Commit(url) | Self::CompareTip(url) | Self::Tagged(url) => url,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Commit(_) => "View GitHub Commit",
            Self::CompareTip(_) => "Changes Since This Tip Release",
            Self::Tagged(_) => "View Release Notes",
        }
    }
}

/// `x.y.z` made of digits only, joined with dashes for use in a URL.
fn semantic_version_slug(version: &str) -> Option<String> {
    let parts: Vec<&str> = version.split('.').collect();
    let valid = parts.len() == 3 && parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));
    valid.then(|| parts.join("-"))
}

/// The first run of 7 to 40 lowercase hex digits.
fn git_hash(version: &str) -> Option<String> {
    let bytes = version.as_bytes();
    let mut start = 0;
    while start < bytes.len() {
        if !bytes[start].is_ascii_hexdigit() || bytes[start].is_ascii_uppercase() {
            start += 1;
            continue;
        }
        let run = bytes[start..].iter().take_while(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b)).take(40).count();
        if run >= 7 {
            return Some(version[start..start + run].to_string());
        }
        start += run.max(1);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(version: &str) -> ReleaseInfo {
        ReleaseInfo::for_tests(version)
    }

    #[test]
    fn tagged_release() {
        let notes = ReleaseNotes::new("1.2.3", None).unwrap();
        assert_eq!(notes, ReleaseNotes::Tagged("https://ghostty.org/docs/install/release-notes/1-2-3".into()));
        assert_eq!(notes.label(), "View Release Notes");
    }

    #[test]
    fn tip_release_comparison_with_current_commit() {
        let notes = ReleaseNotes::new("tip-abc1234", Some("def5678")).unwrap();
        assert_eq!(notes, ReleaseNotes::CompareTip("https://github.com/ghostty-org/ghostty/compare/def5678...abc1234".into()));
        assert_eq!(notes.label(), "Changes Since This Tip Release");
    }

    #[test]
    fn tip_release_without_or_with_empty_current_commit() {
        for current in [None, Some("")] {
            let notes = ReleaseNotes::new("tip-abc1234", current).unwrap();
            assert_eq!(notes, ReleaseNotes::Commit("https://github.com/ghostty-org/ghostty/commit/abc1234".into()));
            assert_eq!(notes.label(), "View GitHub Commit");
        }
    }

    #[test]
    fn full_git_hash() {
        let hash = "1234567890abcdef1234567890abcdef12345678";
        let notes = ReleaseNotes::new(&format!("tip-{hash}"), None).unwrap();
        assert_eq!(notes.url(), format!("https://github.com/ghostty-org/ghostty/commit/{hash}"));
    }

    #[test]
    fn versions_without_a_recognizable_pattern() {
        for version in ["unknown-version", "1.2.3-beta", "1.2.3.4", ""] {
            assert_eq!(ReleaseNotes::new(version, None), None, "{version}");
        }
    }

    #[test]
    fn version_with_embedded_hash() {
        let notes = ReleaseNotes::new("v2024.01.15-abc1234", Some("def5678")).unwrap();
        assert_eq!(notes.url(), "https://github.com/ghostty-org/ghostty/compare/def5678...abc1234");
    }

    #[test]
    fn simple_state_equality() {
        assert_eq!(UpdateState::Idle, UpdateState::Idle);
        assert_eq!(UpdateState::Checking, UpdateState::Checking);
        assert_eq!(UpdateState::NotFound, UpdateState::NotFound);
        assert_eq!(UpdateState::PermissionRequest, UpdateState::PermissionRequest);
        assert_eq!(UpdateState::Installing { release: None }, UpdateState::Installing { release: None });
        assert_ne!(UpdateState::Idle, UpdateState::Checking);
    }

    #[test]
    fn downloading_equality_depends_on_progress_and_length() {
        let d = |expected_length, progress| UpdateState::Downloading { expected_length, progress };
        assert_eq!(d(Some(1000), 500), d(Some(1000), 500));
        assert_ne!(d(Some(1000), 500), d(Some(1000), 600));
        assert_ne!(d(Some(1000), 500), d(Some(2000), 500));
        assert_eq!(d(None, 500), d(None, 500));
    }

    #[test]
    fn extracting_equality() {
        assert_eq!(UpdateState::Extracting { progress: 0.5 }, UpdateState::Extracting { progress: 0.5 });
        assert_ne!(UpdateState::Extracting { progress: 0.5 }, UpdateState::Extracting { progress: 0.6 });
    }

    #[test]
    fn error_equality_compares_messages() {
        let e = |m: &str| UpdateState::Error { message: m.into() };
        assert_eq!(e("Error message"), e("Error message"));
        assert_ne!(e("Error 1"), e("Error 2"));
    }

    #[test]
    fn available_and_installing_equality_compare_versions() {
        assert_eq!(UpdateState::UpdateAvailable(release("1.0.0")), UpdateState::UpdateAvailable(release("1.0.0")));
        assert_ne!(UpdateState::UpdateAvailable(release("1.0.0")), UpdateState::UpdateAvailable(release("1.0.1")));
        assert_ne!(UpdateState::Installing { release: Some(release("1.0.0")) }, UpdateState::Installing { release: None });
    }

    #[test]
    fn is_hidden() {
        assert!(UpdateState::Idle.is_hidden());
        assert!(UpdateState::Installing { release: Some(release("1.0.0")) }.is_hidden());
        assert!(!UpdateState::Checking.is_hidden());
        assert!(!UpdateState::Installing { release: None }.is_hidden());
    }

    #[test]
    fn should_terminate_without_warning() {
        assert!(!UpdateState::Idle.should_terminate_without_warning());
        assert!(!UpdateState::Installing { release: Some(release("1.0.0")) }.should_terminate_without_warning());
        assert!(UpdateState::Installing { release: None }.should_terminate_without_warning());
    }

    #[test]
    fn installable_and_cancellable_states() {
        assert!(UpdateState::Checking.is_installable());
        assert!(!UpdateState::NotFound.is_installable());
        assert!(!UpdateState::Error { message: String::new() }.is_installable());
        for cancellable in [UpdateState::Checking, UpdateState::NotFound, UpdateState::Error { message: String::new() }] {
            assert!(cancellable.is_cancellable());
        }
        for fixed in [UpdateState::Idle, UpdateState::PermissionRequest, UpdateState::Extracting { progress: 0.0 }, UpdateState::Installing { release: None }] {
            assert!(!fixed.is_cancellable());
        }
    }

    #[test]
    fn text() {
        assert_eq!(UpdateState::Idle.text(), "");
        assert_eq!(UpdateState::PermissionRequest.text(), "Enable Automatic Updates?");
        assert_eq!(UpdateState::Checking.text(), "Checking for Updates…");
        assert_eq!(UpdateState::NotFound.text(), "No Updates Available");
        assert_eq!(UpdateState::Error { message: "Network error".into() }.text(), "Network error");
        assert_eq!(UpdateState::UpdateAvailable(release("1.2.3")).text(), "Update Available: 1.2.3");
        assert_eq!(UpdateState::UpdateAvailable(release("")).text(), "Update Available");
        assert_eq!(UpdateState::Extracting { progress: 0.75 }.text(), "Preparing: 75%");
        assert_eq!(UpdateState::Installing { release: None }.text(), "Installing…");
        assert_eq!(UpdateState::Installing { release: Some(release("1.0.0")) }.text(), "Restart to Complete Update");
    }

    #[test]
    fn downloading_text() {
        let d = |expected_length, progress| UpdateState::Downloading { expected_length, progress }.text();
        assert_eq!(d(Some(1000), 500), "Downloading: 50%");
        assert_eq!(d(None, 500), "Downloading…");
        assert_eq!(d(Some(0), 500), "Downloading…");
    }

    #[test]
    fn max_width_text() {
        assert_eq!(UpdateState::Downloading { expected_length: Some(1000), progress: 50 }.max_width_text(), "Downloading: 100%");
        assert_eq!(UpdateState::Extracting { progress: 0.5 }.max_width_text(), "Preparing: 100%");
        assert_eq!(UpdateState::Checking.max_width_text(), UpdateState::Checking.text());
    }

    #[test]
    fn badge_and_description() {
        assert_eq!(UpdateState::UpdateAvailable(release("1.2.3")).badge(), Some("1.2.3".to_string()));
        assert_eq!(UpdateState::UpdateAvailable(release("")).badge(), None);
        assert_eq!(UpdateState::Downloading { expected_length: Some(200), progress: 50 }.badge(), Some("25%".to_string()));
        assert_eq!(UpdateState::Downloading { expected_length: None, progress: 50 }.badge(), None);
        assert_eq!(UpdateState::Extracting { progress: 0.5 }.badge(), Some("50%".to_string()));
        assert_eq!(UpdateState::Checking.badge(), None);
        assert_eq!(UpdateState::UpdateAvailable(release("1.2.3")).description(), "View Release Notes");
        assert_eq!(UpdateState::UpdateAvailable(release("nightly")).description(), "Download and install the latest version");
        assert_eq!(
            UpdateState::Installing { release: Some(release("1.2.3")) }.description(),
            "The update is ready. Version: 1.2.3"
        );
    }

    #[test]
    fn icons_and_tones() {
        assert_eq!(UpdateState::Idle.icon(), None);
        assert_eq!(UpdateState::Error { message: String::new() }.icon(), Some(UpdateIcon::Warning));
        assert_eq!(UpdateState::UpdateAvailable(release("1.0.0")).icon_tone(), Tone::Accent);
        assert_eq!(UpdateState::NotFound.background_tone(), Tone::BlueDarker);
        assert_eq!(UpdateState::Checking.background_tone(), Tone::Control);
        assert_eq!(UpdateState::Checking.foreground_tone(), Tone::Primary);
        assert_eq!(UpdateState::Error { message: String::new() }.background_tone(), Tone::WarningTint);
    }
}
