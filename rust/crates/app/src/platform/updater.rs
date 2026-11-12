//! Installs a downloaded update on macOS: mounts the disk image, checks the app inside,
//! stages it beside the running bundle and swaps it in once the app has exited.
//!
//! The package has already been checked against its published SHA-256 by the update
//! controller. Staging adds two checks of its own: the app inside must have the running
//! app's bundle identifier, and its code signature must verify.

use std::fs;
use std::io;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use angelly_core::update::UpdateController;

use crate::panels::UpdateInstaller;

const OPEN: &str = "/usr/bin/open";

/// Waits for the old app to exit, swaps the staged bundle in (restoring the old one if the
/// move fails) and opens the result. Arguments: pid, bundle, staged, backup, launcher.
const SWAP_SCRIPT: &str = r#"
while kill -0 "$1" 2>/dev/null; do sleep 0.1; done
rm -rf "$4"
if mv "$2" "$4"; then
    if mv "$3" "$2"; then rm -rf "$4"; else mv "$4" "$2"; fi
fi
"$5" "$2"
"#;

pub struct Installer {
    bundle: PathBuf,
    quit: Box<dyn Fn() + Send + Sync>,
    staged: Mutex<Option<PathBuf>>,
}

impl Installer {
    /// An installer for the running `bundle`. `quit` ends the app so the swap can happen.
    pub fn new(bundle: PathBuf, quit: impl Fn() + Send + Sync + 'static) -> Self {
        Self { bundle, quit: Box::new(quit), staged: Mutex::new(None) }
    }

    /// The bundle containing the running executable, when it runs from one.
    pub fn running_bundle() -> Option<PathBuf> {
        let executable = std::env::current_exe().ok()?.canonicalize().ok()?;
        executable.ancestors().find(|path| path.extension().is_some_and(|ext| ext == "app")).map(Path::to_path_buf)
    }
}

impl UpdateInstaller for Installer {
    fn install(&self, package: PathBuf, controller: UpdateController) {
        match stage(&package, &self.bundle, &|progress| controller.report_extraction(progress)) {
            Ok(staged) => {
                *self.staged.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(staged);
                controller.report_installing(None);
                self.relaunch();
            }
            Err(message) => controller.report_error(message),
        }
    }

    fn relaunch(&self) {
        let staged = self.staged.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).take();
        let Some(staged) = staged else { return };
        match spawn_swap(std::process::id(), &self.bundle, &staged, OPEN) {
            Ok(()) => (self.quit)(),
            Err(error) => eprintln!("angelly: could not start the update swap: {error}"),
        }
    }
}

/// A mounted disk image, detached when dropped.
struct Mount {
    point: PathBuf,
}

impl Mount {
    fn attach(image: &Path) -> Result<Self, String> {
        static MOUNTS: AtomicUsize = AtomicUsize::new(0);
        let point = std::env::temp_dir()
            .join(format!("angelly-update-{}-{}", std::process::id(), MOUNTS.fetch_add(1, Ordering::Relaxed)));
        fs::create_dir_all(&point).map_err(|error| format!("Couldn't prepare the update: {error}"))?;
        let mount = Self { point };
        run("/usr/bin/hdiutil", [
            "attach".as_ref(),
            "-nobrowse".as_ref(),
            "-readonly".as_ref(),
            "-noautoopen".as_ref(),
            "-mountpoint".as_ref(),
            mount.point.as_os_str(),
            image.as_os_str(),
        ])
        .map_err(|error| format!("Couldn't open the update package: {error}"))?;
        Ok(mount)
    }
}

impl Drop for Mount {
    fn drop(&mut self) {
        let _ = Command::new("/usr/bin/hdiutil")
            .args(["detach", "-force"])
            .arg(&self.point)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let _ = fs::remove_dir(&self.point);
    }
}

fn run<'a>(program: &str, args: impl IntoIterator<Item = &'a std::ffi::OsStr>) -> Result<String, String> {
    let output = Command::new(program).args(args).output().map_err(|error| error.to_string())?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    } else {
        let message = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        Err(if message.is_empty() { format!("{program} failed") } else { message })
    }
}

