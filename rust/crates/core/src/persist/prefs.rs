//! App preferences: a small JSON object of typed values, stored in one file.
//!
//! Key names match the macOS app's user defaults keys.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use super::atomic::write_private_atomic;
use super::tab::HorizontalNewTabButtonPlacement;
use crate::config::Env;

pub const SHOW_AI_QUOTA_TRACKER_KEY: &str = "ShowAIQuotaTracker";
pub const WORKTREE_ROOT_KEY: &str = "WorktreeRoot";
pub const TAB_SIDEBAR_KEY: &str = "TabSidebar";
pub const TAB_SIDEBAR_WIDTH_KEY: &str = "TabSidebarWidth";
pub const OPEN_WITH_APPLICATION_KEY: &str = "TerminalOpenWithApplication";
pub const OPEN_WITH_APPLICATION_PATH_KEY: &str = "TerminalOpenWithApplicationPath";
pub const UPDATE_LAST_CHECK_KEY: &str = "UpdateLastCheck";
pub const UPDATE_AUTOMATIC_CHECKS_KEY: &str = "UpdateAutomaticChecks";

pub const DEFAULT_TAB_SIDEBAR_WIDTH: f64 = 220.0;
pub const TAB_SIDEBAR_WIDTH_RANGE: (f64, f64) = (160.0, 480.0);

pub const FILE_NAME: &str = "preferences.json";

/// Where the app keeps its own data: Application Support on macOS, and
/// `~/.local/share/<app id>` elsewhere.
pub fn data_dir(env: &Env) -> Option<PathBuf> {
    env.data_dir()
}

