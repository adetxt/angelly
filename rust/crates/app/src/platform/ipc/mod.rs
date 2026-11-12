//! A local control channel for platforms without Apple events. A client writes one JSON
//! request per line and reads one JSON response per line; the requests are the ones
//! AppleScript and Shortcuts use (see [`automation`](super::automation)). `angelly msg`
//! is the command line client.
//!
//! On Unix the channel is a socket in the user's runtime directory; on Windows it is a
//! loopback TCP port guarded by a token that only the user's profile can read. It runs by
//! default on Linux and Windows. On macOS, where AppleScript is the interface, it only
//! starts when `ANGELLY_IPC` is set.

use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;

use gpui_kit::App;

use super::{automation, command};

#[cfg(any(windows, test))]
mod tcp;
#[cfg(unix)]
mod unix;

#[cfg(unix)]
use unix as transport;
#[cfg(windows)]
use tcp as transport;

use transport::{Listener, Stream};

/// `$ANGELLY_SOCKET`, else the platform's default endpoint.
pub fn socket_path() -> PathBuf {
    std::env::var_os("ANGELLY_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(transport::default_endpoint)
}

/// Answers every request line of every connection with `handle`, one connection at a time.
fn serve(listener: Listener, handle: impl Fn(String) -> String + Send + 'static) {
    thread::spawn(move || {
        while let Some(stream) = listener.accept() {
            let Ok(reader) = stream.try_clone().map(BufReader::new) else { continue };
            let mut writer = stream;
            for line in reader.lines().map_while(Result::ok).filter(|line| !line.trim().is_empty()) {
                if writeln!(writer, "{}", handle(line)).is_err() {
                    break;
                }
            }
        }
    });
}

/// Starts the server when it is enabled for this platform.
pub fn start(cx: &mut App) {
    if cfg!(target_os = "macos") && std::env::var_os("ANGELLY_IPC").is_none() {
        return;
    }
    let listener = match Listener::bind(&socket_path()) {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("angelly: control channel unavailable: {error}");
            return;
        }
    };
    let (requests, queue) = async_channel::unbounded::<(String, mpsc::Sender<String>)>();
    cx.spawn(async move |cx| {
        while let Ok((line, reply)) = queue.recv().await {
            let response = cx.update(|cx| match command::host(cx) {
                Some(host) => automation::handle_json(&*host, &line, cx),
                None => r#"{"ok":false,"error":"Angelly is starting."}"#.to_owned(),
            });
            let _ = reply.send(response);
        }
    })
    .detach();
    serve(listener, move |line| {
        let (reply, response) = mpsc::channel();
        match requests.send_blocking((line, reply)) {
            Ok(()) => response.recv().unwrap_or_else(|_| r#"{"ok":false,"error":"Angelly quit."}"#.to_owned()),
            Err(_) => r#"{"ok":false,"error":"Angelly quit."}"#.to_owned(),
        }
    });
}

/// Sends one request line to the running application and returns its response line.
pub fn send(path: &Path, request: &str) -> io::Result<String> {
    let mut stream = Stream::connect(path)?;
    writeln!(stream, "{}", request.trim())?;
    let mut response = String::new();
    BufReader::new(stream).read_line(&mut response)?;
    Ok(response.trim_end().to_owned())
}

/// `angelly msg <json | command>`: prints the response and returns the exit code. `None`
/// when the arguments are not a `msg` invocation and the application should start.
pub fn run_cli(args: &[String]) -> Option<i32> {
    if args.get(1).map(String::as_str) != Some("msg") {
        return None;
    }
    let Some(request) = args.get(2) else {
        eprintln!("usage: angelly msg '<json request>' | angelly msg <command>");
        return Some(2);
    };
    let request = if request.trim_start().starts_with('{') {
        request.clone()
    } else {
        serde_json::json!({ "cmd": request }).to_string()
    };
    Some(match send(&socket_path(), &request) {
        Ok(response) => {
            println!("{response}");
            let ok = serde_json::from_str::<serde_json::Value>(&response).is_ok_and(|value| value["ok"] == true);
            if ok { 0 } else { 1 }
        }
        Err(error) => {
            eprintln!("angelly: cannot reach a running Angelly through {}: {error}", socket_path().display());
            1
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_round_trip_over_the_channel() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("run/ipc.sock");
        serve(Listener::bind(&path).unwrap(), |line| format!("echo:{line}"));
        assert_eq!(send(&path, r#"{"cmd":"snapshot"}"#).unwrap(), r#"echo:{"cmd":"snapshot"}"#);
        assert_eq!(send(&path, "again").unwrap(), "echo:again");
    }

    #[test]
    fn a_stale_endpoint_is_replaced_but_a_live_one_is_not() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ipc.sock");
        drop(Listener::bind(&path).unwrap());
        let live = Listener::bind(&path).unwrap();
        assert_eq!(Listener::bind(&path).err().unwrap().kind(), io::ErrorKind::AddrInUse);
        drop(live);
    }

    #[test]
    fn only_msg_invocations_are_handled_as_cli() {
        let args = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(run_cli(&args(&["angelly"])), None);
        assert_eq!(run_cli(&args(&["angelly", "/tmp"])), None);
        assert_eq!(run_cli(&args(&["angelly", "msg"])), Some(2));
    }
}
