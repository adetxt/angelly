use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use terminal::*;

fn sh(script: &str) -> SessionConfig {
    SessionConfig {
        command: Some(vec!["/bin/sh".into(), "-c".into(), script.into()]),
        cwd: Some(PathBuf::from("/")),
        ..SessionConfig::default()
    }
}

fn wait_for(what: &str, mut cond: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !cond() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn screen(s: &Session) -> String {
    let mut f = Frame::new();
    s.term().update_frame(&mut f);
    f.text()
}

fn wait_exit(s: &Session) -> ExitStatus {
    wait_for("child exit", || s.exit_status().is_some());
    s.exit_status().unwrap()
}

#[test]
fn echo_hi() {
    let wakes = Arc::new(AtomicUsize::new(0));
    let w = wakes.clone();
    let s = Session::spawn(sh("echo hi"), move || {
        w.fetch_add(1, Ordering::Relaxed);
    })
    .unwrap();
    assert_eq!(
        wait_exit(&s),
        ExitStatus {
            code: 0,
            signal: None
        }
    );
    assert!(screen(&s).starts_with("hi"), "{:?}", screen(&s));
    assert!(wakes.load(Ordering::Relaxed) >= 1);
}

#[test]
fn cwd_follows_shell_reports_when_leaving_and_returning_to_a_repository() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let repo = home.join("angelly");
    std::fs::create_dir_all(&repo).unwrap();
    let s = Session::spawn(sh(r#"while IFS= read -r directory; do printf '\033]7;kitty-shell-cwd://localhost%s\007' "$directory"; done"#), || {}).unwrap();
    for directory in [&repo, &home, &repo] {
        s.write(format!("{}\n", directory.display()).as_bytes());
        wait_for("reported working directory", || s.cwd().as_ref() == Some(directory));
    }
}

#[test]
fn exit_code() {
    let s = Session::spawn(sh("exit 7"), || {}).unwrap();
    let st = wait_exit(&s);
    assert_eq!(st.code, 7);
    assert!(!st.success());
}

#[test]
fn environment_and_cwd() {
    let mut cfg = sh("echo \"$TERM|$TERM_PROGRAM|$COLORTERM|$FOO|$(pwd)\"");
    cfg.env.push(("FOO".into(), "bar".into()));
    let s = Session::spawn(cfg, || {}).unwrap();
    wait_exit(&s);
    assert!(
        screen(&s).starts_with("xterm-256color|ghostty|truecolor|bar|/"),
        "{:?}",
        screen(&s)
    );
}

fn bundle(terminfo: bool) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("ghostty")).unwrap();
    if terminfo {
        std::fs::create_dir_all(root.path().join("terminfo")).unwrap();
    }
    root
}

#[test]
fn xterm_ghostty_when_the_bundle_has_terminfo() {
    let root = bundle(true);
    let mut cfg = sh("echo $TERM; echo $TERMINFO; echo $GHOSTTY_RESOURCES_DIR");
    cfg.resources_dir = Some(root.path().join("ghostty"));
    let s = Session::spawn(cfg, || {}).unwrap();
    wait_exit(&s);
    let expected = format!(
        "xterm-ghostty\n{0}/terminfo\n{0}/ghostty",
        root.path().display()
    );
    assert!(screen(&s).starts_with(&expected), "{:?}", screen(&s));
}

#[test]
fn xterm_256color_when_the_bundle_lacks_terminfo() {
    let root = bundle(false);
    let mut cfg = sh("echo \"$TERM|$TERMINFO\"");
    cfg.resources_dir = Some(root.path().join("ghostty"));
    let s = Session::spawn(cfg, || {}).unwrap();
    wait_exit(&s);
    assert!(screen(&s).starts_with("xterm-256color|"), "{:?}", screen(&s));
}

