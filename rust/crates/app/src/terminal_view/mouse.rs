//! Pointer handling: selection gestures, mouse reporting, the overlay
//! scrollbar, wheel scrolling, hyperlinks and click-to-move.

use std::time::{Duration, Instant};

use angelly_core::config::CopyOnSelect;
use angelly_core::persist::{Decision, UntrustedUrl};
use gpui_kit::*;
use terminal::{
    Autoscroll, Key, KeyEvent, Mods, MouseAction, MouseEvent, Pointer, PressOptions, Wheel, REPEAT_CLICK_INTERVAL,
};

use super::element::{SCROLLBAR_HIT_WIDTH, SCROLLBAR_MIN_THUMB};
use super::links::{self, LinkAction, LinkKind, Span};
use super::scroll;
use super::{SelectionClipboard, TerminalView};

#[derive(Clone, Debug, PartialEq)]
pub enum LinkTarget {
    /// An OSC 8 hyperlink or a URL found in the text.
    Url(String),
    /// A file path found in the text.
    Path(String),
    /// A keybind action run by a user-defined link.
    Action(String),
}

/// A link under the pointer: the cells it covers, one span per row it wraps over.
#[derive(Clone, Debug, PartialEq)]
pub struct Link {
    pub spans: Vec<Span>,
    /// What the link looks like or points at, for the preview and `copy_url_to_clipboard`.
    pub text: String,
    pub target: LinkTarget,
    /// An OSC 8 hyperlink, whose text may differ from its destination.
    pub osc8: bool,
}

impl Link {
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn contains(&self, col: u16, row: u16) -> bool {
        self.spans.iter().any(|s| s.contains(col, row))
    }
}

#[derive(Default)]
pub struct MouseState {
    pub mods: Modifiers,
    /// Last pointer position in window coordinates.
    pub position: Option<Point<Pixels>>,
    pub over_scrollbar: bool,
    /// Pointer offset inside the scrollbar thumb while it is dragged.
    grab: Option<f32>,
    /// The left button went down and started a selection gesture.
    selecting: bool,
    /// The cell the selection gesture started in.
    press_cell: Option<(u16, u16)>,
    /// The selection gesture moved away from its first cell.
    dragged: bool,
    /// The button whose press was reported to the application.
    reported: Option<terminal::MouseButton>,
    /// The left press landed on a link and waits for its release.
    pressed_link: bool,
    /// When the last selecting press happened; a shift-click after the repeat interval extends
    /// the selection instead of counting as another click.
    last_press: Option<Instant>,
}

fn terminal_mods(m: &Modifiers) -> Mods {
    let mut mods = Mods::default();
    for (on, flag) in [(m.shift, Mods::SHIFT), (m.control, Mods::CTRL), (m.alt, Mods::ALT), (m.platform, Mods::SUPER)] {
        if on {
            mods |= flag;
        }
    }
    mods
}

fn button_of(b: MouseButton) -> Option<terminal::MouseButton> {
    match b {
        MouseButton::Left => Some(terminal::MouseButton::Left),
        MouseButton::Right => Some(terminal::MouseButton::Right),
        MouseButton::Middle => Some(terminal::MouseButton::Middle),
        _ => None,
    }
}

const AUTOSCROLL_INTERVAL: Duration = Duration::from_millis(50);

/// Installs the element's mouse listeners. Called while painting.
pub fn register(view: &Entity<TerminalView>, hitbox: &Hitbox, window: &mut Window) {
    window.on_mouse_event({
        let (view, hitbox) = (view.clone(), hitbox.clone());
        move |ev: &MouseDownEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble && hitbox.is_hovered(window) {
                view.update(cx, |v, cx| v.mouse_down(ev, window, cx));
            }
        }
    });
    window.on_mouse_event({
        let view = view.clone();
        move |ev: &MouseUpEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble {
                view.update(cx, |v, cx| v.mouse_up(ev, window, cx));
            }
        }
    });
    window.on_mouse_event({
        let (view, hitbox) = (view.clone(), hitbox.clone());
        move |ev: &MouseMoveEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble {
                let hovered = hitbox.is_hovered(window);
                view.update(cx, |v, cx| v.mouse_move(ev, hovered, cx));
            }
        }
    });
    window.on_mouse_event({
        let (view, hitbox) = (view.clone(), hitbox.clone());
        move |ev: &ScrollEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble && hitbox.is_hovered(window) {
                view.update(cx, |v, cx| v.scroll_wheel(ev, cx));
            }
        }
    });
}

