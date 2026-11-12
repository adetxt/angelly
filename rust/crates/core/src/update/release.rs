use std::sync::Arc;
use std::time::SystemTime;

use serde::Deserialize;

use super::state::ReleaseNotes;
use super::version::is_newer;
use crate::dates::parse_iso8601;
use crate::http::{HttpClient, HttpRequest};

pub const DEFAULT_REPOSITORY: &str = "adetxt/angelly";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseAsset {
    pub name: String,
    pub url: String,
    pub size: Option<u64>,
}

/// A published GitHub release that is newer than the running app.
#[derive(Debug, Clone, PartialEq)]
pub struct ReleaseInfo {
    pub tag: String,
    pub name: Option<String>,
    pub page_url: String,
    pub published_at: Option<SystemTime>,
    pub body: Option<String>,
    /// The installable package, e.g. the DMG.
    pub package: Option<ReleaseAsset>,
    /// The `<package>.sha256` file published beside it.
    pub checksum: Option<ReleaseAsset>,
}

impl ReleaseInfo {
    /// The tag without a leading `v`, as shown to users.
    pub fn display_version(&self) -> &str {
        self.tag.strip_prefix('v').unwrap_or(&self.tag)
    }

    pub fn notes(&self) -> Option<ReleaseNotes> {
        ReleaseNotes::new(self.display_version(), None)
    }

    #[cfg(test)]
    pub(crate) fn for_tests(version: &str) -> Self {
        Self {
            tag: format!("v{version}"),
            name: None,
            page_url: String::new(),
            published_at: None,
            body: None,
            package: None,
            checksum: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateError {
    Network,
    RateLimited,
    UnexpectedStatus(u16),
    InvalidResponse,
    NoPackage,
    DownloadFailed(String),
    ChecksumMismatch,
    MissingChecksum,
}

impl std::fmt::Display for UpdateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Network => f.write_str("Couldn't reach GitHub. Check your connection and try again."),
            Self::RateLimited => f.write_str("GitHub is limiting update checks right now. Try again later."),
            Self::UnexpectedStatus(status) => write!(f, "GitHub returned an unexpected response (HTTP {status})."),
            Self::InvalidResponse => f.write_str("The release information from GitHub couldn't be read."),
            Self::NoPackage => f.write_str("The latest release has no installable package for this platform."),
            Self::DownloadFailed(reason) => write!(f, "The update couldn't be downloaded: {reason}"),
            Self::ChecksumMismatch => f.write_str("The downloaded update didn't match its published checksum and was discarded."),
            Self::MissingChecksum => f.write_str("The release doesn't publish a checksum for its package, so it won't be installed."),
        }
    }
}

impl std::error::Error for UpdateError {}

#[derive(Debug, Clone, PartialEq)]
pub enum CheckOutcome {
    UpdateAvailable(ReleaseInfo),
    UpToDate,
}

#[derive(Deserialize)]
struct RawRelease {
    tag_name: String,
    name: Option<String>,
    html_url: String,
    published_at: Option<String>,
    body: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<RawAsset>,
}

#[derive(Deserialize)]
struct RawAsset {
    name: String,
    browser_download_url: String,
    size: Option<u64>,
}

/// Looks up the newest published release of a GitHub repository.
pub struct UpdateChecker {
    http: Arc<dyn HttpClient>,
    repository: String,
    package_suffix: String,
}

impl UpdateChecker {
    pub fn new(http: Arc<dyn HttpClient>) -> Self {
        Self { http, repository: DEFAULT_REPOSITORY.to_string(), package_suffix: ".dmg".to_string() }
    }

    pub fn repository(mut self, repository: impl Into<String>) -> Self {
        self.repository = repository.into();
        self
    }

    /// The file extension of the package to install, such as `.dmg`.
    pub fn package_suffix(mut self, suffix: impl Into<String>) -> Self {
        self.package_suffix = suffix.into();
        self
    }

