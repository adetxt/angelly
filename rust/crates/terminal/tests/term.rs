use terminal::*;

const CELL_W: u32 = 10;
const CELL_H: u32 = 20;

fn term(cols: u16, rows: u16) -> Term {
    let mut t = Term::new(cols, rows);
    t.resize(cols, rows, CELL_W, CELL_H);
    t
}

fn frame(t: &mut Term) -> Frame {
    let mut f = Frame::new();
    t.update_frame(&mut f);
    f
}

fn feed(t: &mut Term, s: &str) {
    t.write(s.as_bytes());
}

fn text(t: &mut Term) -> String {
    frame(t).text()
}

#[test]
fn kitty_images_terminal_identity_enables_discordo_detection() {
    let mut t = term(10, 5);
    feed(&mut t, "\x1b[>q");
    assert_eq!(t.take_pty_output(), b"\x1bP>|ghostty 0.0.0\x1b\\");
    t.set_program_version("0.18.0");
    feed(&mut t, "\x1b[>q");
    assert_eq!(t.take_pty_output(), b"\x1bP>|ghostty 0.18.0\x1b\\");
}

#[test]
fn kitty_images_discordo_placeholders_preserve_pixels() {
    use std::sync::Arc;
    let mut t = term(10, 5);
    // Discordo's tview widget sends chunked RGBA and a virtual placement, then
    // encodes image ID 8 and the source row/column in Unicode placeholders.
    feed(&mut t, "\x1b_Ga=T,U=1,f=32,s=2,v=1,q=2,i=8,c=2,r=1,m=1;/wAA\x1b\\");
    feed(&mut t, "\x1b_Gm=0;gAD/AP8=\x1b\\");
    feed(&mut t, "\x1b[2;4H\x1b[38;5;8m\u{10eeee}\u{305}\u{305}\u{305}\u{10eeee}\u{305}\u{30d}\u{305}\x1b[0m");
    let mut f = frame(&mut t);
    assert_eq!(f.images.len(), 1);
    let p = &f.images[0];
    assert_eq!((p.col, p.row), (3, 1));
    assert_eq!(p.size, (20, 10));
    assert_eq!(p.source, (0, 0, 2, 1));
    assert_eq!((p.image.width, p.image.height), (2, 1));
    assert_eq!(p.image.rgba, [255, 0, 0, 128, 0, 255, 0, 255]);
    let pixels = p.image.clone();
    assert!(!t.update_frame(&mut f));
    assert!(Arc::ptr_eq(&pixels, &f.images[0].image));
    feed(&mut t, "\x1b[4;1Htext");
    assert!(t.update_frame(&mut f));
    assert!(Arc::ptr_eq(&pixels, &f.images[0].image));
    feed(&mut t, "\x1b[2;4H  ");
    assert!(t.update_frame(&mut f));
    assert!(f.images.is_empty());
}

#[test]
fn kitty_images_fixed_placement_updates_scrolls_and_deletes() {
    let mut t = term(10, 5);
    feed(&mut t, "\x1b[3;2H\x1b_Ga=T,f=24,s=1,v=1,i=1,c=2,r=2,q=2,z=-1;/wAA\x1b\\");
    let mut f = frame(&mut t);
    assert_eq!(f.images.len(), 1);
    assert_eq!(f.images[0].layer, ImageLayer::BelowText);
    assert_eq!((f.images[0].col, f.images[0].row), (1, 2));
    assert_eq!(f.images[0].size, (20, 40));
    assert_eq!(f.images[0].image.rgba, [255, 0, 0, 255]);
    let generation = f.images[0].image.generation;
    feed(&mut t, "\x1b[3;2H\x1b_Ga=T,f=24,s=1,v=1,i=1,c=2,r=2,q=2,z=-1;AAD/\x1b\\");
    assert!(t.update_frame(&mut f));
    assert_ne!(f.images[0].image.generation, generation);
    assert_eq!(f.images[0].image.rgba, [0, 0, 255, 255]);
    feed(&mut t, "\x1b[5;1H\r\n");
    assert!(t.update_frame(&mut f));
    assert_eq!(f.images[0].row, 1);
    feed(&mut t, "\x1b_Ga=d,d=I,i=1,q=2\x1b\\");
    assert!(t.update_frame(&mut f));
    assert!(f.images.is_empty());
}

#[test]
fn plain_text_and_cursor() {
    let mut t = term(20, 5);
    feed(&mut t, "hello\r\nworld");
    let f = frame(&mut t);
    assert_eq!(f.rows[0].text(), "hello");
    assert_eq!(f.rows[1].text(), "world");
    assert_eq!(f.cursor.pos, Some((5, 1)));
    assert!(f.cursor.visible);
    assert_eq!(f.cursor.style, CursorStyle::Block);
}

#[test]
fn cursor_visibility_and_style() {
    let mut t = term(20, 5);
    feed(&mut t, "\x1b[?25l");
    assert!(!frame(&mut t).cursor.visible);
    feed(&mut t, "\x1b[?25h\x1b[5 q");
    let f = frame(&mut t);
    assert!(f.cursor.visible);
    assert_eq!(f.cursor.style, CursorStyle::Bar);
    assert!(f.cursor.blinking);
    feed(&mut t, "\x1b[4 q");
    let f = frame(&mut t);
    assert_eq!(f.cursor.style, CursorStyle::Underline);
    assert!(!f.cursor.blinking);
}

