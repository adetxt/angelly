use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};

use tempfile::TempDir;

use super::*;

fn decision(value: &str) -> Decision {
    UntrustedUrl::new(value).decision()
}

fn file_url(path: &Path) -> String {
    Url::from_file_path(path).unwrap().to_string()
}

fn directory() -> TempDir {
    tempfile::tempdir().unwrap()
}

fn write(directory: &TempDir, name: &str, contents: &str) -> PathBuf {
    let path = directory.path().join(name);
    fs::write(&path, contents).unwrap();
    path
}

#[test]
fn allows_safe_schemes() {
    for value in ["http://example.com", "https://example.com/path", "mailto:user@example.com"] {
        assert_eq!(decision(value), Decision::Allow(value.to_string()), "{value}");
    }
}

#[test]
fn rejects_web_urls_without_hosts() {
    for value in ["https:relative", "http:///missing-host", "https://", "http://?query"] {
        assert_eq!(decision(value), Decision::Deny(DenialReason::InvalidWebUrl), "{value}");
    }
}

#[test]
fn rejects_scheme_less_targets() {
    for value in ["/tmp/file.txt", "../file.txt", "payload.command", ""] {
        assert_eq!(decision(value), Decision::Deny(DenialReason::MalformedUrl), "{value:?}");
    }
}

#[test]
fn rejects_an_empty_mailto() {
    assert_eq!(decision("mailto:"), Decision::Deny(DenialReason::MalformedUrl));
}

#[test]
fn confirms_custom_schemes() {
    for value in ["vscode://file/tmp/example.swift", "ssh://example.com"] {
        assert_eq!(decision(value), Decision::Confirm(value.to_string()), "{value}");
    }
}

#[test]
fn rejects_invisible_and_line_breaking_characters() {
    for scalar in ["\u{0085}", "\u{2028}", "\u{2029}", "\u{202E}", "\u{2066}", "\n", "\u{200B}", "\u{FEFF}"] {
        let value = format!("https://example.com/before{scalar}after");
        assert_eq!(decision(&value), Decision::Deny(DenialReason::UnsafeCharacters), "{scalar:?}");
        assert!(contains_unsafe_characters(&value));
    }
    assert!(!contains_unsafe_characters("https://example.com/é"));
}

#[test]
fn allows_non_executable_local_files() {
    let dir = directory();
    let file = write(&dir, "document.txt", "safe");
    assert_eq!(
        decision(&file_url(&file)),
        Decision::AllowFile(fs::canonicalize(&file).unwrap())
    );
}

#[test]
fn allows_directories_and_localhost() {
    let dir = directory();
    let canonical = fs::canonicalize(dir.path()).unwrap();
    assert_eq!(decision(&file_url(dir.path())), Decision::AllowFile(canonical.clone()));
    let localhost = format!("file://localhost{}", dir.path().display());
    assert_eq!(decision(&localhost), Decision::AllowFile(canonical));
}

#[test]
fn rejects_dangerous_local_file_extensions() {
    for name in ["payload.command", "payload.tool", "payload.app", "payload.workflow", "PAYLOAD.COMMAND"] {
        let dir = directory();
        let file = write(&dir, name, "#!/bin/sh\n");
        assert_eq!(decision(&file_url(&file)), Decision::Deny(DenialReason::UnsafeFile), "{name}");
    }
}

#[test]
fn rejects_script_content_types() {
    let dir = directory();
    let file = write(&dir, "payload.sh", "#!/bin/sh\n");
    assert_eq!(decision(&file_url(&file)), Decision::Deny(DenialReason::UnsafeFile));
}

#[test]
fn rejects_executable_files_regardless_of_extension() {
    let dir = directory();
    let file = write(&dir, "payload.txt", "#!/bin/sh\n");
    fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(decision(&file_url(&file)), Decision::Deny(DenialReason::UnsafeFile));
}

#[test]
fn resolves_symlinks_before_classifying_files() {
    let dir = directory();
    let payload = write(&dir, "payload.command", "#!/bin/sh\n");
    let link = dir.path().join("document.txt");
    symlink(&payload, &link).unwrap();
    assert_eq!(decision(&file_url(&link)), Decision::Deny(DenialReason::UnsafeFile));
}

#[test]
fn rejects_missing_and_special_files() {
    let dir = directory();
    let missing = dir.path().join("missing.txt");
    assert_eq!(decision(&file_url(&missing)), Decision::Deny(DenialReason::InaccessibleFile));
    assert_eq!(decision("file:///dev/null"), Decision::Deny(DenialReason::InaccessibleFile));
}

#[test]
fn rejects_file_urls_with_a_host_query_or_fragment() {
    let dir = directory();
    let file = write(&dir, "document.txt", "safe");
    let url = file_url(&file);
    assert_eq!(decision(&format!("{url}?x=1")), Decision::Deny(DenialReason::MalformedUrl));
    assert_eq!(decision(&format!("{url}#top")), Decision::Deny(DenialReason::MalformedUrl));
    assert_eq!(
        decision("file://remote.example.com/etc/hosts"),
        Decision::Deny(DenialReason::MalformedUrl)
    );
}

#[test]
fn preview_shows_the_effective_standardized_path() {
    for separator in ["\u{0085}", "\u{2028}", "\u{2029}"] {
        let value = format!("/tmp/preview{separator}////../payload.command////");
        assert_eq!(UntrustedUrl::new(value).display_string(), "/tmp/payload.command");
    }
}

#[test]
fn preview_escapes_bidirectional_controls() {
    let value = "https://example.com/a\u{202E}b";
    assert_eq!(UntrustedUrl::new(value).display_string(), "https://example.com/a\\u{202E}b");
}

#[test]
fn preview_keeps_web_urls_byte_for_byte() {
    let value = "https://example.com//a/../b";
    assert_eq!(UntrustedUrl::new(value).display_string(), value);
}

#[test]
fn preview_standardizes_and_resolves_file_urls() {
    let dir = directory();
    let payload = write(&dir, "payload.command", "x");
    let link = dir.path().join("link.txt");
    symlink(&payload, &link).unwrap();
    let url = format!("{}/./sub/../link.txt", file_url(dir.path()).trim_end_matches('/'));
    let shown = UntrustedUrl::new(url).display_string();
    assert_eq!(shown, fs::canonicalize(&payload).unwrap().to_string_lossy());
}

#[test]
fn denial_reasons_have_messages() {
    assert_eq!(
        DenialReason::UnsafeFile.message(),
        "Opening this local target could execute code."
    );
    assert_eq!(
        DenialReason::MalformedUrl.message(),
        "The target is not an absolute URL with a scheme."
    );
}