/// The default parent folder for new worktrees, `~/worktrees`.
pub fn default_worktree_root(env: &Env) -> Option<PathBuf> {
    env.home.as_ref().map(|home| home.join("worktrees"))
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Preferences {
    values: Map<String, Value>,
}

impl Preferences {
    /// Reads preferences from `path`. A missing file gives defaults; a file that is not a
    /// JSON object is an error so it is not silently overwritten.
    pub fn load(path: &Path) -> io::Result<Self> {
        let data = match fs::read(path) {
            Ok(data) => data,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(error) => return Err(error),
        };
        match serde_json::from_slice(&data) {
            Ok(Value::Object(values)) => Ok(Self { values }),
            _ => Err(io::Error::new(io::ErrorKind::InvalidData, "preferences file is not a JSON object")),
        }
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        let data = serde_json::to_vec_pretty(&self.values)?;
        write_private_atomic(path, &data)
    }

    fn bool(&self, key: &str, default: bool) -> bool {
        self.values.get(key).and_then(Value::as_bool).unwrap_or(default)
    }

    fn string(&self, key: &str) -> Option<&str> {
        self.values.get(key).and_then(Value::as_str)
    }

    pub fn horizontal_new_tab_button_placement(&self) -> HorizontalNewTabButtonPlacement {
        self.string(HorizontalNewTabButtonPlacement::DEFAULTS_KEY)
            .and_then(HorizontalNewTabButtonPlacement::from_raw_value)
            .unwrap_or_default()
    }

    pub fn set_horizontal_new_tab_button_placement(&mut self, placement: HorizontalNewTabButtonPlacement) {
        self.values.insert(
            HorizontalNewTabButtonPlacement::DEFAULTS_KEY.into(),
            placement.raw_value().into(),
        );
    }

    /// Whether the AI quota tracker shows in the sidebar footer. On by default.
    pub fn shows_ai_quota_tracker(&self) -> bool {
        self.bool(SHOW_AI_QUOTA_TRACKER_KEY, true)
    }

    pub fn set_shows_ai_quota_tracker(&mut self, shows: bool) {
        self.values.insert(SHOW_AI_QUOTA_TRACKER_KEY.into(), shows.into());
    }

    /// The parent folder for new worktrees, or `default` when none was chosen.
    pub fn worktree_root(&self, default: &Path) -> PathBuf {
        self.string(WORKTREE_ROOT_KEY).map_or_else(|| default.to_path_buf(), PathBuf::from)
    }

    pub fn set_worktree_root(&mut self, root: &Path) {
        self.values.insert(WORKTREE_ROOT_KEY.into(), root.to_string_lossy().into_owned().into());
    }

    pub fn reset_worktree_root(&mut self) {
        self.values.remove(WORKTREE_ROOT_KEY);
    }

    pub fn tab_sidebar_visible(&self) -> bool {
        self.bool(TAB_SIDEBAR_KEY, true)
    }

    pub fn set_tab_sidebar_visible(&mut self, visible: bool) {
        self.values.insert(TAB_SIDEBAR_KEY.into(), visible.into());
    }

    /// The sidebar width, clamped to the allowed range.
    pub fn tab_sidebar_width(&self) -> f64 {
        self.values
            .get(TAB_SIDEBAR_WIDTH_KEY)
            .and_then(Value::as_f64)
            .map_or(DEFAULT_TAB_SIDEBAR_WIDTH, clamp_sidebar_width)
    }

    pub fn set_tab_sidebar_width(&mut self, width: f64) {
        self.values
            .insert(TAB_SIDEBAR_WIDTH_KEY.into(), clamp_sidebar_width(width).into());
    }

    /// The identifier and path of the application the "open with" button launches.
    pub fn open_with_application(&self) -> (&str, &str) {
        (
            self.string(OPEN_WITH_APPLICATION_KEY).unwrap_or_default(),
            self.string(OPEN_WITH_APPLICATION_PATH_KEY).unwrap_or_default(),
        )
    }

    /// When the last automatic update check ran, in seconds since the Unix epoch.
    pub fn update_last_check(&self) -> Option<f64> {
        self.values.get(UPDATE_LAST_CHECK_KEY).and_then(Value::as_f64)
    }

    pub fn set_update_last_check(&mut self, seconds: f64) {
        self.values.insert(UPDATE_LAST_CHECK_KEY.into(), seconds.into());
    }

    /// The answer to the "Enable Automatic Updates?" prompt, when it was answered.
    pub fn update_automatic_checks(&self) -> Option<bool> {
        self.values.get(UPDATE_AUTOMATIC_CHECKS_KEY).and_then(Value::as_bool)
    }

    pub fn set_update_automatic_checks(&mut self, allowed: bool) {
        self.values.insert(UPDATE_AUTOMATIC_CHECKS_KEY.into(), allowed.into());
    }

    pub fn set_open_with_application(&mut self, id: &str, path: &str) {
        self.values.insert(OPEN_WITH_APPLICATION_KEY.into(), id.into());
        self.values.insert(OPEN_WITH_APPLICATION_PATH_KEY.into(), path.into());
    }
}

pub fn clamp_sidebar_width(width: f64) -> f64 {
    let (min, max) = TAB_SIDEBAR_WIDTH_RANGE;
    if width.is_nan() {
        DEFAULT_TAB_SIDEBAR_WIDTH
    } else {
        width.clamp(min, max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Platform;

    #[test]
    fn defaults() {
        let prefs = Preferences::default();
        assert_eq!(prefs.horizontal_new_tab_button_placement(), HorizontalNewTabButtonPlacement::Tabbar);
        assert!(prefs.shows_ai_quota_tracker());
        assert!(prefs.tab_sidebar_visible());
        assert_eq!(prefs.tab_sidebar_width(), 220.0);
        assert_eq!(prefs.worktree_root(Path::new("/home/u/worktrees")), Path::new("/home/u/worktrees"));
        assert_eq!(prefs.open_with_application(), ("", ""));
    }

    #[test]
    fn values_round_trip_through_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config").join(FILE_NAME);
        let mut prefs = Preferences::default();
        prefs.set_horizontal_new_tab_button_placement(HorizontalNewTabButtonPlacement::Titlebar);
        prefs.set_shows_ai_quota_tracker(false);
        prefs.set_tab_sidebar_visible(false);
        prefs.set_tab_sidebar_width(300.0);
        prefs.set_worktree_root(Path::new("/work/trees"));
        prefs.set_open_with_application("com.apple.dt.Xcode", "/Applications/Xcode.app");
        prefs.save(&path).unwrap();

        let loaded = Preferences::load(&path).unwrap();
        assert_eq!(loaded, prefs);
        assert_eq!(loaded.horizontal_new_tab_button_placement(), HorizontalNewTabButtonPlacement::Titlebar);
        assert!(!loaded.shows_ai_quota_tracker());
        assert!(!loaded.tab_sidebar_visible());
        assert_eq!(loaded.tab_sidebar_width(), 300.0);
        assert_eq!(loaded.worktree_root(Path::new("/default")), Path::new("/work/trees"));
        assert_eq!(
            loaded.open_with_application(),
            ("com.apple.dt.Xcode", "/Applications/Xcode.app")
        );
    }

    #[test]
    fn the_sidebar_width_is_clamped() {
        let mut prefs = Preferences::default();
        prefs.set_tab_sidebar_width(10.0);
        assert_eq!(prefs.tab_sidebar_width(), 160.0);
        prefs.set_tab_sidebar_width(10_000.0);
        assert_eq!(prefs.tab_sidebar_width(), 480.0);

        let stored: Preferences = Preferences {
            values: serde_json::from_str(r#"{"TabSidebarWidth": 9999}"#).unwrap(),
        };
        assert_eq!(stored.tab_sidebar_width(), 480.0);
    }

    #[test]
    fn resetting_the_worktree_root_restores_the_default() {
        let mut prefs = Preferences::default();
        prefs.set_worktree_root(Path::new("/elsewhere"));
        prefs.reset_worktree_root();
        assert_eq!(prefs.worktree_root(Path::new("/default")), Path::new("/default"));
    }

    #[test]
    fn unrecognized_values_fall_back_to_defaults() {
        let prefs = Preferences {
            values: serde_json::from_str(r#"{"HorizontalNewTabButtonPlacement": "nowhere", "TabSidebar": "yes"}"#)
                .unwrap(),
        };
        assert_eq!(prefs.horizontal_new_tab_button_placement(), HorizontalNewTabButtonPlacement::Tabbar);
        assert!(prefs.tab_sidebar_visible());
    }

    #[test]
    fn a_missing_file_gives_defaults_and_a_bad_file_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);
        assert_eq!(Preferences::load(&path).unwrap(), Preferences::default());
        fs::write(&path, "[1, 2]").unwrap();
        assert!(Preferences::load(&path).is_err());
    }

    #[test]
    fn data_dir_per_platform() {
        let env = |platform| Env {
            platform,
            home: Some(PathBuf::from("/home/u")),
            xdg_config_home: None,
            resources_dir: None,
        };
        assert_eq!(
            data_dir(&env(Platform::MacOs)).unwrap(),
            Path::new("/home/u/Library/Application Support/com.adetxt.angelly")
        );
        assert_eq!(
            data_dir(&env(Platform::Linux)).unwrap(),
            Path::new("/home/u/.local/share/com.adetxt.angelly")
        );
        assert_eq!(default_worktree_root(&env(Platform::Linux)).unwrap(), Path::new("/home/u/worktrees"));
    }

    #[test]
    fn update_check_state_round_trips() {
        let mut prefs = Preferences::default();
        assert_eq!((prefs.update_last_check(), prefs.update_automatic_checks()), (None, None));
        prefs.set_update_last_check(1_800_000_000.5);
        prefs.set_update_automatic_checks(false);
        let dir = std::env::temp_dir().join(format!("angelly-prefs-{}", std::process::id()));
        let path = dir.join("preferences.json");
        std::fs::create_dir_all(&dir).unwrap();
        prefs.save(&path).unwrap();
        let loaded = Preferences::load(&path).unwrap();
        assert_eq!((loaded.update_last_check(), loaded.update_automatic_checks()), (Some(1_800_000_000.5), Some(false)));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