#[test]
fn sgr_colors_and_flags() {
    let mut t = term(40, 5);
    let mut colors = Colors::new(Rgb::from_hex(0xeeeeee), Rgb::from_hex(0x101010));
    colors.palette[1] = Rgb::from_hex(0xaa0000);
    t.set_colors(&colors);
    feed(
        &mut t,
        "\x1b[31mR\x1b[0m\x1b[38;2;1;2;3;48;2;10;20;30mT\x1b[0m\x1b[38;5;200mP\x1b[0m\
         \x1b[1mB\x1b[0m\x1b[3mI\x1b[0m\x1b[2mF\x1b[0m\x1b[5mK\x1b[0m\x1b[7mV\x1b[0m\
         \x1b[8mH\x1b[0m\x1b[9mS\x1b[0m\x1b[53mO\x1b[0mN",
    );
    let f = frame(&mut t);
    let c = &f.rows[0].cells;
    assert_eq!(c[0].ch, 'R');
    assert_eq!(c[0].fg, Some(Rgb::from_hex(0xaa0000)));
    assert_eq!(c[1].fg, Some(Rgb::new(1, 2, 3)));
    assert_eq!(c[1].bg, Some(Rgb::new(10, 20, 30)));
    assert_eq!(c[2].fg, Some(f.colors.palette[200]));
    assert!(c[3].flags.contains(CellFlags::BOLD));
    assert!(c[4].flags.contains(CellFlags::ITALIC));
    assert!(c[5].flags.contains(CellFlags::FAINT));
    assert!(c[6].flags.contains(CellFlags::BLINK));
    assert!(c[7].flags.contains(CellFlags::INVERSE));
    assert!(c[8].flags.contains(CellFlags::INVISIBLE));
    assert!(c[9].flags.contains(CellFlags::STRIKETHROUGH));
    assert!(c[10].flags.contains(CellFlags::OVERLINE));
    assert_eq!(c[11].flags, CellFlags::default());
    assert_eq!(c[11].fg, None);
    assert_eq!(f.colors.foreground, Rgb::from_hex(0xeeeeee));
    assert_eq!(f.colors.background, Rgb::from_hex(0x101010));
    let (fg, bg) = c[7].colors(f.colors.foreground, f.colors.background);
    assert_eq!((fg, bg), (f.colors.background, f.colors.foreground));
}

#[test]
fn underline_kinds_and_color() {
    let mut t = term(40, 5);
    feed(
        &mut t,
        "\x1b[4mA\x1b[0m\x1b[21mB\x1b[0m\x1b[4:3mC\x1b[0m\x1b[4:4mD\x1b[0m\x1b[4:5mE\x1b[0m\
         \x1b[4m\x1b[58;2;9;8;7mF\x1b[0mG",
    );
    let c = &frame(&mut t).rows[0].cells;
    assert_eq!(c[0].underline, Underline::Single);
    assert_eq!(c[1].underline, Underline::Double);
    assert_eq!(c[2].underline, Underline::Curly);
    assert_eq!(c[3].underline, Underline::Dotted);
    assert_eq!(c[4].underline, Underline::Dashed);
    assert_eq!(c[5].underline_color, Some(Rgb::new(9, 8, 7)));
    assert_eq!(c[6].underline, Underline::None);
}

#[test]
fn wide_and_grapheme_clusters() {
    let mut t = term(20, 3);
    feed(&mut t, "日a\u{0065}\u{0301}b👨‍👩‍👧x");
    let f = frame(&mut t);
    let c = &f.rows[0].cells;
    assert_eq!(c[0].ch, '日');
    assert_eq!(c[0].wide, Wide::Wide);
    assert_eq!(c[1].wide, Wide::SpacerTail);
    assert!(c[1].is_empty());
    assert_eq!(c[2].ch, 'a');
    assert_eq!(c[3].ch, 'e');
    assert_eq!(c[3].extra.as_deref(), Some(&['\u{301}'][..]));
    let mut s = String::new();
    c[3].push_text(&mut s);
    assert_eq!(s, "e\u{301}");
    assert_eq!(c[4].ch, 'b');
    assert_eq!(c[5].ch, '👨');
    assert_eq!(c[5].extra.as_ref().map(|e| e.len()), Some(4));
    assert_eq!(c[5].wide, Wide::Wide);
    assert_eq!(c[7].ch, 'x');
}

#[test]
fn hyperlinks() {
    let mut t = term(20, 3);
    feed(
        &mut t,
        "\x1b]8;;https://example.com/a\x1b\\link\x1b]8;;\x1b\\ plain",
    );
    let f = frame(&mut t);
    assert!(f.rows[0].cells[0].flags.contains(CellFlags::HYPERLINK));
    assert!(!f.rows[0].cells[6].flags.contains(CellFlags::HYPERLINK));
    assert_eq!(
        t.hyperlink_at(1, 0).as_deref(),
        Some("https://example.com/a")
    );
    assert_eq!(t.hyperlink_at(6, 0), None);
}

#[test]
fn scrollback_and_scrollbar() {
    let mut t = term(10, 4);
    for i in 0..30 {
        feed(&mut t, &format!("line{i}\r\n"));
    }
    let f = frame(&mut t);
    assert!(t.viewport_at_bottom());
    assert_eq!(f.scrollbar.len, 4);
    assert!(f.scrollbar.total >= 30);
    assert_eq!(f.scrollbar.offset + f.scrollbar.len, f.scrollbar.total);
    let bottom_text = f.rows[0].text();

    t.scroll_lines(-5);
    let f = frame(&mut t);
    assert!(!t.viewport_at_bottom());
    assert_ne!(f.rows[0].text(), bottom_text);
    assert!(
        f.rows.iter().all(|r| r.dirty),
        "scrolling dirties every row"
    );

    t.scroll_to_top();
    let f = frame(&mut t);
    assert_eq!(f.rows[0].text(), "line0");
    assert_eq!(f.scrollbar.offset, 0);
    assert_eq!(f.cursor.pos, None);

    t.scroll_pages(1);
    assert_eq!(frame(&mut t).scrollbar.offset, 4);

    t.scroll_to_row(10);
    assert_eq!(frame(&mut t).rows[0].text(), "line10");

    t.scroll_to_bottom();
    let f = frame(&mut t);
    assert!(t.viewport_at_bottom());
    assert_eq!(f.rows[0].text(), bottom_text);
}