fn bundle_identifier(app: &Path) -> Result<String, String> {
    let plist = app.join("Contents/Info.plist");
    run("/usr/bin/plutil", ["-extract".as_ref(), "CFBundleIdentifier".as_ref(), "raw".as_ref(), "-o".as_ref(), "-".as_ref(), plist.as_os_str()])
        .map_err(|_| "The update does not contain a valid app.".to_owned())
}

fn app_in(directory: &Path) -> Result<PathBuf, String> {
    fs::read_dir(directory)
        .map_err(|error| error.to_string())?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| path.extension().is_some_and(|ext| ext == "app"))
        .ok_or_else(|| "The update does not contain an app.".to_owned())
}

/// Copies the app out of `package` next to `bundle` and verifies it. `progress` receives
/// values between 0 and 1.
fn stage(package: &Path, bundle: &Path, progress: &dyn Fn(f64)) -> Result<PathBuf, String> {
    let parent = bundle.parent().ok_or("The running app has no folder.")?;
    let mount = Mount::attach(package)?;
    progress(0.2);
    let source = app_in(&mount.point)?;
    if bundle_identifier(&source)? != bundle_identifier(bundle)? {
        return Err("The update is for a different app.".into());
    }
    progress(0.4);
    let staged = parent.join(format!(".{}.update", bundle.file_name().unwrap_or_default().to_string_lossy()));
    let _ = fs::remove_dir_all(&staged);
    run("/usr/bin/ditto", [source.as_os_str(), staged.as_os_str()]).map_err(|error| {
        format!("Angelly can't write to {}: {error}", parent.display())
    })?;
    progress(0.8);
    let verified = run("/usr/bin/codesign", ["--verify".as_ref(), "--deep".as_ref(), "--strict".as_ref(), staged.as_os_str()]);
    if let Err(error) = verified {
        let _ = fs::remove_dir_all(&staged);
        return Err(format!("The update's signature is not valid: {error}"));
    }
    let _ = run("/usr/bin/xattr", ["-dr".as_ref(), "com.apple.quarantine".as_ref(), staged.as_os_str()]);
    progress(1.0);
    Ok(staged)
}