impl TerminalView {
    /// Pointer position relative to the view's top-left, in logical pixels.
    fn local(&self, p: Point<Pixels>) -> (f32, f32) {
        (f32::from(p.x - self.layout.bounds.origin.x), f32::from(p.y - self.layout.bounds.origin.y))
    }

    fn scale(&self) -> f32 {
        self.metrics.map_or(1.0, |m| m.scale)
    }

    /// The cell under `p`, clamped to the grid.
    fn cell_clamped(&self, p: Point<Pixels>) -> (u16, u16) {
        let Some(m) = self.metrics else { return (0, 0) };
        let x = f32::from(p.x - self.layout.origin.x) / f32::from(m.cell_width);
        let y = f32::from(p.y - self.layout.origin.y) / f32::from(m.cell_height);
        let (cols, rows) = self.layout.grid;
        (
            (x.floor().max(0.0) as u16).min(cols.saturating_sub(1)),
            (y.floor().max(0.0) as u16).min(rows.saturating_sub(1)),
        )
    }

    /// The cell under `p`, or `None` in the padding.
    fn cell_at(&self, p: Point<Pixels>) -> Option<(u16, u16)> {
        let m = self.metrics?;
        let x = f32::from(p.x - self.layout.origin.x) / f32::from(m.cell_width);
        let y = f32::from(p.y - self.layout.origin.y) / f32::from(m.cell_height);
        let (cols, rows) = self.layout.grid;
        (x >= 0.0 && y >= 0.0 && x < f32::from(cols) && y < f32::from(rows)).then(|| (x as u16, y as u16))
    }

    fn pointer(&self, p: Point<Pixels>) -> Pointer {
        let (col, row) = self.cell_clamped(p);
        let (x, y) = self.local(p);
        let s = self.scale();
        Pointer { col, row, x: f64::from(x * s), y: f64::from(y * s) }
    }

    fn mouse_event(
        &self,
        action: MouseAction,
        button: Option<terminal::MouseButton>,
        ev_mods: &Modifiers,
        p: Point<Pixels>,
    ) -> MouseEvent {
        let (x, y) = self.local(p);
        let s = self.scale();
        MouseEvent { action, button, mods: terminal_mods(ev_mods), x: x * s, y: y * s }
    }

    /// Whether the application receives pointer events: it asked for them,
    /// reporting is not toggled off and Shift is not overriding.
    fn app_gets_mouse(&self, mods: &Modifiers) -> bool {
        self.mouse_reporting_enabled
            && self.session.term().mouse_reporting()
            && !(mods.shift && !self.settings.shift_captured_by_apps)
    }

    fn report(&mut self, event: MouseEvent) {
        let bytes = self.session.term().encode_mouse(&event);
        if !bytes.is_empty() {
            self.session.write(&bytes);
        }
    }

    fn scrollbar_visible(&self) -> bool {
        let bar = self.frame.scrollbar;
        self.settings.scrollbar && bar.total > bar.len
    }

    fn in_scroller_zone(&self, p: Point<Pixels>) -> bool {
        self.scrollbar_visible() && f32::from(self.layout.bounds.right() - p.x) <= SCROLLBAR_HIT_WIDTH
    }

    fn thumb(&self) -> Option<(f32, f32)> {
        let bar = self.frame.scrollbar;
        scroll::thumb(f32::from(self.layout.bounds.size.height), bar.total, bar.offset, bar.len, SCROLLBAR_MIN_THUMB)
    }

    fn drag_scrollbar_to(&mut self, p: Point<Pixels>, grab: f32) {
        let Some((_, height)) = self.thumb() else { return };
        let bar = self.frame.scrollbar;
        let y = f32::from(p.y - self.layout.bounds.origin.y);
        let track = f32::from(self.layout.bounds.size.height);
        let row = scroll::offset_for_thumb(track, bar.total, bar.len, height, y - grab);
        self.session.term().scroll_to_row(row as usize);
    }