#[test]
fn scrollback_limit_zero_disables_history() {
    let mut t = term(10, 4);
    t.set_scrollback_limit_bytes(0);
    for i in 0..30 {
        feed(&mut t, &format!("line{i}\r\n"));
    }
    let f = frame(&mut t);
    assert_eq!(f.scrollbar.total, 4);
}

#[test]
fn alt_screen_and_modes() {
    let mut t = term(20, 5);
    assert!(!t.is_alt_screen());
    feed(&mut t, "main\x1b[?1049h");
    assert!(t.is_alt_screen());
    let f = frame(&mut t);
    assert!(f.alt_screen);
    assert_eq!(f.text().trim(), "");
    feed(&mut t, "\x1b[?1049l");
    assert!(!t.is_alt_screen());
    assert_eq!(text(&mut t).trim(), "main");

    assert_eq!(t.mouse_tracking(), MouseTracking::None);
    for (seq, want) in [
        ("\x1b[?9h", MouseTracking::X10),
        ("\x1b[?9l\x1b[?1000h", MouseTracking::Normal),
        ("\x1b[?1000l\x1b[?1002h", MouseTracking::Button),
        ("\x1b[?1002l\x1b[?1003h", MouseTracking::Any),
        ("\x1b[?1003l", MouseTracking::None),
    ] {
        feed(&mut t, seq);
        assert_eq!(t.mouse_tracking(), want, "after {seq:?}");
    }

    assert!(!t.bracketed_paste());
    feed(&mut t, "\x1b[?2004h");
    assert!(t.bracketed_paste());
    assert!(!t.focus_reporting());
    feed(&mut t, "\x1b[?1004h");
    assert!(t.focus_reporting());
    assert!(t.mode(Mode::FOCUS_EVENT));
    t.set_mode(Mode::FOCUS_EVENT, false);
    assert!(!t.focus_reporting());
}

#[test]
fn dirty_rows() {
    let mut t = term(20, 5);
    let mut f = Frame::new();
    assert!(t.update_frame(&mut f));
    assert_eq!(f.dirty, Dirty::Full);
    assert!(!t.update_frame(&mut f));
    assert_eq!(f.dirty, Dirty::Clean);
    assert!(f.rows.iter().all(|r| !r.dirty));

    feed(&mut t, "x");
    assert!(t.update_frame(&mut f));
    assert_eq!(f.dirty, Dirty::Partial);
    assert!(f.rows[0].dirty);
    assert!(f.rows[1..].iter().all(|r| !r.dirty));
    assert_eq!(f.rows[0].text(), "x");
}

/// The shell repaints its prompt after a resize (SIGWINCH), so the terminal must clear the
/// reflowed prompt first or the old copy stays above the new one.
#[test]
fn resize_clears_the_prompt_for_the_shell_to_redraw() {
    let mut t = term(40, 6);
    feed(&mut t, "out\r\n\x1b]133;A;cl=line\x07$ \x1b]133;B\x07");
    t.resize(30, 6, CELL_W, CELL_H);
    let f = frame(&mut t);
    assert_eq!(f.rows[0].text(), "out");
    assert_eq!(f.rows[1].text(), "");
}

#[test]
fn resize_clears_every_row_of_a_prompt_that_wraps() {
    let mut t = term(40, 6);
    feed(&mut t, "out\r\n\x1b]133;A;cl=line\x07$ ");
    feed(&mut t, &"x".repeat(30));
    feed(&mut t, "\x1b]133;B\x07");
    t.resize(20, 6, CELL_W, CELL_H);
    let f = frame(&mut t);
    assert_eq!(f.rows[0].text(), "out");
    assert!(f.rows[1..].iter().all(|row| row.text().is_empty()), "{:?}", f.text());
}

#[test]
fn resize_updates_frame() {
    let mut t = term(20, 5);
    feed(&mut t, "abc");
    let mut f = Frame::new();
    t.update_frame(&mut f);
    t.resize(30, 8, CELL_W, CELL_H);
    assert!(t.update_frame(&mut f));
    assert_eq!((f.cols, f.rows.len()), (30, 8));
    assert_eq!(f.rows[0].cells.len(), 30);
    assert_eq!(f.rows[0].text(), "abc");
    assert_eq!(
        t.size(),
        Size {
            cols: 30,
            rows: 8,
            cell_width: CELL_W,
            cell_height: CELL_H
        }
    );
}

#[test]
fn palette_and_default_colors() {
    let mut t = term(20, 3);
    let base: [Rgb; 16] = std::array::from_fn(|i| Rgb::new(i as u8 * 10, 0, 0));
    let mut colors =
        Colors::new(Rgb::from_hex(0xf0f0f0), Rgb::from_hex(0x202020)).with_base16(base, true);
    colors.cursor = Some(Rgb::from_hex(0x00ff00));
    colors.selection_background = Some(Rgb::from_hex(0x334455));
    assert_eq!(colors.palette[3], Rgb::new(30, 0, 0));
    assert_ne!(
        colors.palette[100],
        default_palette()[100],
        "cube regenerated"
    );
    t.set_colors(&colors);
    feed(&mut t, "\x1b[44mx");
    let f = frame(&mut t);
    assert_eq!(f.colors.palette[4], Rgb::new(40, 0, 0));
    assert_eq!(f.rows[0].cells[0].bg, Some(Rgb::new(40, 0, 0)));
    assert_eq!(f.colors.cursor, Some(Rgb::from_hex(0x00ff00)));
    assert_eq!(f.colors.selection_background, Some(Rgb::from_hex(0x334455)));

    // Programs can override at runtime (OSC 4 / OSC 11).
    feed(&mut t, "\x1b]4;4;rgb:01/02/03\x1b\\\x1b]11;#123456\x1b\\");
    let f = frame(&mut t);
    assert_eq!(f.colors.palette[4], Rgb::new(1, 2, 3));
    assert_eq!(f.colors.background, Rgb::from_hex(0x123456));
}