/// Ghostty's own shell integration scripts, loaded by a real interactive zsh through the
/// injected `ZDOTDIR`, must produce OSC 133 prompt marks and OSC 7 working directory reports.
#[cfg(unix)]
#[test]
fn zsh_integration_reports_prompts_and_the_working_directory() {
    let zsh = std::path::Path::new("/bin/zsh");
    if !zsh.exists() {
        return;
    }
    let home = tempfile::tempdir().unwrap();
    let cfg = SessionConfig {
        command: Some(vec![zsh.to_string_lossy().into_owned(), "-i".into()]),
        cwd: Some(home.path().to_path_buf()),
        env: vec![("HOME".into(), home.path().to_string_lossy().into_owned())],
        resources_dir: Some(PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../src"))),
        ..SessionConfig::default()
    };
    let s = Session::spawn(cfg, || {}).unwrap();
    let mut seen_prompt = false;
    wait_for("an OSC 133 prompt mark", || {
        for event in s.term().take_events() {
            seen_prompt |= matches!(event, Event::SemanticPrompt { kind: PromptKind::PromptStart, .. });
        }
        seen_prompt
    });
    wait_for("an OSC 7 report", || s.term().pwd().is_some());
    s.write(b"exit\n");
    wait_exit(&s);
}

#[test]
fn input_reaches_child() {
    let s = Session::spawn(sh("read line; echo \"got:$line\""), || {}).unwrap();
    s.write(b"ping\r");
    wait_exit(&s);
    assert!(screen(&s).contains("got:ping"), "{:?}", screen(&s));
}

#[test]
fn key_encoder_feeds_child() {
    let s = Session::spawn(sh("stty raw -echo; head -c 3 | od -An -tx1"), || {}).unwrap();
    let up = s
        .term()
        .encode_key(&KeyEvent::press(Key::ArrowUp, Mods::default()));
    s.write(&up);
    wait_exit(&s);
    let hex = screen(&s)
        .split_whitespace()
        .skip(1)
        .collect::<Vec<_>>()
        .join(" ");
    assert_eq!(hex, "1b 5b 41", "{:?}", screen(&s));
}

#[test]
fn resize_reaches_child() {
    let s = Session::spawn(sh("sleep 0.3; stty size"), || {}).unwrap();
    s.resize(100, 30, 9, 18);
    wait_exit(&s);
    assert!(screen(&s).contains("30 100"), "{:?}", screen(&s));
    let size = s.term().size();
    assert_eq!((size.cols, size.rows), (100, 30));
}

#[test]
fn terminal_replies_reach_child() {
    // The child asks for device attributes and a cursor position report and
    // reads the replies back from its stdin.
    let script = r#"stty raw -echo
printf '\033[c'
head -c 12 | od -An -c
printf '\033[6n'
head -c 6 | od -An -c"#;
    let s = Session::spawn(sh(script), || {}).unwrap();
    wait_exit(&s);
    let text = screen(&s);
    assert!(
        text.contains("?   6   2   ;   2   2   ;   5   2   c"),
        "{text:?}"
    );
    assert!(text.contains("[   "), "{text:?}");
}

#[test]
fn events_surface_through_session() {
    let s = Session::spawn(
        sh(r"printf '\033]2;hello\007\033]7;file:///tmp/x\007'"),
        || {},
    )
    .unwrap();
    wait_exit(&s);
    let events = s.term().take_events();
    assert!(events.contains(&Event::Title("hello".into())), "{events:?}");
    assert_eq!(s.cwd(), Some(PathBuf::from("/tmp/x")));
}

#[test]
fn foreground_process_and_kill() {
    let s = Session::spawn(sh("sleep 30"), || {}).unwrap();
    wait_for("sleep to be foreground", || {
        s.foreground_process().is_some_and(|p| p.name == "sleep")
    });
    let p = s.foreground_process().unwrap();
    assert_eq!(p.cwd.as_deref().and_then(|p| p.to_str()), Some("/"));
    assert!(s.exit_status().is_none());
    s.kill();
    let st = wait_exit(&s);
    assert!(!st.success());
}

#[test]
fn login_shell_default() {
    let s = Session::spawn(
        SessionConfig {
            cwd: Some(PathBuf::from("/")),
            ..SessionConfig::default()
        },
        || {},
    )
    .unwrap();
    s.write(b"echo $((20+22))\rexit 5\r");
    let st = wait_exit(&s);
    assert_eq!(st.code, 5);
    assert!(
        screen(&s).contains("\n42") || screen(&s).starts_with("42"),
        "{:?}",
        screen(&s)
    );
}

#[test]
fn wake_is_coalesced_until_term_is_locked() {
    let wakes = Arc::new(AtomicUsize::new(0));
    let w = wakes.clone();
    let s = Session::spawn(
        sh("for i in 1 2 3 4 5 6 7 8 9 10; do echo $i; done"),
        move || {
            w.fetch_add(1, Ordering::Relaxed);
        },
    )
    .unwrap();
    wait_for("exit", || s.exit_status().is_some());
    let before = wakes.load(Ordering::Relaxed);
    assert!(before >= 1);
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(
        wakes.load(Ordering::Relaxed),
        before,
        "no further wakes while one is pending"
    );
    drop(s.term());
    s.write(b"");
}
