//! "Open With" on Linux and Windows: the applications that can open a folder, and the
//! commands that launch them.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::ExternalApp;

/// Starts `command` detached from the terminal and reaps it in the background.
fn spawn(mut command: Command, directory: &Path) -> Result<(), String> {
    let mut child = command
        .current_dir(directory)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| error.to_string())?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(unix)]
pub use unix::{application_at, discover, launch};

#[cfg(windows)]
pub use windows::{application_at, discover, launch};

#[cfg(unix)]
mod unix {
    use std::sync::Arc;

    use super::*;
    use crate::open_with::desktop::{self, DesktopEntry};
    use gpui_kit::{Image, ImageFormat};

    fn env_path(name: &str) -> Option<PathBuf> {
        std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from)
    }

    fn data_dirs() -> Vec<PathBuf> {
        let data_dirs = std::env::var("XDG_DATA_DIRS").ok();
        desktop::application_dirs(env_path("XDG_DATA_HOME").as_deref(), env_path("HOME").as_deref(), data_dirs.as_deref())
    }

    /// The desktop file id the user's `mimeapps.list` files choose for folders.
    fn default_handler(applications: &[PathBuf]) -> Option<String> {
        let mut files = Vec::new();
        if let Some(config) = env_path("XDG_CONFIG_HOME").or_else(|| env_path("HOME").map(|home| home.join(".config"))) {
            files.push(config.join("mimeapps.list"));
        }
        files.extend(applications.iter().map(|dir| dir.join("mimeapps.list")));
        files.iter().find_map(|file| {
            let text = std::fs::read_to_string(file).ok()?;
            desktop::default_for(desktop::DIRECTORY_MIME, &text)
        })
    }

    /// The icon file for an `Icon=` value: an absolute path, else the best hicolor match.
    fn icon_file(name: &str, applications: &[PathBuf]) -> Option<PathBuf> {
        let path = Path::new(name);
        if path.is_absolute() {
            return path.is_file().then(|| path.to_owned());
        }
        for base in applications.iter().filter_map(|dir| dir.parent()) {
            for size in ["48x48", "64x64", "32x32", "128x128", "256x256", "scalable"] {
                for extension in ["png", "svg"] {
                    let candidate = base.join("icons/hicolor").join(size).join("apps").join(format!("{name}.{extension}"));
                    if candidate.is_file() {
                        return Some(candidate);
                    }
                }
            }
            for extension in ["png", "svg"] {
                let candidate = base.join("pixmaps").join(format!("{name}.{extension}"));
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
        None
    }

    fn icon_of(name: &str, applications: &[PathBuf]) -> Option<Arc<Image>> {
        let file = icon_file(name, applications)?;
        let format = match file.extension()?.to_str()? {
            "png" => ImageFormat::Png,
            "svg" => ImageFormat::Svg,
            _ => return None,
        };
        Some(Arc::new(Image::from_bytes(format, std::fs::read(file).ok()?)))
    }

    fn external(entry: &DesktopEntry, applications: &[PathBuf]) -> ExternalApp {
        ExternalApp {
            id: entry.id.clone(),
            name: entry.name.clone(),
            path: entry.path.clone(),
            icon: entry.icon.as_deref().and_then(|icon| icon_of(icon, applications)),
        }
    }

    pub fn discover(_directory: &Path, selected: &str) -> Vec<ExternalApp> {
        let applications = data_dirs();
        let desktops: Vec<String> = std::env::var("XDG_CURRENT_DESKTOP")
            .map(|value| value.split(':').map(str::to_owned).collect())
            .unwrap_or_default();
        let default = default_handler(&applications);
        let handlers = desktop::directory_handlers(desktop::load_entries(&applications), &desktops, default.as_deref());
        let mut apps: Vec<ExternalApp> = handlers.iter().map(|entry| external(entry, &applications)).collect();
        if !selected.is_empty() && !apps.iter().any(|app| app.path == Path::new(selected)) {
            if let Some(app) = application_at(PathBuf::from(selected)) {
                apps.insert(0, app);
            }
        }
        apps
    }

    /// A desktop file or an executable chosen with the file picker.
    pub fn application_at(path: PathBuf) -> Option<ExternalApp> {
        if path.extension().is_some_and(|e| e == "desktop") {
            let id = path.file_name()?.to_string_lossy().into_owned();
            let entry = DesktopEntry::parse(&id, &path, &std::fs::read_to_string(&path).ok()?)?;
            return Some(external(&entry, &data_dirs()));
        }
        if !path.is_file() {
            return None;
        }
        let name = path.file_stem()?.to_string_lossy().into_owned();
        Some(ExternalApp { id: path.to_string_lossy().into_owned(), name, path, icon: None })
    }

    pub fn launch(app: &ExternalApp, directory: &Path) -> Result<(), String> {
        let argv = if app.path.extension().is_some_and(|e| e == "desktop") {
            let text = std::fs::read_to_string(&app.path).map_err(|error| error.to_string())?;
            DesktopEntry::parse(&app.id, &app.path, &text)
                .and_then(|entry| entry.command(directory))
                .ok_or_else(|| format!("{} has no usable Exec line.", app.name))?
        } else {
            vec![app.path.to_string_lossy().into_owned(), directory.to_string_lossy().into_owned()]
        };
        let mut command = Command::new(&argv[0]);
        command.args(&argv[1..]);
        spawn(command, directory)
    }
}

#[cfg(windows)]
mod windows {
    use std::os::windows::process::CommandExt as _;

    use super::*;

    /// Explorer plus the folder verbs other applications register, such as "Open with Code".
    pub fn discover(_directory: &Path, selected: &str) -> Vec<ExternalApp> {
        let mut apps = vec![ExternalApp {
            id: "explorer".into(),
            name: "File Explorer".into(),
            path: PathBuf::from("explorer.exe"),
            icon: None,
        }];
        apps.extend(registered_verbs());
        if !selected.is_empty() && !apps.iter().any(|app| app.path == Path::new(selected)) {
            if let Some(app) = application_at(PathBuf::from(selected)) {
                apps.insert(0, app);
            }
        }
        apps
    }

    fn registered_verbs() -> Vec<ExternalApp> {
        let Ok(output) = Command::new("reg").args(["query", r"HKEY_CLASSES_ROOT\Directory\shell", "/s", "/ve"]).output() else {
            return Vec::new();
        };
        let text = String::from_utf8_lossy(&output.stdout);
        let mut apps: Vec<ExternalApp> = Vec::new();
        let mut key = "";
        for line in text.lines() {
            if line.starts_with("HKEY_") {
                key = line.trim();
                continue;
            }
            let Some((_, value)) = line.trim().strip_prefix("(Default)").and_then(|rest| rest.trim_start().split_once(char::is_whitespace)) else {
                continue;
            };
            let value = value.trim();
            let parts: Vec<&str> = key.rsplit('\\').collect();
            match parts.as_slice() {
                ["command", verb, ..] => {
                    if let Some(exe) = executable_of(value).filter(|exe| !is_shell_builtin(exe)) {
                        if let Some(app) = apps.iter_mut().find(|app| app.id == *verb) {
                            app.path = exe;
                        }
                    }
                }
                [verb, "shell", ..] => apps.push(ExternalApp {
                    id: (*verb).to_owned(),
                    name: value.replace('&', "").trim_start_matches("Open with ").trim().to_owned(),
                    path: PathBuf::new(),
                    icon: None,
                }),
                _ => {}
            }
        }
        apps.retain(|app| !app.path.as_os_str().is_empty() && !app.name.is_empty());
        apps
    }

    fn is_shell_builtin(exe: &Path) -> bool {
        exe.file_name().is_some_and(|name| ["cmd.exe", "powershell.exe", "pwsh.exe"].iter().any(|b| name.eq_ignore_ascii_case(b)))
    }

    /// The program of a registry command template, `"C:\...\Code.exe" "%V"`.
    fn executable_of(command: &str) -> Option<PathBuf> {
        let command = command.trim();
        let exe = match command.strip_prefix('"') {
            Some(rest) => rest.split('"').next()?,
            None => command.split_whitespace().next()?,
        };
        let expanded = expand_environment(exe);
        Some(PathBuf::from(expanded))
    }

    fn expand_environment(text: &str) -> String {
        let mut out = String::new();
        let mut rest = text;
        while let Some(start) = rest.find('%') {
            out.push_str(&rest[..start]);
            let after = &rest[start + 1..];
            match after.find('%') {
                Some(end) => {
                    let name = &after[..end];
                    out.push_str(&std::env::var(name).unwrap_or_else(|_| format!("%{name}%")));
                    rest = &after[end + 1..];
                }
                None => {
                    out.push('%');
                    rest = after;
                }
            }
        }
        out.push_str(rest);
        out
    }

    pub fn application_at(path: PathBuf) -> Option<ExternalApp> {
        if !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe")) || !path.is_file() {
            return None;
        }
        let name = path.file_stem()?.to_string_lossy().into_owned();
        Some(ExternalApp { id: path.to_string_lossy().into_owned(), name, path, icon: None })
    }

    pub fn launch(app: &ExternalApp, directory: &Path) -> Result<(), String> {
        let mut command = Command::new(&app.path);
        command.raw_arg(format!("\"{}\"", directory.display()));
        spawn(command, directory)
    }
}