#[test]
fn rgb_parse() {
    assert_eq!(Rgb::parse("#ff8000"), Some(Rgb::new(255, 128, 0)));
    assert_eq!(Rgb::parse("ForestGreen"), Some(Rgb::new(34, 139, 34)));
    assert_eq!(Rgb::parse("nope"), None);
}

#[test]
fn synchronized_output_holds_frame() {
    let mut t = term(20, 3);
    feed(&mut t, "before");
    let mut f = Frame::new();
    t.update_frame(&mut f);
    feed(&mut t, "\x1b[?2026h\x1b[2J\x1b[Hduring");
    t.update_frame(&mut f);
    assert_eq!(
        f.text().trim(),
        "before",
        "held frame keeps showing previous content"
    );
    feed(&mut t, "\x1b[?2026l");
    t.update_frame(&mut f);
    assert_eq!(f.text().trim(), "during");
}

#[test]
fn events() {
    let mut t = term(20, 3);
    feed(
        &mut t,
        "\x07\x1b]2;my title\x07\x1b]7;file://host/Users/a%20b/proj\x07",
    );
    feed(&mut t, "\x1b]9;4;1;42\x07");
    feed(&mut t, "\x1b]777;notify;Build;done\x07");
    feed(&mut t, "\x1b]52;c;aGVsbG8=\x07");
    feed(&mut t, "\x1b]133;D;3\x07");
    let events = t.take_events();
    assert!(events.contains(&Event::Bell), "{events:?}");
    assert!(
        events.contains(&Event::Title("my title".into())),
        "{events:?}"
    );
    assert!(
        events.contains(&Event::Pwd("file://host/Users/a%20b/proj".into())),
        "{events:?}"
    );
    assert!(
        events.contains(&Event::Progress(Progress {
            state: ProgressState::Set,
            percent: Some(42)
        })),
        "{events:?}"
    );
    assert!(
        events.contains(&Event::Notification {
            title: "Build".into(),
            body: "done".into()
        }),
        "{events:?}"
    );
    assert!(
        events.contains(&Event::ClipboardWrite {
            location: ClipboardLocation::Standard,
            text: Some("hello".into())
        }),
        "{events:?}"
    );
    assert!(
        events.contains(&Event::SemanticPrompt {
            kind: PromptKind::CommandEnd,
            exit_code: Some(3)
        }),
        "{events:?}"
    );
    assert_eq!(t.title(), "my title");
    assert_eq!(t.pwd_path().as_deref(), Some("/Users/a b/proj"));
    assert!(t.take_events().is_empty());
}

#[test]
fn working_directory_reports_support_shell_integration_round_trip() {
    let mut t = term(20, 3);
    for (report, expected) in [
        ("file://localhost/Users/engineering/development/rnd/angelly", "/Users/engineering/development/rnd/angelly"),
        ("kitty-shell-cwd://localhost/Users/engineering", "/Users/engineering"),
        ("kitty-shell-cwd://localhost/Users/engineering/development/rnd/angelly", "/Users/engineering/development/rnd/angelly"),
        ("kitty-shell-cwd://localhost/tmp/a%20 b", "/tmp/a%20 b"),
        ("file://localhost/tmp/a%20b", "/tmp/a b"),
    ] {
        feed(&mut t, &format!("\x1b]7;{report}\x07"));
        assert_eq!(t.pwd_path().as_deref(), Some(expected), "{report}");
    }
}

#[test]
fn program_status_event() {
    let mut t = term(20, 3);
    feed(&mut t, "\x1b]7501;?\x1b\\");
    let reply = String::from_utf8(t.take_pty_output()).unwrap();
    assert!(reply.contains("7501;?"), "{reply:?}");
    feed(
        &mut t,
        "\x1b]7501;state=working:app=claude-code:progress=30:msg=QXBwbHk/\x1b\\",
    );
    let events = t.take_events();
    let status = events.iter().find_map(|e| match e {
        Event::ProgramStatus(s) => Some(s.clone()),
        _ => None,
    });
    let status = status.unwrap_or_else(|| panic!("no program status in {events:?}"));
    assert_eq!(status.state, ProgramState::Working);
    assert_eq!(status.app, "claude-code");
    assert_eq!(status.message, "Apply?");
    assert_eq!(status.progress, Some(30));
}

#[test]
fn resize_window_request() {
    let mut t = term(20, 3);
    feed(&mut t, "\x1b[8;10;40t");
    assert!(t
        .take_events()
        .contains(&Event::ResizeRequest { cols: 40, rows: 10 }));
}

#[test]
fn query_replies() {
    let mut t = term(20, 5);
    t.set_terminal_name("xterm-256color");
    feed(&mut t, "ab\x1b[c");
    assert_eq!(t.take_pty_output(), b"\x1b[?62;22;52c");
    feed(&mut t, "\x1b[>c");
    assert_eq!(t.take_pty_output(), b"\x1b[>1;10;0c");
    feed(&mut t, "\x1b[6n");
    assert_eq!(t.take_pty_output(), b"\x1b[1;3R");
    feed(&mut t, "\x1b[5n");
    assert_eq!(t.take_pty_output(), b"\x1b[0n");
    feed(&mut t, "\x1b[18t");
    assert_eq!(t.take_pty_output(), b"\x1b[8;5;20t");
    feed(&mut t, "\x1b[16t");
    assert_eq!(t.take_pty_output(), b"\x1b[6;20;10t");
    feed(&mut t, "\x1b[?996n");
    assert_eq!(t.take_pty_output(), b"\x1b[?997;1n");
    t.set_colors(&Colors::new(
        Rgb::from_hex(0x111111),
        Rgb::from_hex(0xffffff),
    ));
    feed(&mut t, "\x1b[?996n");
    assert_eq!(t.take_pty_output(), b"\x1b[?997;2n");
    feed(&mut t, "\x1b[?2031h");
    t.set_color_scheme(ColorScheme::Dark);
    assert_eq!(t.take_pty_output(), b"\x1b[?997;1n");
    feed(&mut t, "\x1b]11;?\x07");
    assert_eq!(t.take_pty_output(), b"\x1b]11;rgb:ffff/ffff/ffff\x07");
    feed(&mut t, "\x1b[?2004$p");
    assert_eq!(t.take_pty_output(), b"\x1b[?2004;2$y");
}