    pub(crate) fn mouse_down(&mut self, ev: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.mouse.mods = ev.modifiers;
        self.mouse.position = Some(ev.position);
        window.focus(&self.focus, cx);
        self.note_input(cx);
        if ev.button == MouseButton::Left && self.in_scroller_zone(ev.position) {
            self.begin_scrollbar_drag(ev.position, cx);
            return;
        }
        if self.app_gets_mouse(&ev.modifiers) {
            let Some(button) = button_of(ev.button) else { return };
            if button == terminal::MouseButton::Left {
                self.session.term().clear_selection();
            }
            self.mouse.reported = Some(button);
            let event = self.mouse_event(MouseAction::Press, Some(button), &ev.modifiers, ev.position);
            self.report(event);
            cx.stop_propagation();
            return;
        }
        match ev.button {
            MouseButton::Left => self.left_down(ev, cx),
            MouseButton::Middle => {
                self.paste_selection_clipboard(cx);
                cx.stop_propagation();
            }
            MouseButton::Right => self.right_down(ev, cx),
            _ => {}
        }
    }

    fn begin_scrollbar_drag(&mut self, p: Point<Pixels>, cx: &mut Context<Self>) {
        let y = f32::from(p.y - self.layout.bounds.origin.y);
        let grab = match self.thumb() {
            Some((top, height)) if y >= top && y <= top + height => y - top,
            Some((_, height)) => height / 2.0,
            None => return,
        };
        self.mouse.grab = Some(grab);
        self.scroller.dragging = true;
        self.drag_scrollbar_to(p, grab);
        cx.stop_propagation();
        cx.notify();
    }

    fn left_down(&mut self, ev: &MouseDownEvent, cx: &mut Context<Self>) {
        self.mouse.dragged = false;
        self.mouse.pressed_link = false;
        if self.hover_link.is_some() && link_modifiers(&ev.modifiers) {
            self.mouse.pressed_link = true;
            cx.stop_propagation();
            return;
        }
        let pointer = self.pointer(ev.position);
        let extend = ev.modifiers.shift
            && self.mouse.last_press.is_some_and(|at| at.elapsed() > REPEAT_CLICK_INTERVAL)
            && self.session.term().has_selection();
        self.mouse.selecting = true;
        if extend {
            self.drag_selection(ev.position, ev.modifiers.alt, cx);
        } else {
            self.mouse.press_cell = Some((pointer.col, pointer.row));
            self.mouse.last_press = Some(Instant::now());
            let options = PressOptions {
                output_on_triple: ev.modifiers.control || ev.modifiers.platform,
                word_boundaries: Some(self.settings.word_boundaries.clone()),
            };
            self.session.term().select_press_with(pointer, &options);
        }
        cx.stop_propagation();
        cx.notify();
    }

    fn right_down(&mut self, ev: &MouseDownEvent, cx: &mut Context<Self>) {
        use super::settings::RightClick;
        let has_selection = self.session.term().has_selection();
        let action = match self.settings.right_click {
            RightClick::ContextMenu => return,
            RightClick::CopyOrPaste if has_selection => RightClick::Copy,
            RightClick::CopyOrPaste => RightClick::Paste,
            other => other,
        };
        match action {
            RightClick::Copy => {
                self.copy_selection(cx);
            }
            RightClick::Paste => self.paste_clipboard(cx),
            _ => {}
        }
        cx.stop_propagation();
        let _ = ev;
    }

    pub(crate) fn mouse_up(&mut self, ev: &MouseUpEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.mouse.mods = ev.modifiers;
        if ev.button == MouseButton::Left && self.mouse.grab.take().is_some() {
            self.scroller.dragging = false;
            self.scroller.poke();
            cx.notify();
            return;
        }
        if let Some(button) = self.mouse.reported {
            if button_of(ev.button) == Some(button) {
                self.mouse.reported = None;
                let event = self.mouse_event(MouseAction::Release, Some(button), &ev.modifiers, ev.position);
                self.report(event);
            }
            return;
        }
        if ev.button != MouseButton::Left {
            return;
        }
        if std::mem::take(&mut self.mouse.pressed_link) {
            if let Some(link) = self.hover_link.clone() {
                if self.cell_at(ev.position).is_some_and(|(c, r)| link.contains(c, r)) {
                    self.open_link(&link, window, cx);
                }
            }
            return;
        }
        if !std::mem::take(&mut self.mouse.selecting) {
            return;
        }
        self.tasks.autoscroll = None;
        let cell = self.cell_at(ev.position);
        let mut term = self.session.term();
        term.select_release(cell);
        let selected = term.has_selection();
        drop(term);
        if selected {
            self.copy_on_select(cx);
        } else if !self.mouse.dragged && ev.click_count == 1 {
            self.click_to_move(ev.position, cx);
        }
        cx.notify();
    }