    pub fn check(&self, current_version: &str) -> Result<CheckOutcome, UpdateError> {
        let request = HttpRequest::get(format!("https://api.github.com/repos/{}/releases/latest", self.repository))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .header("User-Agent", "Angelly");
        let response = self.http.get(&request).map_err(|_| UpdateError::Network)?;
        match response.status {
            200 => {}
            404 => return Ok(CheckOutcome::UpToDate),
            429 => return Err(UpdateError::RateLimited),
            403 if response.header("x-ratelimit-remaining") == Some("0") || response.header("retry-after").is_some() => {
                return Err(UpdateError::RateLimited)
            }
            status => return Err(UpdateError::UnexpectedStatus(status)),
        }
        let raw: RawRelease = serde_json::from_slice(&response.body).map_err(|_| UpdateError::InvalidResponse)?;
        if raw.draft || raw.prerelease {
            return Ok(CheckOutcome::UpToDate);
        }
        let release = self.release_info(raw);
        if !release.display_version().bytes().any(|b| b.is_ascii_digit()) {
            return Err(UpdateError::InvalidResponse);
        }
        if !is_newer(release.display_version(), current_version) {
            return Ok(CheckOutcome::UpToDate);
        }
        if release.package.is_none() {
            return Err(UpdateError::NoPackage);
        }
        Ok(CheckOutcome::UpdateAvailable(release))
    }