#[test]
fn selection_ranges_and_text() {
    let mut t = term(20, 5);
    feed(&mut t, "hello world\r\nsecond line\r\nthird");
    assert!(!t.has_selection());

    assert!(t.select_range((6, 0), (4, 1), false));
    assert!(t.has_selection());
    assert_eq!(t.selection_text().as_deref(), Some("world\nsecon"));
    let f = frame(&mut t);
    assert_eq!(f.rows[0].selection, Some((6, 19)));
    assert_eq!(f.rows[1].selection, Some((0, 4)));
    assert_eq!(f.rows[2].selection, None);

    assert!(t.select_range((1, 0), (3, 2), true));
    assert_eq!(t.selection_text().as_deref(), Some("ell\neco\nhir"));

    assert!(t.select_word(7, 0));
    assert_eq!(t.selection_text().as_deref(), Some("world"));
    assert!(t.select_line(3, 1));
    assert_eq!(t.selection_text().as_deref(), Some("second line"));
    assert!(t.select_all());
    assert_eq!(
        t.selection_text().as_deref(),
        Some("hello world\nsecond line\nthird")
    );

    t.clear_selection();
    assert!(!t.has_selection());
    assert_eq!(t.selection_text(), None);
    let f = frame(&mut t);
    assert!(f.rows.iter().all(|r| r.selection.is_none()));
}

#[test]
fn selection_unwraps_soft_wrapped_lines() {
    let mut t = term(10, 4);
    feed(&mut t, "0123456789abcdef");
    assert!(t.select_line(2, 0));
    assert_eq!(t.selection_text().as_deref(), Some("0123456789abcdef"));
}

#[test]
fn selection_adjust() {
    let mut t = term(20, 5);
    feed(&mut t, "abc def");
    t.select_range((0, 0), (1, 0), false);
    assert!(t.adjust_selection(SelectionAdjust::Right));
    assert_eq!(t.selection_text().as_deref(), Some("abc"));
    assert!(t.adjust_selection(SelectionAdjust::LineEnd));
    assert_eq!(t.selection_text().as_deref(), Some("abc def"));
}

/// Pointer in the left half of the cell.
fn pointer(col: u16, row: u16) -> Pointer {
    Pointer {
        col,
        row,
        x: col as f64 * CELL_W as f64 + 2.0,
        y: row as f64 * CELL_H as f64 + 5.0,
    }
}

/// Pointer in the right half of the cell, which includes it in a drag.
fn pointer_right(col: u16, row: u16) -> Pointer {
    Pointer {
        x: pointer(col, row).x + 5.0,
        ..pointer(col, row)
    }
}

#[test]
fn empty_selection_text() {
    let mut t = term(20, 5);
    t.select_press(pointer(1, 0));
    t.select_drag(pointer_right(8, 0), false);
    t.select_release(Some((8, 0)));
    assert!(t.has_selection());
    assert_eq!(t.selection_text().as_deref(), Some(""));
    assert_eq!(t.selection_text_as(TextFormat::Plain, true).as_deref(), Some(""));
    assert!(t.selection_text_as(TextFormat::Vt, true).is_some());
}

#[test]
fn mouse_selection_gesture() {
    let mut t = term(20, 5);
    feed(&mut t, "hello world\r\nsecond line");

    t.select_press(pointer(1, 0));
    assert!(!t.has_selection(), "a bare click selects nothing");
    t.select_drag(pointer_right(8, 0), false);
    assert_eq!(t.selection_text().as_deref(), Some("ello wor"));
    t.select_drag(pointer_right(3, 1), false);
    assert_eq!(t.selection_text().as_deref(), Some("ello world\nseco"));
    t.select_release(Some((3, 1)));
    assert!(t.has_selection());

    // Double click selects the word, triple click the line.
    t.select_press(pointer(7, 0));
    t.select_release(Some((7, 0)));
    t.select_press(pointer(7, 0));
    assert_eq!(t.selection_text().as_deref(), Some("world"));
    t.select_release(Some((7, 0)));
    t.select_press(pointer(7, 0));
    assert_eq!(t.selection_text().as_deref(), Some("hello world"));
    t.select_release(Some((7, 0)));
}

#[test]
fn drag_below_grid_requests_autoscroll() {
    let mut t = term(20, 3);
    for i in 0..10 {
        feed(&mut t, &format!("row{i}\r\n"));
    }
    t.select_press(pointer(0, 1));
    t.select_drag(
        Pointer {
            col: 4,
            row: 2,
            x: 45.0,
            y: 3.0 * CELL_H as f64 + 30.0,
        },
        false,
    );
    assert_eq!(t.selection_autoscroll(), Some(Autoscroll::Down));
    t.select_release(None);
}

#[test]
fn search() {
    let mut t = term(30, 4);
    for i in 0..12 {
        feed(&mut t, &format!("item {i} needle\r\n"));
    }
    assert!(t.search_poll().is_none());
    t.search_set("NEEDLE");
    assert!(t.is_searching());
    let mut st = t.search_poll().unwrap();
    for _ in 0..20 {
        if st.complete {
            break;
        }
        st = t.search_poll().unwrap();
    }
    assert!(st.complete);
    assert_eq!(st.total, 12);
    assert_eq!(st.selected, None);
    let visible = t.search_viewport_matches();
    assert!(!visible.is_empty() && visible.len() <= 4, "{visible:?}");
    assert!(visible
        .iter()
        .all(|m| m.end.0 - m.start.0 == 5 && m.start.1 == m.end.1));

    assert!(t.search_next());
    t.search_poll();
    assert_eq!(t.search_poll().unwrap().selected, Some(0));
    assert!(t.search_selected_match().is_some());
    assert!(t.search_next());
    assert_eq!(t.search_poll().unwrap().selected, Some(1));
    assert!(t.search_prev());
    assert_eq!(t.search_poll().unwrap().selected, Some(0));

    t.search_set("no such text");
    t.search_poll();
    assert_eq!(t.search_poll().unwrap().total, 0);
    t.search_set("");
    assert!(!t.is_searching());
}