    pub(crate) fn mouse_move(&mut self, ev: &MouseMoveEvent, hovered: bool, cx: &mut Context<Self>) {
        self.mouse.mods = ev.modifiers;
        let dragging = ev.pressed_button.is_some();
        if !hovered && !dragging {
            if self.mouse.over_scrollbar || self.hover_link.is_some() {
                self.mouse.over_scrollbar = false;
                self.scroller.hovering = false;
                self.hover_link = None;
                cx.notify();
            }
            return;
        }
        self.mouse.position = Some(ev.position);

        if let Some(grab) = self.mouse.grab {
            self.drag_scrollbar_to(ev.position, grab);
            self.scroller.poke();
            cx.notify();
            return;
        }
        let over = hovered && !dragging && self.in_scroller_zone(ev.position);
        if over != self.mouse.over_scrollbar {
            self.mouse.over_scrollbar = over;
            self.scroller.hovering = over;
            self.scroller.poke();
            cx.notify();
        }

        if let Some(button) = self.mouse.reported {
            let tracking = self.session.term().mouse_tracking();
            if matches!(tracking, terminal::MouseTracking::Button | terminal::MouseTracking::Any) {
                let event = self.mouse_event(MouseAction::Motion, Some(button), &ev.modifiers, ev.position);
                self.report(event);
            }
            return;
        }
        if self.mouse.selecting && ev.pressed_button == Some(MouseButton::Left) {
            self.drag_selection(ev.position, ev.modifiers.alt, cx);
            return;
        }
        if dragging {
            return;
        }
        if self.app_gets_mouse(&ev.modifiers) {
            if self.session.term().mouse_tracking() == terminal::MouseTracking::Any {
                let event = self.mouse_event(MouseAction::Motion, None, &ev.modifiers, ev.position);
                self.report(event);
            }
            if self.hover_link.take().is_some() {
                cx.notify();
            }
            return;
        }
        self.refresh_link(cx);
    }

    fn drag_selection(&mut self, position: Point<Pixels>, rectangle: bool, cx: &mut Context<Self>) {
        let pointer = self.pointer(position);
        if self.mouse.press_cell != Some((pointer.col, pointer.row)) {
            self.mouse.dragged = true;
        }
        let autoscroll = {
            let mut term = self.session.term();
            term.select_drag(pointer, rectangle);
            term.selection_autoscroll()
        };
        match autoscroll {
            Some(direction) if self.tasks.autoscroll.is_none() => self.start_autoscroll(direction, cx),
            None => self.tasks.autoscroll = None,
            _ => {}
        }
        cx.notify();
    }

