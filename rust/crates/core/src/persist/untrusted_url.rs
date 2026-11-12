//! Classification of URLs that came from an untrusted source such as terminal output,
//! deciding whether they may be handed to the operating system.

use std::fs;
use std::path::{Component, Path, PathBuf};

use url::Url;

/// Why a target is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DenialReason {
    MalformedUrl,
    UnsafeCharacters,
    InvalidWebUrl,
    InaccessibleFile,
    UnsafeFile,
}

impl DenialReason {
    pub fn message(self) -> &'static str {
        match self {
            Self::MalformedUrl => "The target is not an absolute URL with a scheme.",
            Self::UnsafeCharacters => "The target contains invisible or line-breaking characters.",
            Self::InvalidWebUrl => "The web target does not contain a valid host.",
            Self::InaccessibleFile => "The local target does not exist or is not a regular file or directory.",
            Self::UnsafeFile => "Opening this local target could execute code.",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// A web or mail URL with well-understood, non-executing behavior. Carries the URL
    /// exactly as given.
    Allow(String),
    /// A safe local file or directory, as its canonical path.
    AllowFile(PathBuf),
    /// A custom scheme that would invoke whatever application registered for it. The
    /// caller must show the target and ask before dispatching it.
    Confirm(String),
    Deny(DenialReason),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UntrustedUrl {
    string: String,
}

/// Extensions Launch Services-style handlers treat as executable containers. Blocked
/// even when the file's executable bit is clear.
const UNSAFE_EXTENSIONS: &[&str] = &[
    "action", "app", "applescript", "class", "command", "desktop", "inetloc", "jar", "mobileconfig",
    "mpkg", "pkg", "scpt", "terminal", "tool", "url", "webloc", "workflow",
];

/// Stand-in for the system's `application`, `executable` and `script` content types, which
/// are not available here. Covers common script and binary extensions; the executable-bit
/// check below catches the rest.
const UNSAFE_CONTENT_EXTENSIONS: &[&str] = &[
    "sh", "bash", "zsh", "csh", "ksh", "tcsh", "fish", "py", "pyw", "pl", "pm", "rb", "php", "js",
    "mjs", "cjs", "awk", "tcl", "exe", "dll", "dylib", "bundle", "kext", "xpc", "bat", "cmd", "com",
    "ps1", "msi", "scr", "vbs", "lnk", "reg",
];

impl UntrustedUrl {
    pub fn new(string: impl Into<String>) -> Self {
        Self { string: string.into() }
    }

    pub fn as_str(&self) -> &str {
        &self.string
    }

    pub fn decision(&self) -> Decision {
        let string = self.string.as_str();
        if string.is_empty() {
            return Decision::Deny(DenialReason::MalformedUrl);
        }

        // Control, invisible and bidirectional characters are refused before parsing
        // changes their representation.
        if contains_unsafe_characters(string) {
            return Decision::Deny(DenialReason::UnsafeCharacters);
        }

        // Relative references are never accepted: an explicit scheme keeps a later layer
        // from reinterpreting the target as a local path.
        let Some(scheme) = scheme_of(string) else {
            return Decision::Deny(DenialReason::MalformedUrl);
        };
        let scheme = scheme.to_ascii_lowercase();

        match scheme.as_str() {
            "http" | "https" => self.web_decision(),
            "mailto" => match Url::parse(string) {
                Ok(url) if !url.path().is_empty() => Decision::Allow(string.to_string()),
                _ => Decision::Deny(DenialReason::MalformedUrl),
            },
            "file" => match Url::parse(string) {
                Ok(url) => file_decision(&url),
                Err(_) => Decision::Deny(DenialReason::MalformedUrl),
            },
            _ => match Url::parse(string) {
                Ok(_) => Decision::Confirm(string.to_string()),
                Err(_) => Decision::Deny(DenialReason::MalformedUrl),
            },
        }
    }

    fn web_decision(&self) -> Decision {
        let invalid = Decision::Deny(DenialReason::InvalidWebUrl);
        // "https:relative" has a scheme but no authority; consumers resolve it differently.
        let after_scheme = &self.string[self.string.find(':').map_or(0, |i| i + 1)..];
        let Some(rest) = after_scheme.strip_prefix("//") else {
            return invalid;
        };
        let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
        if authority.is_empty() {
            return invalid;
        }
        match Url::parse(&self.string) {
            Ok(url) if url.host_str().is_some_and(|host| !host.is_empty()) => {
                Decision::Allow(self.string.clone())
            }
            _ => invalid,
        }
    }

    /// A single-line representation of the effective target. File paths are normalized
    /// first so traversal and repeated separators cannot make the shown and opened
    /// targets differ; other URLs are shown as given. Unsafe characters that remain are
    /// escaped as `\u{XXXX}`.
    pub fn display_string(&self) -> String {
        let normalized = match scheme_of(&self.string).and_then(|_| Url::parse(&self.string).ok()) {
            Some(url) if url.scheme() == "file" => {
                let path = url
                    .to_file_path()
                    .unwrap_or_else(|_| PathBuf::from(url.path()));
                resolve_symlinks(&standardize(&path)).to_string_lossy().into_owned()
            }
            Some(_) => self.string.clone(),
            None => standardize(Path::new(&self.string)).to_string_lossy().into_owned(),
        };

        let mut result = String::with_capacity(normalized.len());
        for c in normalized.chars() {
            if is_unsafe_character(c) {
                result.push_str(&format!("\\u{{{:X}}}", c as u32));
            } else {
                result.push(c);
            }
        }
        result
    }
}

/// Whether `value` contains control, invisible or text-reversing characters. Check before
/// converting a value to a URL, since conversion can percent-encode them.
pub fn contains_unsafe_characters(value: &str) -> bool {
    value.chars().any(is_unsafe_character)
}

pub fn is_unsafe_character(c: char) -> bool {
    matches!(
        c as u32,
        // C0 and C1 controls, including CR, LF and NEL.
        0x00..=0x1F | 0x7F..=0x9F
        // Directional marks and zero-width characters.
        | 0x061C | 0x200B..=0x200F | 0x202A..=0x202E | 0x2066..=0x2069
        // Line and paragraph separators.
        | 0x2028..=0x2029
        // Word joiner and byte order mark.
        | 0x2060 | 0xFEFF
    )
}

fn scheme_of(string: &str) -> Option<&str> {
    let scheme = &string[..string.find(':')?];
    let mut chars = scheme.chars();
    let valid = chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    valid.then_some(scheme)
}

fn file_decision(url: &Url) -> Decision {
    let malformed = Decision::Deny(DenialReason::MalformedUrl);
    // A query or fragment does not identify part of a filesystem object.
    if url.query().is_some() || url.fragment().is_some() {
        return malformed;
    }
    // An empty host and localhost both mean this machine; a remote host could trigger
    // network access.
    if url
        .host_str()
        .is_some_and(|host| !host.is_empty() && !host.eq_ignore_ascii_case("localhost"))
    {
        return malformed;
    }
    let Ok(path) = url.to_file_path() else {
        return malformed;
    };

    // Classify the effective object: this collapses dot traversal and keeps a harmless
    // looking symlink from hiding an executable target.
    let Ok(canonical) = fs::canonicalize(&path) else {
        return Decision::Deny(DenialReason::InaccessibleFile);
    };
    let Ok(metadata) = fs::metadata(&canonical) else {
        return Decision::Deny(DenialReason::InaccessibleFile);
    };
    if !(metadata.is_dir() || metadata.is_file()) {
        return Decision::Deny(DenialReason::InaccessibleFile);
    }
    if is_unsafe_file(&canonical, metadata.is_dir()) {
        return Decision::Deny(DenialReason::UnsafeFile);
    }
    Decision::AllowFile(canonical)
}

fn is_unsafe_file(path: &Path, is_directory: bool) -> bool {
    let extension = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if UNSAFE_EXTENSIONS.contains(&extension.as_str())
        || UNSAFE_CONTENT_EXTENSIONS.contains(&extension.as_str())
    {
        return true;
    }
    // Any regular file the filesystem marks executable, whatever its name.
    !is_directory && is_executable(path)
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let Ok(c_path) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    unsafe { libc::access(c_path.as_ptr(), libc::X_OK) == 0 }
}

/// Windows has no executable bit; the extension lists above decide.
#[cfg(not(unix))]
fn is_executable(_path: &Path) -> bool {
    false
}

/// Lexically resolves `.`, `..` and repeated separators. A relative path is taken
/// relative to the current directory.
fn standardize(path: &Path) -> PathBuf {
    let mut result = if path.has_root() {
        PathBuf::new()
    } else {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"))
    };
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => result.push(component),
            Component::CurDir => {}
            Component::ParentDir => {
                result.pop();
            }
            Component::Normal(part) => result.push(part),
        }
    }
    result
}

/// The path with symlinks resolved, or unchanged when it does not exist.
fn resolve_symlinks(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests;