#[test]
fn key_encoding_legacy() {
    let mut t = term(20, 5);
    let enc = |t: &mut Term, ev: KeyEvent| String::from_utf8_lossy(&t.encode_key(&ev)).into_owned();
    assert_eq!(
        enc(
            &mut t,
            KeyEvent::press(Key::A, Mods::default()).with_text("a")
        ),
        "a"
    );
    assert_eq!(
        enc(
            &mut t,
            KeyEvent::press(Key::A, Mods::SHIFT)
                .with_text("A")
                .with_consumed(Mods::SHIFT)
        ),
        "A"
    );
    assert_eq!(
        enc(&mut t, KeyEvent::press(Key::C, Mods::CTRL).with_text("c")),
        "\x03"
    );
    assert_eq!(
        enc(&mut t, KeyEvent::press(Key::Enter, Mods::default())),
        "\r"
    );
    assert_eq!(
        enc(&mut t, KeyEvent::press(Key::Backspace, Mods::default())),
        "\x7f"
    );
    assert_eq!(
        enc(&mut t, KeyEvent::press(Key::Tab, Mods::default())),
        "\t"
    );
    assert_eq!(
        enc(&mut t, KeyEvent::press(Key::Tab, Mods::SHIFT)),
        "\x1b[Z"
    );
    assert_eq!(
        enc(&mut t, KeyEvent::press(Key::Escape, Mods::default())),
        "\x1b"
    );
    assert_eq!(
        enc(&mut t, KeyEvent::press(Key::ArrowUp, Mods::default())),
        "\x1b[A"
    );
    assert_eq!(
        enc(&mut t, KeyEvent::press(Key::ArrowLeft, Mods::SHIFT)),
        "\x1b[1;2D"
    );
    assert_eq!(
        enc(&mut t, KeyEvent::press(Key::ArrowRight, Mods::CTRL)),
        "\x1b[1;5C"
    );
    assert_eq!(
        enc(&mut t, KeyEvent::press(Key::Home, Mods::default())),
        "\x1b[H"
    );
    assert_eq!(
        enc(&mut t, KeyEvent::press(Key::PageUp, Mods::default())),
        "\x1b[5~"
    );
    assert_eq!(
        enc(&mut t, KeyEvent::press(Key::Delete, Mods::default())),
        "\x1b[3~"
    );
    assert_eq!(
        enc(&mut t, KeyEvent::press(Key::F1, Mods::default())),
        "\x1bOP"
    );
    assert_eq!(
        enc(&mut t, KeyEvent::press(Key::F5, Mods::default())),
        "\x1b[15~"
    );
    assert_eq!(
        enc(&mut t, KeyEvent::press(Key::ShiftLeft, Mods::SHIFT)),
        ""
    );
    assert_eq!(
        enc(
            &mut t,
            KeyEvent::press(Key::A, Mods::default())
                .with_text("a")
                .with_action(KeyAction::Release)
        ),
        ""
    );

    feed(&mut t, "\x1b[?1h");
    assert_eq!(
        enc(&mut t, KeyEvent::press(Key::ArrowUp, Mods::default())),
        "\x1bOA"
    );
    assert_eq!(
        enc(&mut t, KeyEvent::press(Key::ArrowUp, Mods::SHIFT)),
        "\x1b[1;2A"
    );
}

#[test]
fn key_encoding_option_as_alt() {
    let mut t = term(20, 5);
    let ev = KeyEvent::press(Key::B, Mods::ALT).with_text("∫");
    assert_eq!(t.encode_key(&ev), "∫".as_bytes());
    t.set_option_as_alt(OptionAsAlt::True);
    assert_eq!(t.encode_key(&ev), b"\x1bb");
    t.set_option_as_alt(OptionAsAlt::Left);
    assert_eq!(
        t.encode_key(&ev),
        b"\x1bb",
        "left option (no SIDE bit) counts as left"
    );
    t.set_option_as_alt(OptionAsAlt::Right);
    assert_eq!(t.encode_key(&ev), "∫".as_bytes());
}

#[test]
fn key_encoding_kitty() {
    let mut t = term(20, 5);
    assert_eq!(t.kitty_keyboard_flags(), 0);
    feed(&mut t, "\x1b[>1u");
    assert_eq!(t.kitty_keyboard_flags(), 1);
    assert_eq!(
        t.encode_key(&KeyEvent::press(Key::Escape, Mods::default())),
        b"\x1b[27u"
    );
    assert_eq!(
        t.encode_key(&KeyEvent::press(Key::C, Mods::CTRL).with_text("c")),
        b"\x1b[99;5u"
    );
    assert_eq!(
        t.encode_key(&KeyEvent::press(Key::Enter, Mods::SHIFT)),
        b"\x1b[13;2u"
    );
    feed(&mut t, "\x1b[<u");
    assert_eq!(t.kitty_keyboard_flags(), 0);
    assert_eq!(
        t.encode_key(&KeyEvent::press(Key::Escape, Mods::default())),
        b"\x1b"
    );

    feed(&mut t, "\x1b[>3u");
    let release = KeyEvent::press(Key::A, Mods::default())
        .with_text("a")
        .with_action(KeyAction::Release);
    assert_eq!(t.encode_key(&release), b"\x1b[97;1:3u");
}

fn mouse(action: MouseAction, button: Option<MouseButton>, col: u16, row: u16) -> MouseEvent {
    MouseEvent {
        action,
        button,
        mods: Mods::default(),
        x: col as f32 * CELL_W as f32 + 3.0,
        y: row as f32 * CELL_H as f32 + 4.0,
    }
}

