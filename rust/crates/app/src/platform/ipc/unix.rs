//! Unix domain socket transport: `$XDG_RUNTIME_DIR/angelly/ipc.sock`, mode 0600 in a 0700
//! directory.

use std::io::{self, Read, Write};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};

pub fn default_endpoint() -> PathBuf {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join(format!("angelly-{}", unsafe { libc::getuid() })));
    runtime.join("angelly").join("ipc.sock")
}

pub struct Listener(UnixListener);

impl Listener {
    pub fn bind(path: &Path) -> io::Result<Self> {
        if let Some(directory) = path.parent() {
            std::fs::DirBuilder::new().recursive(true).mode(0o700).create(directory)?;
        }
        if UnixStream::connect(path).is_ok() {
            return Err(io::Error::new(io::ErrorKind::AddrInUse, "another Angelly is listening"));
        }
        let _ = std::fs::remove_file(path);
        let listener = UnixListener::bind(path)?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        Ok(Self(listener))
    }

    /// The next connection, or `None` when the listener is gone.
    pub fn accept(&self) -> Option<Stream> {
        loop {
            match self.0.accept() {
                Ok((stream, _)) => return Some(Stream(stream)),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) => return None,
            }
        }
    }
}

pub struct Stream(UnixStream);

impl Stream {
    pub fn connect(path: &Path) -> io::Result<Self> {
        UnixStream::connect(path).map(Self)
    }

    pub fn try_clone(&self) -> io::Result<Self> {
        self.0.try_clone().map(Self)
    }
}

impl Read for Stream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }
}

impl Write for Stream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_socket_is_private_to_the_user() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("run/ipc.sock");
        let _listener = Listener::bind(&path).unwrap();
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(path.parent().unwrap()), 0o700);
    }
}
