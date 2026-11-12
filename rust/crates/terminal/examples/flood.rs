//! Throughput check: `cargo run --release -p terminal --example flood [MiB]`.
//!
//! Measures the VT parser alone on in-memory data, then the full PTY path
//! (`yes <line> | head -c N` through a child shell, reader thread and parser).
//! Plain `yes` emits 2-byte lines, which macOS ptys deliver one read per line
//! (~5 bytes each), so the kernel rather than the terminal is the bottleneck.

use std::{
    sync::mpsc,
    time::{Duration, Instant},
};

use terminal::{Frame, Session, SessionConfig, Term};

fn main() {
    let mib: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(200);
    let total = mib << 20;

    let mut term = Term::new(200, 50);
    term.resize(200, 50, 8, 16);
    let line = b"the quick brown fox jumps over the lazy dog 0123456789 \x1b[31mred\x1b[0m\r\n";
    let chunk: Vec<u8> = line.iter().copied().cycle().take(64 * 1024).collect();
    let start = Instant::now();
    let mut fed = 0;
    while fed < total {
        term.write(&chunk);
        fed += chunk.len();
    }
    report("parser only", fed, start.elapsed());

    let mut frame = Frame::new();
    let start = Instant::now();
    for _ in 0..200 {
        term.write(&chunk[..4096]);
        term.update_frame(&mut frame);
    }
    println!(
        "write 4 KiB + update_frame: {:.2} ms each",
        start.elapsed().as_secs_f64() * 1000.0 / 200.0
    );

    let (tx, rx) = mpsc::channel();
    let cfg = SessionConfig {
        cols: 200,
        rows: 50,
        command: Some(vec!["/bin/sh".into(), "-c".into(), format!("yes 'the quick brown fox jumps over the lazy dog 0123456789 the quick brown fox' | head -c {total}")]),
        ..SessionConfig::default()
    };
    let start = Instant::now();
    let session = Session::spawn(cfg, move || {
        let _ = tx.send(());
    })
    .expect("spawn");
    let mut frame = Frame::new();
    let mut frames = 0u32;
    while session.exit_status().is_none() {
        if rx.recv_timeout(Duration::from_millis(16)).is_ok() {
            session.term().update_frame(&mut frame);
            frames += 1;
            // A real view redraws at most once per display frame.
            std::thread::sleep(Duration::from_millis(16));
        }
    }
    report("pty -> parser", total, start.elapsed());
    println!("frames drawn while flooding: {frames}");
}

fn report(what: &str, bytes: usize, took: Duration) {
    let mib = bytes as f64 / (1 << 20) as f64;
    println!(
        "{what}: {mib:.0} MiB in {:.2}s = {:.0} MiB/s",
        took.as_secs_f64(),
        mib / took.as_secs_f64()
    );
}
