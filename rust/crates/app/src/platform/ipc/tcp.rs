//! Loopback TCP transport for platforms without Unix sockets in the standard library.
//!
//! The endpoint is a file holding the port and a random token. A client reads it, connects
//! to `127.0.0.1:<port>` and sends the token as its first line; a connection that does not
//! is dropped. The file lives in the user's profile, so other users cannot read the token.

use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use angelly_core::config::Env;
use serde_json::json;
use uuid::Uuid;

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(2);

#[cfg_attr(not(windows), allow(dead_code))]
pub fn default_endpoint() -> PathBuf {
    let directory = Env::from_process().data_dir().unwrap_or_else(std::env::temp_dir);
    directory.join("ipc.json")
}

pub struct Listener {
    listener: TcpListener,
    token: String,
}

impl Listener {
    pub fn bind(path: &Path) -> io::Result<Self> {
        if Stream::connect(path).is_ok() {
            return Err(io::Error::new(io::ErrorKind::AddrInUse, "another Angelly is listening"));
        }
        if let Some(directory) = path.parent() {
            std::fs::create_dir_all(directory)?;
        }
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        let endpoint = json!({ "port": listener.local_addr()?.port(), "token": token });
        std::fs::write(path, endpoint.to_string())?;
        Ok(Self { listener, token })
    }

    /// The next authenticated connection, or `None` when the listener is gone.
    pub fn accept(&self) -> Option<Stream> {
        loop {
            let stream = match self.listener.accept() {
                Ok((stream, _)) => stream,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) => return None,
            };
            if stream.set_read_timeout(Some(HANDSHAKE_TIMEOUT)).is_err() {
                continue;
            }
            // Unbuffered, so no byte of the first request is consumed with the token.
            let mut line = Vec::new();
            let mut byte = [0u8; 1];
            let complete = loop {
                match (&stream).read(&mut byte) {
                    Ok(1) if byte[0] == b'\n' => break true,
                    Ok(1) if line.len() < self.token.len() + 2 => line.push(byte[0]),
                    _ => break false,
                }
            };
            if complete && line.strip_suffix(b"\r").unwrap_or(&line) == self.token.as_bytes() && stream.set_read_timeout(None).is_ok() {
                return Some(Stream(stream));
            }
        }
    }
}

pub struct Stream(TcpStream);

impl Stream {
    pub fn connect(path: &Path) -> io::Result<Self> {
        let endpoint: serde_json::Value = serde_json::from_slice(&std::fs::read(path)?)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let invalid = || io::Error::new(io::ErrorKind::InvalidData, "malformed endpoint file");
        let port = endpoint["port"].as_u64().and_then(|p| u16::try_from(p).ok()).ok_or_else(invalid)?;
        let token = endpoint["token"].as_str().ok_or_else(invalid)?;
        let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port))?;
        writeln!(stream, "{token}")?;
        Ok(Self(stream))
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
    use std::io::{BufRead, BufReader};

    #[test]
    fn an_authenticated_client_gets_its_reply() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("run/ipc.json");
        let listener = Listener::bind(&path).unwrap();
        std::thread::spawn(move || {
            let stream = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap()).read_line(&mut line).unwrap();
            writeln!({ stream }, "echo:{}", line.trim_end()).unwrap();
        });
        let mut client = Stream::connect(&path).unwrap();
        writeln!(client, "ping").unwrap();
        let mut reply = String::new();
        BufReader::new(client).read_line(&mut reply).unwrap();
        assert_eq!(reply, "echo:ping\n");
        assert_eq!(Listener::bind(&path).err().map(|e| e.kind()), None);
    }

    #[test]
    fn a_client_without_the_token_is_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ipc.json");
        let listener = Listener::bind(&path).unwrap();
        let port = serde_json::from_slice::<serde_json::Value>(&std::fs::read(&path).unwrap()).unwrap()["port"]
            .as_u64()
            .unwrap() as u16;
        std::thread::spawn(move || {
            let _ = listener.accept();
        });
        let mut intruder = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        writeln!(intruder, "wrong").unwrap();
        let mut reply = String::new();
        assert_eq!(BufReader::new(intruder).read_line(&mut reply).unwrap_or(0), 0);
    }
}
