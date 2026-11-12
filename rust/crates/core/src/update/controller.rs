use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime};

use super::download::{download_release, Downloader, UreqDownloader};
use super::release::{CheckOutcome, ReleaseInfo, UpdateChecker, UpdateError};
use super::state::UpdateState;
use crate::config::AutoUpdate;
use crate::http::HttpClient;

/// How often background checks run while automatic updates are enabled.
pub const AUTOMATIC_CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

/// Whether a background check is due under the `auto-update` setting.
pub fn automatic_check_due(mode: Option<AutoUpdate>, last_check: Option<SystemTime>, now: SystemTime) -> bool {
    match mode {
        Some(AutoUpdate::Off) => false,
        _ => last_check
            .and_then(|last| now.duration_since(last).ok())
            .is_none_or(|elapsed| elapsed >= AUTOMATIC_CHECK_INTERVAL),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckStart {
    Started,
    /// An update is already installed and waiting for a restart; the caller should
    /// offer to restart instead of checking again.
    RestartRequired,
}

struct Current {
    state: UpdateState,
    /// Bumped whenever work is cancelled so a late result can tell it's stale.
    generation: u64,
    downloaded: Option<PathBuf>,
}

struct Shared {
    current: Mutex<Current>,
    subscribers: Mutex<Vec<Arc<dyn Fn() + Send + Sync>>>,
    checker: UpdateChecker,
    http: Arc<dyn HttpClient>,
    downloader: Arc<dyn Downloader>,
    current_version: String,
    download_dir: PathBuf,
}

/// Drives [`UpdateState`] through checking and downloading. Installing the downloaded
/// package and relaunching is platform work: after [`UpdateController::downloaded_package`]
/// is ready, the installer reports its progress with `report_extraction` and
/// `report_installing`.
#[derive(Clone)]
pub struct UpdateController {
    shared: Arc<Shared>,
}

impl UpdateController {
    pub fn new(current_version: impl Into<String>, download_dir: impl Into<PathBuf>) -> Self {
        let http = crate::http::UreqClient::shared();
        Self::with(current_version, download_dir, UpdateChecker::new(http.clone()), http, Arc::new(UreqDownloader::default()))
    }

    pub fn with(
        current_version: impl Into<String>,
        download_dir: impl Into<PathBuf>,
        checker: UpdateChecker,
        http: Arc<dyn HttpClient>,
        downloader: Arc<dyn Downloader>,
    ) -> Self {
        Self {
            shared: Arc::new(Shared {
                current: Mutex::new(Current { state: UpdateState::Idle, generation: 0, downloaded: None }),
                subscribers: Mutex::new(Vec::new()),
                checker,
                http,
                downloader,
                current_version: current_version.into(),
                download_dir: download_dir.into(),
            }),
        }
    }

    pub fn state(&self) -> UpdateState {
        self.shared.current.lock().unwrap().state.clone()
    }

    /// The verified package once a download has finished.
    pub fn downloaded_package(&self) -> Option<PathBuf> {
        self.shared.current.lock().unwrap().downloaded.clone()
    }

    /// Registers a callback run after every state change, on whichever thread made it.
    pub fn subscribe(&self, callback: impl Fn() + Send + Sync + 'static) {
        self.shared.subscribers.lock().unwrap().push(Arc::new(callback));
    }

    /// Checks GitHub for a newer release, abandoning whatever the previous state was
    /// doing.
    pub fn check_for_updates(&self) -> CheckStart {
        let generation = {
            let mut current = self.shared.current.lock().unwrap();
            if matches!(current.state, UpdateState::Installing { .. }) {
                return CheckStart::RestartRequired;
            }
            current.generation += 1;
            current.downloaded = None;
            current.state = UpdateState::Checking;
            current.generation
        };
        self.notify();

        let controller = self.clone();
        thread::spawn(move || {
            let outcome = controller.shared.checker.check(&controller.shared.current_version);
            let next = match outcome {
                Ok(CheckOutcome::UpdateAvailable(release)) => UpdateState::UpdateAvailable(release),
                Ok(CheckOutcome::UpToDate) => UpdateState::NotFound,
                Err(error) => UpdateState::Error { message: error.to_string() },
            };
            controller.transition(generation, next);
        });
        CheckStart::Started
    }

    /// Backs out of the current state. Checking, an offered update, a download, "no
    /// updates" and errors can all be dismissed; other states are left alone.
    pub fn cancel(&self) {
        {
            let mut current = self.shared.current.lock().unwrap();
            if !current.state.is_cancellable() {
                return;
            }
            current.generation += 1;
            current.state = UpdateState::Idle;
        }
        self.notify();
    }

    /// Clears an error and checks again.
    pub fn retry(&self) -> CheckStart {
        self.cancel();
        self.check_for_updates()
    }

    /// Accepts the current state: an offered update starts downloading, and an update
    /// installed in the background is promoted to a manual install. Returns true when
    /// the caller should now quit and relaunch into the installed update.
    pub fn confirm(&self) -> bool {
        let mut current = self.shared.current.lock().unwrap();
        match current.state.clone() {
            UpdateState::UpdateAvailable(release) => {
                current.generation += 1;
                let generation = current.generation;
                current.state = UpdateState::Downloading { expected_length: None, progress: 0 };
                drop(current);
                self.notify();
                self.spawn_download(release, generation);
                false
            }
            UpdateState::Installing { release: Some(_) } => {
                // Dropping the release suppresses the termination warnings.
                current.state = UpdateState::Installing { release: None };
                drop(current);
                self.notify();
                true
            }
            _ => false,
        }
    }

    pub fn request_permission(&self) {
        self.set(UpdateState::PermissionRequest);
    }

    /// Dismisses the permission prompt. Persisting the answer is up to the caller.
    pub fn answer_permission(&self) {
        let mut current = self.shared.current.lock().unwrap();
        if current.state == UpdateState::PermissionRequest {
            current.state = UpdateState::Idle;
            drop(current);
            self.notify();
        }
    }

    pub fn report_extraction(&self, progress: f64) {
        self.set(UpdateState::Extracting { progress: progress.clamp(0.0, 1.0) });
    }

    /// `release` is `Some` when an automatic update was installed silently.
    pub fn report_installing(&self, release: Option<ReleaseInfo>) {
        self.set(UpdateState::Installing { release });
    }

    /// Ends the update with a failure the user can read and retry.
    pub fn report_error(&self, message: impl Into<String>) {
        self.set(UpdateState::Error { message: message.into() });
    }

    pub fn reset(&self) {
        let mut current = self.shared.current.lock().unwrap();
        current.generation += 1;
        current.state = UpdateState::Idle;
        drop(current);
        self.notify();
    }

    fn set(&self, state: UpdateState) {
        self.shared.current.lock().unwrap().state = state;
        self.notify();
    }

    fn transition(&self, generation: u64, state: UpdateState) {
        {
            let mut current = self.shared.current.lock().unwrap();
            if current.generation != generation {
                return;
            }
            current.state = state;
        }
        self.notify();
    }

    fn spawn_download(&self, release: ReleaseInfo, generation: u64) {
        let controller = self.clone();
        thread::spawn(move || {
            let shared = &controller.shared;
            let result = std::fs::create_dir_all(&shared.download_dir)
                .map_err(|e| UpdateError::DownloadFailed(e.to_string()))
                .and_then(|()| {
                    download_release(&release, &shared.download_dir, &shared.http, shared.downloader.as_ref(), &mut |expected_length, progress| {
                        controller.transition(generation, UpdateState::Downloading { expected_length, progress });
                    })
                });
            match result {
                Ok(path) => {
                    let mut current = shared.current.lock().unwrap();
                    if current.generation != generation {
                        drop(current);
                        let _ = std::fs::remove_file(path);
                        return;
                    }
                    current.downloaded = Some(path);
                    current.state = UpdateState::Extracting { progress: 0.0 };
                    drop(current);
                    controller.notify();
                }
                Err(error) => controller.transition(generation, UpdateState::Error { message: error.to_string() }),
            }
        });
    }

    fn notify(&self) {
        let subscribers = self.shared.subscribers.lock().unwrap().clone();
        for callback in subscribers {
            callback();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{FnHttpClient, HttpRequest, HttpResponse};
    use std::path::Path;
    use std::sync::{Condvar, Mutex};
    use std::time::{Instant, UNIX_EPOCH};

    const DIGEST_OF_HELLO: &str = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";

    fn release_json(version: &str) -> String {
        format!(
            r#"{{"tag_name":"v{version}","html_url":"https://example.invalid/r","assets":[
            {{"name":"A.dmg","browser_download_url":"https://example.invalid/A.dmg","size":5}},
            {{"name":"A.dmg.sha256","browser_download_url":"https://example.invalid/A.dmg.sha256","size":70}}]}}"#
        )
    }

    struct Gate {
        open: Mutex<bool>,
        cv: Condvar,
    }

    impl Gate {
        fn new(open: bool) -> Arc<Self> {
            Arc::new(Self { open: Mutex::new(open), cv: Condvar::new() })
        }
        fn wait(&self) {
            let mut open = self.open.lock().unwrap();
            while !*open {
                open = self.cv.wait(open).unwrap();
            }
        }
        fn release(&self) {
            *self.open.lock().unwrap() = true;
            self.cv.notify_all();
        }
    }

    struct FakeDownloader;

    impl Downloader for FakeDownloader {
        fn download(&self, _: &str, destination: &Path, progress: &mut dyn FnMut(Option<u64>, u64)) -> Result<(), UpdateError> {
            progress(Some(5), 2);
            std::fs::write(destination, b"hello").unwrap();
            Ok(())
        }
    }

    fn controller(latest: &'static str, gate: Arc<Gate>, dir: &Path) -> UpdateController {
        let http: Arc<dyn HttpClient> = Arc::new(FnHttpClient(move |request: &HttpRequest| {
            if request.url.ends_with("/releases/latest") {
                gate.wait();
                Ok(HttpResponse::new(200, release_json(latest)))
            } else {
                Ok(HttpResponse::new(200, format!("{DIGEST_OF_HELLO}  A.dmg\n")))
            }
        }));
        UpdateController::with("0.15.0", dir, UpdateChecker::new(http.clone()), http, Arc::new(FakeDownloader))
    }

    fn wait_for(controller: &UpdateController, done: impl Fn(&UpdateState) -> bool) -> UpdateState {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let state = controller.state();
            if done(&state) {
                return state;
            }
            assert!(Instant::now() < deadline, "timed out in {state:?}");
            thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn check_finds_a_newer_release() {
        let dir = tempfile::tempdir().unwrap();
        let controller = controller("0.16.0", Gate::new(true), dir.path());
        assert_eq!(controller.check_for_updates(), CheckStart::Started);
        let state = wait_for(&controller, |s| !matches!(s, UpdateState::Checking));
        assert!(matches!(&state, UpdateState::UpdateAvailable(r) if r.display_version() == "0.16.0"), "{state:?}");
    }

    #[test]
    fn check_reports_no_update_and_errors() {
        let dir = tempfile::tempdir().unwrap();
        let current = controller("0.15.0", Gate::new(true), dir.path());
        current.check_for_updates();
        assert_eq!(wait_for(&current, |s| !matches!(s, UpdateState::Checking)), UpdateState::NotFound);

        let failing: Arc<dyn HttpClient> = Arc::new(FnHttpClient(|_: &HttpRequest| Err(crate::http::HttpError("offline".into()))));
        let broken = UpdateController::with("0.15.0", dir.path(), UpdateChecker::new(failing.clone()), failing, Arc::new(FakeDownloader));
        broken.check_for_updates();
        let state = wait_for(&broken, |s| !matches!(s, UpdateState::Checking));
        assert_eq!(state, UpdateState::Error { message: UpdateError::Network.to_string() });
        assert_eq!(broken.retry(), CheckStart::Started);
    }

    #[test]
    fn cancelling_while_checking_discards_the_late_result() {
        let dir = tempfile::tempdir().unwrap();
        let gate = Gate::new(false);
        let controller = controller("0.16.0", gate.clone(), dir.path());
        controller.check_for_updates();
        assert_eq!(controller.state(), UpdateState::Checking);
        controller.cancel();
        assert_eq!(controller.state(), UpdateState::Idle);
        gate.release();
        thread::sleep(Duration::from_millis(100));
        assert_eq!(controller.state(), UpdateState::Idle);
    }

    #[test]
    fn a_new_check_supersedes_the_previous_one() {
        let dir = tempfile::tempdir().unwrap();
        let gate = Gate::new(false);
        let controller = controller("0.16.0", gate.clone(), dir.path());
        controller.check_for_updates();
        controller.check_for_updates();
        gate.release();
        wait_for(&controller, |s| matches!(s, UpdateState::UpdateAvailable(_)));
    }

    #[test]
    fn confirm_downloads_and_verifies_the_package() {
        let dir = tempfile::tempdir().unwrap();
        let controller = controller("0.16.0", Gate::new(true), dir.path());
        controller.check_for_updates();
        wait_for(&controller, |s| matches!(s, UpdateState::UpdateAvailable(_)));
        assert!(!controller.confirm());
        let state = wait_for(&controller, |s| matches!(s, UpdateState::Extracting { .. } | UpdateState::Error { .. }));
        assert_eq!(state, UpdateState::Extracting { progress: 0.0 });
        let package = controller.downloaded_package().unwrap();
        assert_eq!(std::fs::read(&package).unwrap(), b"hello");
        assert_eq!(package, dir.path().join("A.dmg"));
    }

    #[test]
    fn checksum_mismatch_becomes_an_error_and_leaves_no_file() {
        let dir = tempfile::tempdir().unwrap();
        let http: Arc<dyn HttpClient> = Arc::new(FnHttpClient(|request: &HttpRequest| {
            if request.url.ends_with("/releases/latest") {
                Ok(HttpResponse::new(200, release_json("0.16.0")))
            } else {
                Ok(HttpResponse::new(200, "0".repeat(64)))
            }
        }));
        let controller = UpdateController::with("0.15.0", dir.path(), UpdateChecker::new(http.clone()), http, Arc::new(FakeDownloader));
        controller.check_for_updates();
        wait_for(&controller, |s| matches!(s, UpdateState::UpdateAvailable(_)));
        controller.confirm();
        let state = wait_for(&controller, |s| matches!(s, UpdateState::Error { .. }));
        assert_eq!(state, UpdateState::Error { message: UpdateError::ChecksumMismatch.to_string() });
        assert!(controller.downloaded_package().is_none());
        assert!(!dir.path().join("A.dmg").exists());
    }

    #[test]
    fn confirm_promotes_a_silent_install_and_asks_to_relaunch() {
        let dir = tempfile::tempdir().unwrap();
        let controller = controller("0.16.0", Gate::new(true), dir.path());
        controller.report_installing(Some(ReleaseInfo::for_tests("0.16.0")));
        assert!(controller.state().is_hidden());
        assert!(controller.confirm());
        assert!(controller.state().should_terminate_without_warning());
        assert!(!controller.confirm());
    }

    #[test]
    fn checking_while_an_update_is_installed_asks_for_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let controller = controller("0.16.0", Gate::new(true), dir.path());
        controller.report_installing(Some(ReleaseInfo::for_tests("0.16.0")));
        assert_eq!(controller.check_for_updates(), CheckStart::RestartRequired);
        assert!(matches!(controller.state(), UpdateState::Installing { .. }));
    }

    #[test]
    fn cancel_only_affects_cancellable_states() {
        let dir = tempfile::tempdir().unwrap();
        let controller = controller("0.16.0", Gate::new(true), dir.path());
        controller.report_extraction(0.5);
        controller.cancel();
        assert_eq!(controller.state(), UpdateState::Extracting { progress: 0.5 });
        controller.reset();
        assert_eq!(controller.state(), UpdateState::Idle);
    }

    #[test]
    fn permission_prompt_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let controller = controller("0.16.0", Gate::new(true), dir.path());
        controller.request_permission();
        assert_eq!(controller.state(), UpdateState::PermissionRequest);
        controller.answer_permission();
        assert_eq!(controller.state(), UpdateState::Idle);
    }

    #[test]
    fn subscribers_hear_about_changes() {
        let dir = tempfile::tempdir().unwrap();
        let controller = controller("0.16.0", Gate::new(true), dir.path());
        let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let seen = count.clone();
        controller.subscribe(move || {
            seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        });
        controller.check_for_updates();
        wait_for(&controller, |s| matches!(s, UpdateState::UpdateAvailable(_)));
        assert!(count.load(std::sync::atomic::Ordering::SeqCst) >= 2);
    }

    #[test]
    fn automatic_checks_follow_the_mode_and_interval() {
        let now = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        let ago = |secs| Some(now - Duration::from_secs(secs));
        assert!(!automatic_check_due(Some(AutoUpdate::Off), None, now));
        assert!(automatic_check_due(Some(AutoUpdate::Check), None, now));
        assert!(automatic_check_due(None, ago(86_400), now));
        assert!(!automatic_check_due(Some(AutoUpdate::Download), ago(86_399), now));
        assert!(automatic_check_due(Some(AutoUpdate::Check), Some(now + Duration::from_secs(5)), now));
    }
}
