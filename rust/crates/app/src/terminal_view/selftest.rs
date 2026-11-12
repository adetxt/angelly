//! In-process scripting for verifying the terminal view without driving the
//! system's input.
//!
//! `ANGELLY_SELFTEST_TERMINAL` holds steps separated by `@@`; the first view
//! to render runs them. Text arguments understand `\n`, `\r`, `\t`, `\e`, `\a` and
//! `\\`. Steps:
//!
//! * `wait:<ms>`
//! * `feed:<text>`: print to the terminal as if the program wrote it
//! * `send:<text>`: write raw bytes to the child
//! * `type:<text>`: press each character as a key
//! * `key:<keystroke>[#<code>]`: press a key (`cmd-f`, `ctrl-c`, `up`), optionally with the
//!   macOS virtual key code a real press carries (`key:ctrl-с#8` is Ctrl-C on a Russian layout)
//! * `ime:<text>` and `commit:<text>`: input method composition
//! * `click:<col>,<row>[,<mods>][,<count>]` (mods: `cmd` `alt` `shift` `ctrl`)
//! * `drag:<col>,<row>,<col>,<row>[,<mods>]`
//! * `move:<col>,<row>[,<mods>]` and `mods:<mods>`
//! * `wheel:<pixels>` (precise) and `notch:<lines>` (discrete)
//! * `search:<text>`, `next`, `prev`, `endsearch`
//! * `action:<keybind action>`: run a keybind action (`select_all`, `adjust_selection:left_word`,
//!   `write_scrollback_file:paste,html`)
//! * `resetstats`
//! * `dump:<what>` where what is `screen`, `selection`, `cursor`, `scroll`,
//!   `preedit`, `stats`, `link`, `search`, `password` or `flags` (cells with attributes)
//! * `quit`

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use gpui_kit::*;

use super::TerminalView;
use crate::keybinds::SearchDirection;

static CLAIMED: AtomicBool = AtomicBool::new(false);

#[derive(Debug, PartialEq)]
enum Step {
    Wait(Duration),
    Feed(String),
    Send(String),
    Type(String),
    Key(String, Option<u16>),
    Ime(String),
    Commit(String),
    Click { cell: (u16, u16), mods: Modifiers, count: usize },
    Drag { from: (u16, u16), to: (u16, u16), mods: Modifiers },
    Move { cell: (u16, u16), mods: Modifiers },
    Mods(Modifiers),
    Wheel(f32),
    Notch(f32),
    Search(String),
    Next,
    Prev,
    EndSearch,
    ResetStats,
    Action(String),
    Dump(String),
    Quit,
}