#[test]
fn mouse_encoding_sgr() {
    let mut t = term(40, 10);
    let press = mouse(MouseAction::Press, Some(MouseButton::Left), 4, 2);
    assert!(t.encode_mouse(&press).is_empty(), "no tracking, no report");
    feed(&mut t, "\x1b[?1000h\x1b[?1006h");
    assert!(t.mouse_reporting());
    assert_eq!(t.encode_mouse(&press), b"\x1b[<0;5;3M");
    assert_eq!(
        t.encode_mouse(&mouse(MouseAction::Release, Some(MouseButton::Left), 4, 2)),
        b"\x1b[<0;5;3m"
    );
    assert_eq!(
        t.encode_mouse(&mouse(MouseAction::Press, Some(MouseButton::Right), 0, 0)),
        b"\x1b[<2;1;1M"
    );
    let mut with_ctrl = press;
    with_ctrl.mods = Mods::CTRL;
    assert_eq!(t.encode_mouse(&with_ctrl), b"\x1b[<16;5;3M");
    assert!(
        t.encode_mouse(&mouse(MouseAction::Motion, None, 6, 6))
            .is_empty(),
        "normal mode has no motion"
    );

    feed(&mut t, "\x1b[?1002h");
    t.encode_mouse(&press);
    assert_eq!(
        t.encode_mouse(&mouse(MouseAction::Motion, Some(MouseButton::Left), 5, 2)),
        b"\x1b[<32;6;3M"
    );
    assert!(
        t.encode_mouse(&mouse(MouseAction::Motion, Some(MouseButton::Left), 5, 2))
            .is_empty(),
        "motion inside the same cell is deduplicated"
    );
    t.encode_mouse(&mouse(MouseAction::Release, Some(MouseButton::Left), 5, 2));

    feed(&mut t, "\x1b[?1003h");
    assert_eq!(
        t.encode_mouse(&mouse(MouseAction::Motion, None, 9, 9)),
        b"\x1b[<35;10;10M"
    );
}

#[test]
fn mouse_encoding_legacy_and_padding() {
    let mut t = term(40, 10);
    t.set_padding(Padding {
        left: 10,
        top: 20,
        right: 0,
        bottom: 0,
    });
    feed(&mut t, "\x1b[?1000h");
    let ev = MouseEvent {
        action: MouseAction::Press,
        button: Some(MouseButton::Left),
        mods: Mods::default(),
        x: 10.0 + 4.0 * CELL_W as f32 + 1.0,
        y: 20.0 + 2.0 * CELL_H as f32 + 1.0,
    };
    assert_eq!(t.encode_mouse(&ev), [0x1b, b'[', b'M', 32, 32 + 5, 32 + 3]);
}

#[test]
fn wheel_routing() {
    let mut t = term(40, 4);
    for i in 0..20 {
        feed(&mut t, &format!("l{i}\r\n"));
    }
    // Plain screen: scrolls the viewport.
    assert_eq!(t.wheel(3, 0.0, 0.0, Mods::default()), Wheel::Scrolled);
    assert!(!t.viewport_at_bottom());
    assert_eq!(t.wheel(-3, 0.0, 0.0, Mods::default()), Wheel::Scrolled);
    assert!(t.viewport_at_bottom());

    // Mouse reporting: wheel buttons.
    feed(&mut t, "\x1b[?1000h\x1b[?1006h");
    assert_eq!(
        t.wheel(2, 13.0, 4.0, Mods::default()),
        Wheel::Send(b"\x1b[<64;2;1M\x1b[<64;2;1M".to_vec())
    );
    assert_eq!(
        t.wheel(-1, 13.0, 4.0, Mods::default()),
        Wheel::Send(b"\x1b[<65;2;1M".to_vec())
    );
    feed(&mut t, "\x1b[?1000l");

    // Alternate screen with alternate scroll: arrow keys.
    feed(&mut t, "\x1b[?1049h\x1b[?1007h");
    assert_eq!(
        t.wheel(2, 0.0, 0.0, Mods::default()),
        Wheel::Send(b"\x1b[A\x1b[A".to_vec())
    );
    feed(&mut t, "\x1b[?1h");
    assert_eq!(
        t.wheel(-1, 0.0, 0.0, Mods::default()),
        Wheel::Send(b"\x1bOB".to_vec())
    );
}

#[test]
fn paste_encoding() {
    assert_eq!(encode_paste("a\nb", false), b"a\rb");
    assert_eq!(encode_paste("a\nb", true), b"\x1b[200~a\nb\x1b[201~");
    assert_eq!(
        encode_paste("a\x1bb\x00c", true),
        b"\x1b[200~a b c\x1b[201~"
    );
    assert!(paste_is_safe("plain text"));
    assert!(!paste_is_safe("rm -rf /\n"));
    assert!(!paste_is_safe("x\x1b[201~y"));

    let mut t = term(20, 5);
    assert_eq!(t.paste("one line", false), Ok(b"one line".to_vec()));
    assert_eq!(t.paste("a\nb", false), Err(PasteError::Unsafe));
    assert_eq!(t.paste("a\nb", true), Ok(b"a\rb".to_vec()));
    feed(&mut t, "\x1b[?2004h");
    assert_eq!(
        t.paste("a\nb", false),
        Ok(b"\x1b[200~a\nb\x1b[201~".to_vec())
    );
    assert_eq!(t.paste("x\x1b[201~y", false), Err(PasteError::Unsafe));
}

#[test]
fn focus_encoding() {
    let mut t = term(20, 5);
    assert!(t.encode_focus(true).is_empty());
    feed(&mut t, "\x1b[?1004h");
    assert_eq!(t.encode_focus(true), b"\x1b[I");
    assert_eq!(t.encode_focus(false), b"\x1b[O");
    assert_eq!(encode_focus(true), b"\x1b[I");
}

