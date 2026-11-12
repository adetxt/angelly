use std::path::{Path, PathBuf};

/// Bundle identifier used for the macOS Application Support directory.
pub const APP_ID: &str = "com.adetxt.angelly";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    MacOs,
    Linux,
    Windows,
}

impl Platform {
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::MacOs
        } else if cfg!(windows) {
            Self::Windows
        } else {
            Self::Linux
        }
    }
}

/// The environment config discovery depends on, injectable for tests.
#[derive(Debug, Clone)]
pub struct Env {
    pub platform: Platform,
    pub home: Option<PathBuf>,
    pub xdg_config_home: Option<PathBuf>,
    pub resources_dir: Option<PathBuf>,
}

impl Env {
    pub fn from_process() -> Self {
        let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from);
        let windows = cfg!(windows);
        Self {
            platform: Platform::current(),
            home: var("HOME").or_else(|| var("USERPROFILE").filter(|_| windows)),
            // Windows has no XDG; `%APPDATA%` plays its role.
            xdg_config_home: var("XDG_CONFIG_HOME").or_else(|| var("APPDATA").filter(|_| windows)),
            resources_dir: var("GHOSTTY_RESOURCES_DIR"),
        }
    }

    /// `$XDG_CONFIG_HOME` when set and absolute, otherwise `~/.config`.
    pub fn xdg_config_dir(&self) -> Option<PathBuf> {
        match &self.xdg_config_home {
            Some(p) if p.is_absolute() => Some(p.clone()),
            _ => self.home.as_ref().map(|h| h.join(".config")),
        }
    }

    pub fn xdg_ghostty_dir(&self) -> Option<PathBuf> {
        self.xdg_config_dir().map(|d| d.join("ghostty"))
    }

    pub fn app_support_dir(&self) -> Option<PathBuf> {
        if self.platform != Platform::MacOs {
            return None;
        }
        self.home
            .as_ref()
            .map(|h| h.join("Library").join("Application Support").join(APP_ID))
    }

    pub fn user_themes_dir(&self) -> Option<PathBuf> {
        self.xdg_ghostty_dir().map(|d| d.join("themes"))
    }

    pub fn resource_themes_dir(&self) -> Option<PathBuf> {
        self.resources_dir.as_ref().map(|d| d.join("themes"))
    }

    pub fn expand_home(&self, path: &str) -> PathBuf {
        match (path.strip_prefix("~/"), &self.home) {
            (Some(rest), Some(home)) => home.join(rest),
            _ => PathBuf::from(path),
        }
    }

    /// Where per-user app data lives: `Application Support` on macOS,
    /// `%LOCALAPPDATA%` on Windows, `~/.local/share/<app id>` elsewhere.
    pub fn data_dir(&self) -> Option<PathBuf> {
        let home = self.home.as_ref()?;
        match self.platform {
            Platform::MacOs => self.app_support_dir(),
            Platform::Windows => Some(home.join("AppData").join("Local").join(APP_ID)),
            Platform::Linux => Some(home.join(".local/share").join(APP_ID)),
        }
    }

    /// Default config files in the order Ghostty loads them; later files override
    /// earlier ones. Legacy `config` names load before their `config.ghostty` successors.
    pub fn default_config_paths(&self) -> Vec<PathBuf> {
        let mut paths = Vec::new();
        if let Some(dir) = self.xdg_ghostty_dir() {
            paths.push(dir.join("config"));
            paths.push(dir.join("config.ghostty"));
        }
        if let Some(dir) = self.app_support_dir() {
            paths.push(dir.join("config"));
            paths.push(dir.join("config.ghostty"));
        }
        paths
    }

    /// The file a settings screen should read and write: on macOS the Application
    /// Support file if it exists, else an existing XDG file, else the Application
    /// Support location; elsewhere the XDG file, preferring an existing legacy one.
    pub fn preferred_config_path(&self) -> Option<PathBuf> {
        let first_existing = |dir: Option<PathBuf>| -> Option<PathBuf> {
            let dir = dir?;
            let new = dir.join("config.ghostty");
            let legacy = dir.join("config");
            if is_file(&new) {
                Some(new)
            } else if is_file(&legacy) {
                Some(legacy)
            } else {
                None
            }
        };
        match self.platform {
            Platform::MacOs => first_existing(self.app_support_dir())
                .or_else(|| first_existing(self.xdg_ghostty_dir()))
                .or_else(|| self.app_support_dir().map(|d| d.join("config.ghostty"))),
            Platform::Linux | Platform::Windows => first_existing(self.xdg_ghostty_dir())
                .or_else(|| self.xdg_ghostty_dir().map(|d| d.join("config.ghostty"))),
        }
    }
}

fn is_file(path: &Path) -> bool {
    std::fs::metadata(path).map(|m| m.is_file()).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(platform: Platform, home: &Path) -> Env {
        Env {
            platform,
            home: Some(home.to_path_buf()),
            xdg_config_home: None,
            resources_dir: None,
        }
    }

    #[test]
    fn xdg_honours_env_only_when_absolute() {
        let mut e = env(Platform::Linux, Path::new("/home/a"));
        assert_eq!(e.xdg_config_dir(), Some(PathBuf::from("/home/a/.config")));
        e.xdg_config_home = Some(PathBuf::from("/cfg"));
        assert_eq!(e.xdg_config_dir(), Some(PathBuf::from("/cfg")));
        e.xdg_config_home = Some(PathBuf::from("relative"));
        assert_eq!(e.xdg_config_dir(), Some(PathBuf::from("/home/a/.config")));
    }

    #[test]
    fn load_order_per_platform() {
        let mac = env(Platform::MacOs, Path::new("/Users/a")).default_config_paths();
        assert_eq!(
            mac,
            vec![
                PathBuf::from("/Users/a/.config/ghostty/config"),
                PathBuf::from("/Users/a/.config/ghostty/config.ghostty"),
                PathBuf::from("/Users/a/Library/Application Support/com.adetxt.angelly/config"),
                PathBuf::from("/Users/a/Library/Application Support/com.adetxt.angelly/config.ghostty"),
            ]
        );
        let linux = env(Platform::Linux, Path::new("/home/a")).default_config_paths();
        assert_eq!(linux.len(), 2);
    }

    #[test]
    fn preferred_path_follows_existing_files() {
        let dir = tempfile::tempdir().unwrap();
        let e = env(Platform::MacOs, dir.path());
        let app = e.app_support_dir().unwrap().join("config.ghostty");
        assert_eq!(e.preferred_config_path(), Some(app.clone()));

        let xdg = dir.path().join(".config/ghostty/config.ghostty");
        std::fs::create_dir_all(xdg.parent().unwrap()).unwrap();
        std::fs::write(&xdg, "").unwrap();
        assert_eq!(e.preferred_config_path(), Some(xdg));

        std::fs::create_dir_all(app.parent().unwrap()).unwrap();
        std::fs::write(&app, "").unwrap();
        assert_eq!(e.preferred_config_path(), Some(app));
    }

    #[test]
    fn expands_home_prefix() {
        let e = env(Platform::Linux, Path::new("/home/a"));
        assert_eq!(e.expand_home("~/x/y"), PathBuf::from("/home/a/x/y"));
        assert_eq!(e.expand_home("/abs"), PathBuf::from("/abs"));
        assert_eq!(e.expand_home("~x"), PathBuf::from("~x"));
    }
}