fn unescape(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('e') => out.push('\x1b'),
            Some('a') => out.push('\x07'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

fn parse_mods<'a>(parts: impl Iterator<Item = &'a str>) -> Modifiers {
    let mut m = Modifiers::default();
    for p in parts {
        match p {
            "cmd" => m.platform = true,
            "alt" => m.alt = true,
            "shift" => m.shift = true,
            "ctrl" => m.control = true,
            _ => {}
        }
    }
    m
}

fn numbers(parts: &[&str], n: usize) -> Option<Vec<u16>> {
    let nums: Vec<u16> = parts.iter().take(n).map(|p| p.trim().parse().ok()).collect::<Option<_>>()?;
    (nums.len() == n).then_some(nums)
}

fn parse_step(step: &str) -> Option<Step> {
    let (name, arg) = step.split_once(':').unwrap_or((step, ""));
    let parts: Vec<&str> = arg.split(',').collect();
    Some(match name {
        "wait" => Step::Wait(Duration::from_millis(arg.parse().ok()?)),
        "feed" => Step::Feed(unescape(arg)),
        "send" => Step::Send(unescape(arg)),
        "type" => Step::Type(unescape(arg)),
        "key" => match arg.split_once('#') {
            Some((spec, code)) => Step::Key(spec.to_string(), Some(code.parse().ok()?)),
            None => Step::Key(arg.to_string(), None),
        },
        "ime" => Step::Ime(arg.to_string()),
        "commit" => Step::Commit(arg.to_string()),
        "click" => {
            let n = numbers(&parts, 2)?;
            let count = parts.iter().skip(2).find_map(|p| p.parse().ok()).unwrap_or(1);
            Step::Click { cell: (n[0], n[1]), mods: parse_mods(parts.iter().skip(2).copied()), count }
        }
        "drag" => {
            let n = numbers(&parts, 4)?;
            Step::Drag { from: (n[0], n[1]), to: (n[2], n[3]), mods: parse_mods(parts.iter().skip(4).copied()) }
        }
        "move" => {
            let n = numbers(&parts, 2)?;
            Step::Move { cell: (n[0], n[1]), mods: parse_mods(parts.iter().skip(2).copied()) }
        }
        "mods" => Step::Mods(parse_mods(parts.iter().copied())),
        "wheel" => Step::Wheel(arg.parse().ok()?),
        "notch" => Step::Notch(arg.parse().ok()?),
        "search" => Step::Search(arg.to_string()),
        "next" => Step::Next,
        "prev" => Step::Prev,
        "endsearch" => Step::EndSearch,
        "resetstats" => Step::ResetStats,
        "action" => Step::Action(arg.to_string()),
        "dump" => Step::Dump(arg.to_string()),
        "quit" => Step::Quit,
        _ => return None,
    })
}

fn parse(script: &str) -> Vec<Step> {
    script
        .split("@@")
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .filter_map(|s| {
            let step = parse_step(s);
            if step.is_none() {
                eprintln!("[selftest] ignoring step {s:?}");
            }
            step
        })
        .collect()
}

fn keystroke(spec: &str) -> Option<Keystroke> {
    let mut ks = Keystroke::parse_with_os(spec, OperatingSystem::Mac).ok()?;
    let m = ks.modifiers;
    if ks.key_char.is_none() && !m.control && !m.platform && ks.key.chars().count() == 1 {
        ks.key_char = Some(ks.key.clone());
    }
    Some(ks)
}

/// Starts the script named by `ANGELLY_SELFTEST_TERMINAL`, once per process.
pub fn start(_: &mut TerminalView, window: &mut Window, cx: &mut Context<TerminalView>) {
    let Ok(script) = std::env::var("ANGELLY_SELFTEST_TERMINAL") else { return };
    if CLAIMED.swap(true, Ordering::SeqCst) {
        return;
    }
    let steps = parse(&script);
    let task = cx.spawn_in(window, async move |this, cx| {
        for step in steps {
            let wait = if let Step::Wait(d) = &step { Some(*d) } else { None };
            if let Some(d) = wait {
                cx.background_executor().timer(d).await;
                continue;
            }
            let quit = matches!(step, Step::Quit);
            if this.update_in(cx, |this, window, cx| this.run_step(step, window, cx)).is_err() {
                break;
            }
            if quit {
                break;
            }
            cx.background_executor().timer(Duration::from_millis(30)).await;
        }
        eprintln!("[selftest] script finished");
    });
    task.detach();
}

impl TerminalView {
    fn cell_position(&self, (col, row): (u16, u16)) -> Point<Pixels> {
        let Some(m) = self.metrics else { return self.layout.origin };
        point(
            self.layout.origin.x + m.cell_width * (f32::from(col) + 0.5),
            self.layout.origin.y + m.cell_height * (f32::from(row) + 0.5),
        )
    }

    fn run_step(&mut self, step: Step, window: &mut Window, cx: &mut Context<Self>) {
        match step {
            Step::Wait(_) => {}
            Step::Feed(text) => {
                self.session.term().write(text.as_bytes());
                self.on_wake(cx);
            }
            Step::Send(text) => self.session.write(text.as_bytes()),
            Step::Type(text) => {
                for c in text.chars() {
                    let spec = match c {
                        '\n' | '\r' => "enter".to_string(),
                        '\t' => "tab".to_string(),
                        ' ' => "space".to_string(),
                        c => c.to_string(),
                    };
                    if let Some(mut ks) = keystroke(&spec) {
                        if spec == "space" {
                            ks.key_char = Some(" ".into());
                        }
                        self.press(ks, window, cx);
                    }
                }
            }
            Step::Key(spec, code) => match keystroke(&spec) {
                Some(ks) => self.key_down(ks, false, code, window, cx),
                None => eprintln!("[selftest] bad key {spec:?}"),
            },
            Step::Ime(text) => {
                let end = text.encode_utf16().count();
                EntityInputHandler::replace_and_mark_text_in_range(self, None, &text, Some(end..end), window, cx);
            }
            Step::Commit(text) => EntityInputHandler::replace_text_in_range(self, None, &text, window, cx),
            Step::Click { cell, mods, count } => {
                let position = self.cell_position(cell);
                for n in 1..=count {
                    self.mouse_down(
                        &MouseDownEvent {
                            button: MouseButton::Left,
                            position,
                            modifiers: mods,
                            click_count: n,
                            ..Default::default()
                        },
                        window,
                        cx,
                    );
                    self.mouse_up(
                        &MouseUpEvent { button: MouseButton::Left, position, modifiers: mods, click_count: n },
                        window,
                        cx,
                    );
                }
            }
            Step::Drag { from, to, mods } => {
                let (a, b) = (self.cell_position(from), self.cell_position(to));
                self.mouse_down(
                    &MouseDownEvent {
                        button: MouseButton::Left,
                        position: a,
                        modifiers: mods,
                        click_count: 1,
                        ..Default::default()
                    },
                    window,
                    cx,
                );
                self.mouse_move(
                    &MouseMoveEvent { position: b, pressed_button: Some(MouseButton::Left), modifiers: mods },
                    true,
                    cx,
                );
                self.mouse_up(
                    &MouseUpEvent { button: MouseButton::Left, position: b, modifiers: mods, click_count: 1 },
                    window,
                    cx,
                );
            }
            Step::Move { cell, mods } => {
                let position = self.cell_position(cell);
                self.mouse_move(&MouseMoveEvent { position, pressed_button: None, modifiers: mods }, true, cx);
            }
            Step::Mods(mods) => {
                self.on_modifiers_changed(&ModifiersChangedEvent { modifiers: mods, ..Default::default() }, window, cx)
            }
            Step::Wheel(pixels) => self.scroll_wheel(
                &ScrollEvent {
                    position: self.layout.origin,
                    delta: ScrollDelta::Pixels(point(px(0.), px(pixels))),
                    ..Default::default()
                },
                cx,
            ),
            Step::Notch(lines) => self.scroll_wheel(
                &ScrollEvent {
                    position: self.layout.origin,
                    delta: ScrollDelta::Lines(point(0.0, lines)),
                    ..Default::default()
                },
                cx,
            ),
            Step::Search(text) => self.start_search(Some(text), window, cx),
            Step::Next => {
                self.navigate_search(SearchDirection::Next, cx);
            }
            Step::Prev => {
                self.navigate_search(SearchDirection::Previous, cx);
            }
            Step::EndSearch => {
                self.end_search(window, cx);
            }
            Step::ResetStats => self.stats = Default::default(),
            Step::Action(text) => {
                let action = angelly_core::config::Action::parse(&text).expect("selftest action parses");
                let command = crate::keybinds::Command::parse(&action).expect("selftest action is known");
                self.run_command(command, &action, window, cx);
            }
            Step::Dump(what) => self.dump(&what),
            Step::Quit => cx.quit(),
        }
    }

    fn press(&mut self, keystroke: Keystroke, window: &mut Window, cx: &mut Context<Self>) {
        self.on_key_down(&KeyDownEvent { keystroke, is_held: false, prefer_character_input: false }, window, cx);
    }

    fn dump(&self, what: &str) {
        match what {
            "screen" => eprintln!("[selftest] screen\n{}", self.frame.text()),
            "selection" => eprintln!("[selftest] selection {:?}", self.session.term().selection_text()),
            "password" => eprintln!(
                "[selftest] password prompt {} secure input requested {}",
                self.session.password_input(),
                self.secure_input
            ),
            "cursor" => eprintln!("[selftest] cursor {:?} grid {:?}", self.frame.cursor, self.layout.grid),
            "scroll" => eprintln!("[selftest] scrollbar {:?}", self.frame.scrollbar),
            "preedit" => eprintln!("[selftest] preedit {:?}", self.preedit),
            "link" => eprintln!("[selftest] link {:?}", self.hover_link),
            "search" => {
                let mut term = self.session.term();
                eprintln!(
                    "[selftest] search open {} searching {} status {:?} visible {} selected {:?}",
                    self.search.is_some(),
                    term.is_searching(),
                    term.search_poll(),
                    term.search_viewport_matches().len(),
                    term.search_selected_match()
                );
            }
            "flags" => {
                for (y, row) in self.frame.rows.iter().enumerate() {
                    for (x, c) in row.cells.iter().enumerate() {
                        if !c.flags.is_empty() || c.underline != terminal::Underline::None {
                            eprintln!(
                                "[selftest] cell {x},{y} {:?} flags {:?} underline {:?}",
                                c.ch, c.flags, c.underline
                            );
                        }
                    }
                }
            }
            "stats" => {
                let s = &self.stats;
                let frames = s.frames.max(1);
                eprintln!(
                    "[selftest] frames {} avg {:.2}ms worst {:.2}ms rows_built {} ({:.1}/frame)",
                    s.frames,
                    s.total.as_secs_f64() * 1000.0 / f64::from(frames),
                    s.worst.as_secs_f64() * 1000.0,
                    s.rows_built,
                    s.rows_built as f64 / f64::from(frames),
                );
            }
            other => eprintln!("[selftest] unknown dump {other:?}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::prelude::v1::test;

    use super::*;

    #[test]
    fn parses_steps_and_escapes() {
        let steps = parse("wait:100@@send:ls\\r@@click:3,4,cmd,2@@bogus@@quit");
        assert_eq!(
            steps,
            vec![
                Step::Wait(Duration::from_millis(100)),
                Step::Send("ls\r".into()),
                Step::Click { cell: (3, 4), mods: Modifiers { platform: true, ..Default::default() }, count: 2 },
                Step::Quit,
            ]
        );
    }

    #[test]
    fn drag_needs_four_numbers() {
        assert!(parse_step("drag:1,2,3").is_none());
        assert_eq!(
            parse_step("drag:1,2,3,4,alt"),
            Some(Step::Drag { from: (1, 2), to: (3, 4), mods: Modifiers { alt: true, ..Default::default() } })
        );
    }

    #[test]
    fn plain_letters_carry_their_character() {
        let ks = keystroke("a").unwrap();
        assert_eq!(ks.key_char.as_deref(), Some("a"));
        assert_eq!(keystroke("cmd-f").unwrap().key_char, None);
    }
}