/// Starts the detached helper that swaps `staged` in for `bundle` after process `pid` exits.
fn spawn_swap(pid: u32, bundle: &Path, staged: &Path, launcher: &str) -> io::Result<()> {
    let backup = staged.with_extension("previous");
    Command::new("/bin/sh")
        .arg("-c")
        .arg(SWAP_SCRIPT)
        .arg("angelly-update")
        .arg(pid.to_string())
        .args([bundle, staged, &backup])
        .arg(launcher)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map(drop)
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::time::{Duration, Instant};

    use super::*;

    /// Attaching images in parallel makes hdiutil fail with "resource temporarily unavailable".
    static DISK_IMAGES: Mutex<()> = Mutex::new(());

    /// Writes a minimal app bundle, signed ad hoc, whose executable prints `marker`.
    fn fixture_app(directory: &Path, identifier: &str, marker: &str) -> PathBuf {
        let app = directory.join("Angelly.app");
        let macos = app.join("Contents/MacOS");
        fs::create_dir_all(&macos).unwrap();
        fs::write(
            app.join("Contents/Info.plist"),
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?><plist version=\"1.0\"><dict>\
                 <key>CFBundleIdentifier</key><string>{identifier}</string>\
                 <key>CFBundleExecutable</key><string>angelly</string>\
                 <key>CFBundlePackageType</key><string>APPL</string></dict></plist>"
            ),
        )
        .unwrap();
        fs::write(macos.join("angelly"), format!("#!/bin/sh\necho {marker}\n")).unwrap();
        run("/bin/chmod", ["+x".as_ref(), macos.join("angelly").as_os_str()]).unwrap();
        run("/usr/bin/codesign", ["--force".as_ref(), "--sign".as_ref(), "-".as_ref(), app.as_os_str()]).unwrap();
        app
    }

    fn disk_image(directory: &Path, app_directory: &Path) -> PathBuf {
        let image = directory.join("Angelly.dmg");
        run("/usr/bin/hdiutil", [
            "create".as_ref(),
            "-quiet".as_ref(),
            "-srcfolder".as_ref(),
            app_directory.as_os_str(),
            "-volname".as_ref(),
            "Angelly".as_ref(),
            "-format".as_ref(),
            "UDZO".as_ref(),
            image.as_os_str(),
        ])
        .unwrap();
        image
    }

    fn marker(app: &Path) -> String {
        fs::read_to_string(app.join("Contents/MacOS/angelly")).unwrap()
    }

    #[test]
    fn a_matching_signed_update_is_staged_beside_the_running_app() {
        let _images = DISK_IMAGES.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let root = tempfile::tempdir().unwrap();
        let running = root.path().join("installed");
        let incoming = root.path().join("incoming");
        fs::create_dir_all(&running).unwrap();
        fs::create_dir_all(&incoming).unwrap();
        let bundle = fixture_app(&running, "com.example.angelly", "old");
        fixture_app(&incoming, "com.example.angelly", "new");
        let image = disk_image(root.path(), &incoming);

        let seen = RefCell::new(Vec::new());
        let staged = stage(&image, &bundle, &|progress| seen.borrow_mut().push(progress)).unwrap();

        assert_eq!(staged.parent(), bundle.parent());
        assert!(marker(&staged).contains("new"));
        assert_eq!(seen.into_inner().last(), Some(&1.0));
        assert!(marker(&bundle).contains("old"));
    }

    #[test]
    fn an_update_for_another_app_is_refused() {
        let _images = DISK_IMAGES.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let root = tempfile::tempdir().unwrap();
        let running = root.path().join("installed");
        let incoming = root.path().join("incoming");
        fs::create_dir_all(&running).unwrap();
        fs::create_dir_all(&incoming).unwrap();
        let bundle = fixture_app(&running, "com.example.angelly", "old");
        fixture_app(&incoming, "com.example.other", "new");
        let image = disk_image(root.path(), &incoming);

        let error = stage(&image, &bundle, &|_| {}).unwrap_err();
        assert!(error.contains("different app"), "{error}");
        assert!(!running.join(".Angelly.app.update").exists());
    }

    #[test]
    fn an_update_that_fails_signature_verification_is_refused() {
        let _images = DISK_IMAGES.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let root = tempfile::tempdir().unwrap();
        let running = root.path().join("installed");
        let incoming = root.path().join("incoming");
        fs::create_dir_all(&running).unwrap();
        fs::create_dir_all(&incoming).unwrap();
        let bundle = fixture_app(&running, "com.example.angelly", "old");
        let tampered = fixture_app(&incoming, "com.example.angelly", "new");
        fs::write(tampered.join("Contents/MacOS/angelly"), "#!/bin/sh\necho tampered\n").unwrap();
        let image = disk_image(root.path(), &incoming);

        let error = stage(&image, &bundle, &|_| {}).unwrap_err();
        assert!(error.contains("signature"), "{error}");
        assert!(!running.join(".Angelly.app.update").exists());
    }

    #[test]
    fn the_swap_waits_for_the_old_process_then_replaces_the_bundle() {
        let root = tempfile::tempdir().unwrap();
        let bundle = root.path().join("Angelly.app");
        let staged = root.path().join(".Angelly.app.update");
        fs::create_dir_all(&bundle).unwrap();
        fs::create_dir_all(&staged).unwrap();
        fs::write(bundle.join("version"), "old").unwrap();
        fs::write(staged.join("version"), "new").unwrap();

        let mut parent = Command::new("/bin/sleep").arg("1").spawn().unwrap();
        spawn_swap(parent.id(), &bundle, &staged, "/usr/bin/true").unwrap();
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(fs::read_to_string(bundle.join("version")).unwrap(), "old");

        parent.wait().unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while fs::read_to_string(bundle.join("version")).unwrap_or_default() != "new" {
            assert!(Instant::now() < deadline, "the bundle was never replaced");
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(!staged.exists());
    }
}