#[test]
fn selection_in_vt_and_html_formats() {
    let mut t = term(30, 5);
    feed(&mut t, "plain \x1b[1;31mred\x1b[0m end");
    assert!(t.select_range((0, 0), (29, 0), false));
    assert_eq!(t.selection_text_as(TextFormat::Plain, true).as_deref(), Some("plain red end"));
    let vt = t.selection_text_as(TextFormat::Vt, true).unwrap();
    assert!(vt.contains("\x1b["), "{vt:?}");
    assert!(vt.contains("red"));
    let html = t.selection_text_as(TextFormat::Html, true).unwrap();
    assert!(html.contains("<div") && html.contains("red"), "{html:?}");
    t.clear_selection();
    assert_eq!(t.selection_text_as(TextFormat::Plain, true), None);
}

#[test]
fn screen_text_covers_the_scrollback_or_the_whole_screen() {
    let mut t = term(10, 3);
    for i in 0..8 {
        feed(&mut t, &format!("line{i}\r\n"));
    }
    let all = t.screen_text(ScreenPart::Screen, TextFormat::Plain).unwrap();
    assert!(all.starts_with("line0\nline1"), "{all:?}");
    assert!(all.contains("line7"));
    let history = t.screen_text(ScreenPart::Scrollback, TextFormat::Plain).unwrap();
    assert!(history.starts_with("line0\nline1"), "{history:?}");
    assert!(!history.contains("line7"), "{history:?}");
    let html = t.screen_text(ScreenPart::Scrollback, TextFormat::Html).unwrap();
    assert!(html.contains("<div") && html.contains("line0"), "{html:?}");
    let mut fresh = term(10, 3);
    feed(&mut fresh, "short");
    assert_eq!(fresh.screen_text(ScreenPart::Scrollback, TextFormat::Plain), None);
    assert!(!t.has_selection(), "formatting must not leave a selection behind");
}

#[test]
fn word_wise_selection_adjustment() {
    let mut t = term(20, 3);
    feed(&mut t, "one two three");
    assert!(t.select_range((0, 0), (0, 0), false));
    assert!(t.adjust_selection_by_word(true, DEFAULT_WORD_BOUNDARIES));
    assert_eq!(t.selection_text().as_deref(), Some("one"));
    assert!(t.adjust_selection_by_word(true, DEFAULT_WORD_BOUNDARIES));
    assert_eq!(t.selection_text().as_deref(), Some("one two"));
    assert!(t.adjust_selection_by_word(false, DEFAULT_WORD_BOUNDARIES));
    assert_eq!(t.selection_text().as_deref(), Some("one"));
    t.clear_selection();
    assert!(!t.adjust_selection_by_word(true, DEFAULT_WORD_BOUNDARIES));
}

#[test]
fn scroll_to_selection_brings_it_to_the_top() {
    let mut t = term(10, 3);
    for i in 0..10 {
        feed(&mut t, &format!("row{i}\r\n"));
    }
    t.scroll_to_top();
    assert!(t.select_range((0, 1), (3, 1), false));
    t.scroll_to_bottom();
    assert!(!text(&mut t).contains("row1"));
    assert!(t.scroll_to_selection());
    assert_eq!(frame(&mut t).rows[0].text(), "row1");
    t.clear_selection();
    assert!(!t.scroll_to_selection());
}

#[test]
fn select_output_needs_prompt_marks() {
    let mut t = term(20, 6);
    feed(&mut t, "no marks here");
    assert!(!t.select_output(0, 0));
    let mut t = term(20, 8);
    feed(&mut t, "\x1b]133;A\x07$ \x1b]133;B\x07ls\r\n\x1b]133;C\x07out1\r\nout2\r\n\x1b]133;D;0\x07\x1b]133;A\x07$ ");
    assert!(t.select_output(1, 2));
    assert_eq!(t.selection_text().as_deref(), Some("out1\nout2"));
}

#[test]
fn osc52_read_is_left_for_the_embedder_to_answer() {
    let mut t = term(20, 5);
    feed(&mut t, "\x1b]52;c;?\x07");
    assert_eq!(t.take_events(), vec![Event::ClipboardRead { location: ClipboardLocation::Standard }]);
    assert!(t.take_pty_output().is_empty(), "the library's empty answer must not reach the program");

    feed(&mut t, "\x1b]52;s;?\x1b\\");
    assert_eq!(t.take_events(), vec![Event::ClipboardRead { location: ClipboardLocation::Selection }]);
}

#[test]
fn osc52_write_sets_the_clipboard() {
    let mut t = term(20, 5);
    feed(&mut t, "\x1b]52;c;aGVsbG8=\x07");
    assert_eq!(
        t.take_events(),
        vec![Event::ClipboardWrite { location: ClipboardLocation::Standard, text: Some("hello".into()) }]
    );
}

#[test]
fn clipboard_read_reply_is_base64_osc52() {
    assert_eq!(clipboard_read_reply(ClipboardLocation::Standard, "hello"), b"\x1b]52;c;aGVsbG8=\x1b\\");
    assert_eq!(clipboard_read_reply(ClipboardLocation::Primary, ""), b"\x1b]52;p;\x1b\\");
}

#[test]
fn grapheme_width_method_decides_how_joined_emoji_are_measured() {
    let family = "\u{1F469}\u{200D}\u{1F469}";
    let mut t = term(20, 3);
    t.set_grapheme_clustering(true);
    feed(&mut t, family);
    assert_eq!(frame(&mut t).cursor.pos, Some((2, 0)), "one wide cluster");

    let mut legacy = term(20, 3);
    legacy.set_grapheme_clustering(false);
    feed(&mut legacy, family);
    assert_eq!(frame(&mut legacy).cursor.pos, Some((4, 0)), "two wide codepoints");

    feed(&mut legacy, "\x1b[?2027h\r\n");
    feed(&mut legacy, family);
    assert_eq!(frame(&mut legacy).cursor.pos, Some((2, 1)), "a program can opt back in");
}