    fn release_info(&self, raw: RawRelease) -> ReleaseInfo {
        let asset = |a: &RawAsset| ReleaseAsset { name: a.name.clone(), url: a.browser_download_url.clone(), size: a.size };
        let package = raw.assets.iter().find(|a| a.name.ends_with(&self.package_suffix)).map(asset);
        let checksum = package.as_ref().and_then(|p| {
            let wanted = format!("{}.sha256", p.name);
            raw.assets.iter().find(|a| a.name == wanted).map(asset)
        });
        ReleaseInfo {
            tag: raw.tag_name,
            name: raw.name.filter(|n| !n.is_empty()),
            page_url: raw.html_url,
            published_at: raw.published_at.as_deref().and_then(parse_iso8601),
            body: raw.body.filter(|b| !b.is_empty()),
            package,
            checksum,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{FnHttpClient, HttpError, HttpResponse};

    fn checker(response: Result<HttpResponse, HttpError>) -> UpdateChecker {
        UpdateChecker::new(Arc::new(FnHttpClient(move |request: &HttpRequest| {
            assert_eq!(request.url, "https://api.github.com/repos/adetxt/angelly/releases/latest");
            assert_eq!(request.header_value("User-Agent"), Some("Angelly"));
            response.clone()
        })))
    }

    const RELEASE: &str = r###"{
        "tag_name": "v0.16.0",
        "name": "Angelly v0.16.0",
        "html_url": "https://github.com/adetxt/angelly/releases/tag/v0.16.0",
        "published_at": "2026-11-10T12:30:00Z",
        "body": "## Changes",
        "draft": false,
        "prerelease": false,
        "assets": [
            {"name": "Angelly-0.16.0.dmg.sha256", "browser_download_url": "https://example.invalid/Angelly-0.16.0.dmg.sha256", "size": 90},
            {"name": "appcast.xml", "browser_download_url": "https://example.invalid/appcast.xml", "size": 10},
            {"name": "Angelly-0.16.0.dmg", "browser_download_url": "https://example.invalid/Angelly-0.16.0.dmg", "size": 123456}
        ]
    }"###;

    #[test]
    fn newer_release_is_offered_with_its_package_and_checksum() {
        let outcome = checker(Ok(HttpResponse::new(200, RELEASE))).check("0.15.0").unwrap();
        let CheckOutcome::UpdateAvailable(release) = outcome else { panic!("expected an update") };
        assert_eq!(release.display_version(), "0.16.0");
        assert_eq!(release.package.as_ref().unwrap().name, "Angelly-0.16.0.dmg");
        assert_eq!(release.package.as_ref().unwrap().size, Some(123_456));
        assert_eq!(release.checksum.as_ref().unwrap().name, "Angelly-0.16.0.dmg.sha256");
        assert_eq!(release.page_url, "https://github.com/adetxt/angelly/releases/tag/v0.16.0");
        assert!(release.published_at.is_some());
        assert_eq!(release.notes().unwrap().label(), "View Release Notes");
    }

    #[test]
    fn same_or_older_release_is_up_to_date() {
        for current in ["0.16.0", "0.17.0", "1.0"] {
            assert_eq!(checker(Ok(HttpResponse::new(200, RELEASE))).check(current), Ok(CheckOutcome::UpToDate), "{current}");
        }
    }

    #[test]
    fn drafts_prereleases_and_missing_releases_are_ignored() {
        for flag in ["draft", "prerelease"] {
            let body = RELEASE.replace(&format!("\"{flag}\": false"), &format!("\"{flag}\": true"));
            assert_eq!(checker(Ok(HttpResponse::new(200, body))).check("0.1.0"), Ok(CheckOutcome::UpToDate), "{flag}");
        }
        assert_eq!(checker(Ok(HttpResponse::new(404, "{}"))).check("0.1.0"), Ok(CheckOutcome::UpToDate));
    }

    #[test]
    fn release_without_a_package_is_an_error_not_an_update() {
        let body = r#"{"tag_name":"v9.0.0","html_url":"https://example.invalid","assets":[{"name":"appcast.xml","browser_download_url":"https://example.invalid/a","size":1}]}"#;
        assert_eq!(checker(Ok(HttpResponse::new(200, body))).check("0.1.0"), Err(UpdateError::NoPackage));
    }

    #[test]
    fn package_without_checksum_is_still_reported() {
        let body = r#"{"tag_name":"v9.0.0","html_url":"https://example.invalid","assets":[{"name":"A.dmg","browser_download_url":"https://example.invalid/A.dmg","size":1}]}"#;
        let CheckOutcome::UpdateAvailable(release) = checker(Ok(HttpResponse::new(200, body))).check("0.1.0").unwrap() else {
            panic!("expected an update")
        };
        assert!(release.checksum.is_none());
    }

    #[test]
    fn failures_map_to_errors() {
        assert_eq!(checker(Err(HttpError("offline".into()))).check("0.1.0"), Err(UpdateError::Network));
        assert_eq!(checker(Ok(HttpResponse::new(429, ""))).check("0.1.0"), Err(UpdateError::RateLimited));
        let limited = HttpResponse::new(403, "").with_header("x-ratelimit-remaining", "0");
        assert_eq!(checker(Ok(limited)).check("0.1.0"), Err(UpdateError::RateLimited));
        assert_eq!(checker(Ok(HttpResponse::new(403, ""))).check("0.1.0"), Err(UpdateError::UnexpectedStatus(403)));
        assert_eq!(checker(Ok(HttpResponse::new(302, ""))).check("0.1.0"), Err(UpdateError::UnexpectedStatus(302)));
        assert_eq!(checker(Ok(HttpResponse::new(200, "not json"))).check("0.1.0"), Err(UpdateError::InvalidResponse));
        let tag = RELEASE.replace("v0.16.0\",\n        \"name", "latest\",\n        \"name");
        assert_eq!(checker(Ok(HttpResponse::new(200, tag))).check("0.1.0"), Err(UpdateError::InvalidResponse));
    }

    #[test]
    fn repository_and_package_suffix_are_configurable() {
        let http = Arc::new(FnHttpClient(|request: &HttpRequest| {
            assert!(request.url.contains("/repos/example/app/"));
            Ok(HttpResponse::new(
                200,
                r#"{"tag_name":"2.0.0","html_url":"https://example.invalid","assets":[{"name":"app.AppImage","browser_download_url":"https://example.invalid/app.AppImage"}]}"#,
            ))
        }));
        let checker = UpdateChecker::new(http).repository("example/app").package_suffix(".AppImage");
        let CheckOutcome::UpdateAvailable(release) = checker.check("1.0.0").unwrap() else { panic!("expected an update") };
        assert_eq!(release.display_version(), "2.0.0");
        assert_eq!(release.package.unwrap().name, "app.AppImage");
    }
}
