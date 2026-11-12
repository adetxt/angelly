//! The titlebar "Open With" button: the applications that can open the working
//! directory, and opening it in one of them.

use std::path::PathBuf;
#[cfg(target_os = "macos")]
use std::{path::Path, process::Command};
use std::sync::Arc;

use gpui_kit::*;

use crate::workspace::Workspace;

#[cfg(any(test, all(unix, not(target_os = "macos"))))]
mod desktop;
#[cfg(not(target_os = "macos"))]
mod other;

#[derive(Clone)]
pub struct ExternalApp {
    pub id: String,
    pub name: String,
    pub path: PathBuf,
    pub icon: Option<Arc<Image>>,
}

#[derive(Default)]
pub struct OpenWith {
    pub apps: Vec<ExternalApp>,
    loaded_for: Option<(PathBuf, String)>,
}

impl OpenWith {
    /// The app the button opens: the saved choice, else the first discovered one.
    pub fn selected<'a>(&'a self, id: &str, path: &str) -> Option<&'a ExternalApp> {
        self.apps
            .iter()
            .find(|app| app.path.to_string_lossy() == path)
            .or_else(|| self.apps.iter().find(|app| app.id == id))
            .or_else(|| self.apps.first())
    }
}

#[cfg(target_os = "macos")]
fn candidate_paths(directory: &Path, selected: &str) -> Vec<PathBuf> {
    use objc2_foundation::{NSString, NSURL};

    let workspace = objc2_app_kit::NSWorkspace::sharedWorkspace();
    let url = NSURL::fileURLWithPath(&NSString::from_str(&directory.to_string_lossy()));
    let to_path = |url: objc2::rc::Retained<NSURL>| url.path().map(|p| PathBuf::from(p.to_string()));
    let default = workspace.URLForApplicationToOpenURL(&url).and_then(to_path);
    let mut paths: Vec<PathBuf> = default.iter().cloned().collect();
    paths.extend(workspace.URLsForApplicationsToOpenURL(&url).into_iter().filter_map(to_path));
    if !selected.is_empty() {
        paths.insert(0, PathBuf::from(selected));
    }
    let mut seen = Vec::new();
    paths.retain(|p| {
        let new = !seen.contains(p);
        seen.push(p.clone());
        new
    });
    paths.sort_by_key(|p| (default.as_ref() != Some(p), app_name(p).to_lowercase()));
    paths
}

#[cfg(target_os = "macos")]
fn app_name(path: &Path) -> String {
    path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

#[cfg(target_os = "macos")]
fn plist_value(plist: &Path, key: &str) -> Option<String> {
    let output = Command::new("/usr/bin/plutil").args(["-extract", key, "raw", "-o", "-"]).arg(plist).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// The app's icon as a 32 px PNG, converted from its `.icns` with `sips`.
#[cfg(target_os = "macos")]
fn icon_of(app: &Path, cache: &Path) -> Option<Arc<Image>> {
    let plist = app.join("Contents/Info.plist");
    let mut file = plist_value(&plist, "CFBundleIconFile")?;
    if Path::new(&file).extension().is_none() {
        file.push_str(".icns");
    }
    let source = app.join("Contents/Resources").join(file);
    let target = cache.join(format!("{}.png", app_name(app)));
    std::fs::create_dir_all(cache).ok()?;
    let converted = Command::new("/usr/bin/sips")
        .args(["-s", "format", "png", "-Z", "32"])
        .arg(&source)
        .arg("--out")
        .arg(&target)
        .output()
        .ok()?;
    if !converted.status.success() {
        return None;
    }
    Some(Arc::new(Image::from_bytes(ImageFormat::Png, std::fs::read(target).ok()?)))
}

#[cfg(not(target_os = "macos"))]
use other::{application_at, discover, launch};

#[cfg(target_os = "macos")]
fn discover(directory: &Path, selected: &str) -> Vec<ExternalApp> {
    let cache = std::env::temp_dir().join("angelly-open-with");
    candidate_paths(directory, selected)
        .into_iter()
        .filter(|path| path.extension().is_some_and(|e| e.eq_ignore_ascii_case("app")) && path.exists())
        .map(|path| {
            let id = plist_value(&path.join("Contents/Info.plist"), "CFBundleIdentifier")
                .unwrap_or_else(|| path.to_string_lossy().into_owned());
            ExternalApp { id, name: app_name(&path), icon: icon_of(&path, &cache), path }
        })
        .collect()
}

/// Opens `directory` with `app`, returning the failure message when it cannot start.
#[cfg(target_os = "macos")]
fn launch(app: &ExternalApp, directory: &Path) -> Result<(), String> {
    match Command::new("/usr/bin/open").arg("-a").arg(&app.path).arg(directory).output() {
        Ok(output) if output.status.success() => Ok(()),
        Ok(output) => Err(String::from_utf8_lossy(&output.stderr).trim().to_string()),
        Err(error) => Err(error.to_string()),
    }
}

/// The application bundle at `path`, when it is one.
#[cfg(target_os = "macos")]
fn application_at(path: PathBuf) -> Option<ExternalApp> {
    if !path.extension().is_some_and(|e| e == "app") {
        return None;
    }
    let id = plist_value(&path.join("Contents/Info.plist"), "CFBundleIdentifier")
        .unwrap_or_else(|| path.to_string_lossy().into_owned());
    Some(ExternalApp { id, name: app_name(&path), path, icon: None })
}

impl Workspace {
    /// Rediscovers the applications for the selected tab's directory when it changed.
    pub fn reload_open_with(&mut self, cx: &mut Context<Self>) {
        let Some(directory) = self.titlebar_directory() else { return };
        let selected = crate::prefs::Prefs::read(cx).preferences().open_with_application().1.to_string();
        let key = (directory.clone(), selected.clone());
        if self.chrome.open_with.loaded_for.as_ref() == Some(&key) {
            return;
        }
        self.chrome.open_with.loaded_for = Some(key);
        cx.spawn(async move |this, cx| {
            let apps = cx.background_executor().spawn(async move { discover(&directory, &selected) }).await;
            this.update(cx, |this, cx| {
                this.chrome.open_with.apps = apps;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Opens the directory in `app`, remembering the choice. A failure is shown in an alert.
    pub fn open_in(&mut self, app: &ExternalApp, remember: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(directory) = self.titlebar_directory() else { return };
        if remember {
            crate::prefs::Prefs::update(cx, |prefs| prefs.set_open_with_application(&app.id, &app.path.to_string_lossy()));
            self.chrome.open_with.loaded_for = None;
        }
        if let Err(message) = launch(app, &directory) {
            self.ask(crate::panels::Confirm::notice("Couldn’t Open Folder", message), window, cx);
        }
        cx.notify();
    }

    /// "Other Application…": pick an application bundle and open the directory with it.
    pub fn choose_application(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose an application to open this folder.".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else { return };
            let Some(app) = paths.into_iter().next().and_then(application_at) else { return };
            this.update_in(cx, |this, window, cx| this.open_in(&app, true, window, cx)).ok();
        })
        .detach();
    }

    pub fn copy_directory_path(&mut self, cx: &mut Context<Self>) {
        if let Some(directory) = self.titlebar_directory() {
            cx.write_to_clipboard(ClipboardItem::new_string(directory.to_string_lossy().into_owned()));
        }
    }
}