    fn start_autoscroll(&mut self, direction: Autoscroll, cx: &mut Context<Self>) {
        let task = cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(AUTOSCROLL_INTERVAL).await;
            let keep = this
                .update(cx, |this, cx| {
                    let Some(position) = this.mouse.position.filter(|_| this.mouse.selecting) else { return false };
                    let rectangle = this.mouse.mods.alt;
                    let pointer = this.pointer(position);
                    let mut term = this.session.term();
                    let Some(current) = term.selection_autoscroll() else { return false };
                    let delta = if current == direction { 1 } else { -1 };
                    term.scroll_lines(if matches!(current, Autoscroll::Up) { -delta } else { delta });
                    term.select_autoscroll_tick(pointer, rectangle);
                    drop(term);
                    cx.notify();
                    true
                })
                .unwrap_or(false);
            if !keep {
                this.update(cx, |this, _| this.tasks.autoscroll = None).ok();
                break;
            }
        });
        self.tasks.autoscroll = Some(task);
    }

    pub(crate) fn scroll_wheel(&mut self, ev: &ScrollEvent, cx: &mut Context<Self>) {
        let Some(metrics) = self.metrics else { return };
        self.mouse.mods = ev.modifiers;
        if ev.touch_phase == TouchPhase::Started {
            self.rows.reset();
        }
        let rows = match ev.delta {
            ScrollDelta::Pixels(p) => f32::from(p.y) / f32::from(metrics.cell_height) * self.settings.scroll_precision,
            ScrollDelta::Lines(l) => l.y * self.settings.scroll_discrete,
        };
        let lines = self.rows.push(rows);
        cx.stop_propagation();
        if lines == 0 {
            return;
        }
        let (x, y) = self.local(ev.position);
        let s = self.scale();
        let wheel = self.session.term().wheel(lines, x * s, y * s, terminal_mods(&ev.modifiers));
        match wheel {
            Wheel::Scrolled => self.scroller.poke(),
            Wheel::Send(bytes) => {
                if !bytes.is_empty() {
                    self.session.write(&bytes);
                }
            }
        }
        cx.notify();
    }

    /// Recomputes the link under the pointer. Links are live while the link
    /// modifier (Cmd) is held alone.
    pub(crate) fn refresh_link(&mut self, cx: &mut Context<Self>) {
        let link = self.link_under_pointer();
        if link != self.hover_link {
            self.hover_link = link;
            cx.notify();
        }
    }

    fn link_under_pointer(&self) -> Option<Link> {
        let position = self.mouse.position?;
        if !link_modifiers(&self.mouse.mods) || self.mouse.over_scrollbar || self.mouse.selecting {
            return None;
        }
        let (col, row) = self.cell_at(position)?;
        let rows = &self.frame.rows;
        let cols = rows.get(usize::from(row))?.cells.len();
        if cols == 0 {
            return None;
        }
        // A soft-wrapped line is one run of text across its rows.
        let mut first = usize::from(row);
        while first > 0 && rows[first - 1].wrapped {
            first -= 1;
        }
        let mut last = usize::from(row);
        while last + 1 < rows.len() && rows[last].wrapped {
            last += 1;
        }
        let at = (usize::from(row) - first) * cols + usize::from(col);
        let chars: Vec<char> = rows[first..=last]
            .iter()
            .flat_map(|r| r.cells.iter())
            .map(|c| if c.is_empty() || c.wide == terminal::Wide::SpacerTail { ' ' } else { c.ch })
            .collect();
        if chars.len() != (last - first + 1) * cols {
            return None;
        }

        let uri_at = |index: usize| {
            let (r, c) = (first + index / cols, (index % cols) as u16);
            let cell = rows[r].cells.get(usize::from(c))?;
            cell.flags.contains(terminal::CellFlags::HYPERLINK).then(|| self.session.term().hyperlink_at(c, r as u16))?
        };
        let make = |range: std::ops::Range<usize>, target: LinkTarget, osc8: bool| Link {
            spans: links::spans(range.clone(), cols, first),
            text: match &target {
                LinkTarget::Url(s) | LinkTarget::Path(s) => s.clone(),
                LinkTarget::Action(_) => chars[range].iter().collect(),
            },
            target,
            osc8,
        };

        if self.settings.link_osc8 {
            if let Some(uri) = uri_at(at) {
                let range = links::run_around(at, chars.len(), |i| uri_at(i).as_deref() == Some(uri.as_str()));
                return Some(make(range, LinkTarget::Url(uri), true));
            }
        }
        for custom in self.settings.links.iter() {
            if let Some(range) = custom.find_at(&chars, at) {
                let text: String = chars[range.clone()].iter().collect();
                let target = match &custom.action {
                    LinkAction::Open => LinkTarget::Url(text),
                    LinkAction::Run(action) => LinkTarget::Action(action.clone()),
                };
                return Some(make(range, target, false));
            }
        }
        if !self.settings.link_url {
            return None;
        }
        let m = links::link_at(&chars, at)?;
        let text: String = chars[m.range.clone()].iter().collect();
        let target = match m.kind {
            LinkKind::Url => LinkTarget::Url(text),
            LinkKind::Path => LinkTarget::Path(text),
        };
        Some(make(m.range, target, false))
    }

    pub(crate) fn open_link(&mut self, link: &Link, window: &mut Window, cx: &mut Context<Self>) {
        let target = match &link.target {
            LinkTarget::Url(url) => url.clone(),
            LinkTarget::Path(path) => match self.resolve_path(path) {
                Some(url) => url,
                None => return,
            },
            LinkTarget::Action(text) => {
                let Ok(action) = angelly_core::config::Action::parse(text) else { return };
                if let Some(command) = crate::keybinds::Command::parse(&action) {
                    self.run_command(command, &action, window, cx);
                }
                return;
            }
        };
        open_untrusted(target, window, cx);
    }

    /// A `file://` URL for a path shown in the terminal, relative to the
    /// shell's working directory. `None` when the file does not exist.
    fn resolve_path(&self, text: &str) -> Option<String> {
        let (path, _) = links::split_position(text);
        let path = match path.strip_prefix("~/") {
            Some(rest) => std::path::Path::new(&std::env::var_os("HOME")?).join(rest),
            None => {
                let p = std::path::PathBuf::from(path);
                if p.is_absolute() {
                    p
                } else {
                    self.session.cwd()?.join(p)
                }
            }
        };
        let path = path.canonicalize().ok()?;
        url::Url::from_file_path(path).ok().map(String::from)
    }

    /// Alt-click at the shell prompt moves the cursor by sending arrow keys.
    fn click_to_move(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        if !self.settings.click_to_move || !self.at_prompt || self.readonly || self.session.term().is_alt_screen() {
            return;
        }
        let (Some((cursor_col, cursor_row)), Some((col, row))) = (self.frame.cursor.pos, self.cell_at(position)) else {
            return;
        };
        if row != cursor_row || col == cursor_col {
            return;
        }
        let (key, count) =
            if col > cursor_col { (Key::ArrowRight, col - cursor_col) } else { (Key::ArrowLeft, cursor_col - col) };
        let mut bytes = Vec::new();
        {
            let mut term = self.session.term();
            for _ in 0..count {
                bytes.extend(term.encode_key(&KeyEvent::press(key, Mods::default())));
            }
        }
        self.session.write(&bytes);
        self.note_input(cx);
    }

    fn copy_on_select(&mut self, cx: &mut Context<Self>) {
        let Some(text) = self.session.term().selection_text() else { return };
        match self.settings.copy_on_select {
            CopyOnSelect::None => {}
            CopyOnSelect::Primary => cx.set_global(SelectionClipboard(Some(text))),
            CopyOnSelect::Clipboard => cx.write_to_clipboard(ClipboardItem::new_string(text)),
            CopyOnSelect::Both => {
                cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                cx.set_global(SelectionClipboard(Some(text)));
            }
        }
    }
}

