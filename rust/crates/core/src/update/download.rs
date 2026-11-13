use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use sha2::{Digest, Sha256};

use super::release::{ReleaseAsset, ReleaseInfo, UpdateError};

const MAX_CHECKSUM_BYTES: usize = 4096;

/// Fetches a file to disk, reporting `(expected_length, received_so_far)` as it goes.
pub trait Downloader: Send + Sync {
    fn download(&self, url: &str, destination: &Path, progress: &mut dyn FnMut(Option<u64>, u64)) -> Result<(), UpdateError>;
}

/// Streams a file over HTTPS. Release assets are served through a redirect to a CDN, so
/// redirects are followed here; no credentials are ever attached to these requests.
pub struct UreqDownloader {
    agent: ureq::Agent,
}

impl Default for UreqDownloader {
    fn default() -> Self {
        let agent = ureq::AgentBuilder::new()
            .redirects(5)
            .https_only(true)
            .timeout_connect(Duration::from_secs(10))
            .timeout_read(Duration::from_secs(30))
            .build();
        Self { agent }
    }
}

impl Downloader for UreqDownloader {
    fn download(&self, url: &str, destination: &Path, progress: &mut dyn FnMut(Option<u64>, u64)) -> Result<(), UpdateError> {
        let failed = |reason: &str| UpdateError::DownloadFailed(reason.to_string());
        let response = self
            .agent
            .get(url)
            .set("User-Agent", "Angelly")
            .call()
            .map_err(|e| match e {
                ureq::Error::Status(status, _) => UpdateError::DownloadFailed(format!("HTTP {status}")),
                ureq::Error::Transport(_) => UpdateError::Network,
            })?;
        let expected = response.header("Content-Length").and_then(|v| v.parse::<u64>().ok());
        progress(expected, 0);
        let mut reader = response.into_reader();
        let mut file = std::fs::File::create(destination).map_err(|_| failed("couldn't write the file"))?;
        let mut buffer = vec![0u8; 64 * 1024];
        let mut received = 0u64;
        loop {
            let n = reader.read(&mut buffer).map_err(|_| UpdateError::Network)?;
            if n == 0 {
                break;
            }
            file.write_all(&buffer[..n]).map_err(|_| failed("couldn't write the file"))?;
            received += n as u64;
            progress(expected, received);
        }
        if expected.is_some_and(|e| e != received) {
            return Err(failed("the download was cut short"));
        }
        Ok(())
    }
}

/// Extracts the hex digest from `shasum` output (`<hex>  <filename>`) or a bare digest.
pub fn parse_checksum(text: &str) -> Option<String> {
    let digest = text.split_whitespace().next()?.to_ascii_lowercase();
    (digest.len() == 64 && digest.bytes().all(|b| b.is_ascii_hexdigit())).then_some(digest)
}

pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Reads the digest published beside a package. Release assets are served through a
/// redirect, so it travels through the redirect-following `downloader` rather than the
/// API client, which must not follow redirects. Any failure to obtain a usable digest
/// means the package can't be verified.
fn fetch_expected_digest(asset: &ReleaseAsset, scratch: &Path, downloader: &dyn Downloader) -> Result<String, UpdateError> {
    downloader.download(&asset.url, scratch, &mut |_, _| {}).map_err(|e| match e {
        UpdateError::Network => UpdateError::Network,
        _ => UpdateError::MissingChecksum,
    })?;
    let mut text = Vec::new();
    let read = std::fs::File::open(scratch).and_then(|f| f.take(MAX_CHECKSUM_BYTES as u64 + 1).read_to_end(&mut text));
    let _ = std::fs::remove_file(scratch);
    read.map_err(|_| UpdateError::MissingChecksum)?;
    if text.len() > MAX_CHECKSUM_BYTES {
        return Err(UpdateError::MissingChecksum);
    }
    parse_checksum(&String::from_utf8_lossy(&text)).ok_or(UpdateError::MissingChecksum)
}

