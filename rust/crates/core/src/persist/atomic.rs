use std::fs::{self, DirBuilder, OpenOptions};
use std::io::{self, Write};
use std::path::Path;

use uuid::Uuid;

/// Writes `contents` to `path` through a temporary file and a rename, so a failed write
/// leaves the previous file intact. On Unix the directory is created owner-only (0700) and
/// the file is owner-only (0600); on Windows both inherit the owner-only ACL of the
/// profile directory they live in.
pub(crate) fn write_private_atomic(path: &Path, contents: &[u8]) -> io::Result<()> {
    let directory = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    create_private_directory(directory)?;

    let temporary = directory.join(Uuid::new_v4().to_string());
    let result = (|| {
        let mut file = private_file_options().open(&temporary)?;
        file.write_all(contents)?;
        file.sync_all()?;
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(unix)]
fn create_private_directory(directory: &Path) -> io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    DirBuilder::new().recursive(true).mode(0o700).create(directory)?;
    fs::set_permissions(directory, fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn create_private_directory(directory: &Path) -> io::Result<()> {
    DirBuilder::new().recursive(true).create(directory)
}

#[cfg(unix)]
fn private_file_options() -> OpenOptions {
    use std::os::unix::fs::OpenOptionsExt;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true).mode(0o600);
    options
}

#[cfg(not(unix))]
fn private_file_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    options
}