/// Links respond to Cmd held without other modifiers, as in Ghostty on macOS.
fn link_modifiers(m: &Modifiers) -> bool {
    m.platform && !m.shift && !m.control && !m.alt
}

/// Opens `target` according to its [`UntrustedUrl`] decision: web and file
/// targets directly, other schemes after asking, and nothing when denied.
pub fn open_untrusted<T: 'static>(target: String, window: &mut Window, cx: &mut Context<T>) {
    let url = UntrustedUrl::new(target);
    match url.decision() {
        Decision::Allow(u) => cx.open_url(&u),
        Decision::AllowFile(path) => {
            if let Ok(u) = url::Url::from_file_path(path) {
                cx.open_url(u.as_str());
            }
        }
        Decision::Confirm(u) => {
            let detail =
                format!("The link will be passed to the application registered for it.\n\n{}", url.display_string());
            let answer = window.prompt(PromptLevel::Warning, "Open this link?", Some(&detail), &["Open", "Cancel"], cx);
            cx.spawn(async move |_, cx| {
                if answer.await == Ok(0) {
                    cx.update(|cx| cx.open_url(&u));
                }
            })
            .detach();
        }
        Decision::Deny(reason) => {
            let detail = format!("{}\n\n{}", reason.message(), url.display_string());
            let answer = window.prompt(PromptLevel::Critical, "Link blocked", Some(&detail), &["OK"], cx);
            cx.spawn(async move |_, _| {
                let _ = answer.await;
            })
            .detach();
        }
    }
}