/// Downloads a release's package into `directory` and checks it against the checksum
/// published beside it. A package without a published checksum is refused, and a file
/// that doesn't match is deleted.
pub fn download_release(
    release: &ReleaseInfo,
    directory: &Path,
    downloader: &dyn Downloader,
    progress: &mut dyn FnMut(Option<u64>, u64),
) -> Result<PathBuf, UpdateError> {
    let package: &ReleaseAsset = release.package.as_ref().ok_or(UpdateError::NoPackage)?;
    let checksum_asset = release.checksum.as_ref().ok_or(UpdateError::MissingChecksum)?;

    let file_name = Path::new(&package.name).file_name().ok_or(UpdateError::NoPackage)?;
    let destination = directory.join(file_name);
    let mut scratch = destination.clone().into_os_string();
    scratch.push(".sha256");
    let expected = fetch_expected_digest(checksum_asset, Path::new(&scratch), downloader)?;

    downloader.download(&package.url, &destination, progress)?;
    let actual = sha256_file(&destination).map_err(|_| UpdateError::DownloadFailed("couldn't read the file".to_string()))?;
    if actual != expected {
        let _ = std::fs::remove_file(&destination);
        return Err(UpdateError::ChecksumMismatch);
    }
    Ok(destination)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIGEST_OF_HELLO: &str = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";

    /// Serves `package` for the package URL and `checksum` (or an HTTP 404) for the
    /// `.sha256` URL, like a CDN behind a redirect.
    struct FakeDownloader {
        package: &'static [u8],
        checksum: Option<String>,
    }

    impl FakeDownloader {
        fn new(checksum: Option<&str>) -> Self {
            Self { package: b"hello", checksum: checksum.map(str::to_string) }
        }
    }

    impl Downloader for FakeDownloader {
        fn download(&self, url: &str, destination: &Path, progress: &mut dyn FnMut(Option<u64>, u64)) -> Result<(), UpdateError> {
            let body = if url.ends_with(".sha256") {
                self.checksum.clone().ok_or_else(|| UpdateError::DownloadFailed("HTTP 404".to_string()))?.into_bytes()
            } else {
                self.package.to_vec()
            };
            progress(Some(body.len() as u64), 0);
            std::fs::write(destination, &body).unwrap();
            progress(Some(body.len() as u64), body.len() as u64);
            Ok(())
        }
    }

    fn release() -> ReleaseInfo {
        let asset = |name: &str| ReleaseAsset { name: name.to_string(), url: format!("https://example.invalid/{name}"), size: None };
        ReleaseInfo {
            package: Some(asset("Angelly-1.0.0.dmg")),
            checksum: Some(asset("Angelly-1.0.0.dmg.sha256")),
            ..ReleaseInfo::for_tests("1.0.0")
        }
    }

    #[test]
    fn checksum_formats() {
        assert_eq!(parse_checksum(&format!("{DIGEST_OF_HELLO}  Angelly.dmg\n")).as_deref(), Some(DIGEST_OF_HELLO));
        assert_eq!(parse_checksum(&DIGEST_OF_HELLO.to_uppercase()).as_deref(), Some(DIGEST_OF_HELLO));
        assert_eq!(parse_checksum("short"), None);
        assert_eq!(parse_checksum(""), None);
        assert_eq!(parse_checksum(&"z".repeat(64)), None);
    }

    #[test]
    fn verified_download_returns_the_path_and_reports_package_progress() {
        let dir = tempfile::tempdir().unwrap();
        let downloader = FakeDownloader::new(Some(&format!("{DIGEST_OF_HELLO}  Angelly-1.0.0.dmg\n")));
        let mut seen = Vec::new();
        let path = download_release(&release(), dir.path(), &downloader, &mut |e, p| seen.push((e, p))).unwrap();
        assert_eq!(path, dir.path().join("Angelly-1.0.0.dmg"));
        assert_eq!(std::fs::read(path).unwrap(), b"hello");
        assert_eq!(seen, vec![(Some(5), 0), (Some(5), 5)]);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1, "the checksum scratch file is removed");
    }

    #[test]
    fn mismatched_download_is_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let downloader = FakeDownloader::new(Some(&format!("{}  x\n", "0".repeat(64))));
        let result = download_release(&release(), dir.path(), &downloader, &mut |_, _| {});
        assert_eq!(result, Err(UpdateError::ChecksumMismatch));
        assert!(std::fs::read_dir(dir.path()).unwrap().next().is_none());
    }

    #[test]
    fn missing_or_unreadable_checksums_refuse_the_download() {
        let dir = tempfile::tempdir().unwrap();
        let mut unverified = release();
        unverified.checksum = None;
        let result = download_release(&unverified, dir.path(), &FakeDownloader::new(None), &mut |_, _| panic!("no download without a checksum"));
        assert_eq!(result, Err(UpdateError::MissingChecksum));

        let oversized = "a".repeat(MAX_CHECKSUM_BYTES + 1);
        for checksum in [Some("garbage"), Some(oversized.as_str()), None] {
            let result = download_release(&release(), dir.path(), &FakeDownloader::new(checksum), &mut |_, _| panic!("package must not download"));
            assert_eq!(result, Err(UpdateError::MissingChecksum), "{checksum:?}");
        }
        assert!(std::fs::read_dir(dir.path()).unwrap().next().is_none());
    }

    #[test]
    fn package_names_cannot_escape_the_download_directory() {
        let dir = tempfile::tempdir().unwrap();
        let mut sneaky = release();
        sneaky.package.as_mut().unwrap().name = "../../evil.dmg".to_string();
        let downloader = FakeDownloader::new(Some(&format!("{DIGEST_OF_HELLO}\n")));
        let path = download_release(&sneaky, dir.path(), &downloader, &mut |_, _| {}).unwrap();
        assert_eq!(path, dir.path().join("evil.dmg"));
    }

    #[test]
    fn sha256_matches_known_digest() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("f");
        std::fs::write(&file, "hello").unwrap();
        assert_eq!(sha256_file(&file).unwrap(), DIGEST_OF_HELLO);
    }
}
