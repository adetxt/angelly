//! The terminal surface: one shell session drawn into a custom gpui element.
//!
//! The view owns the [`Session`] and everything about presenting it: the cell
//! grid and its row cache (`element`), keyboard and IME input (`input`),
//! pointer handling (`mouse`), keybind execution (`commands`), the find bar
//! (`search`) and the overlays (`overlays`). Other parts of the app talk to a
//! view through its public methods, the [`TerminalEvent`]s it emits and the
//! gpui actions it handles (`Copy`, `Paste`, `SelectAll` and the menu actions
//! for find, font size and read-only mode).

mod colors;
mod clipboard;
mod commands;
mod element;
mod exit;
mod font;
mod glyphs;
mod input;
mod links;
mod macos;
mod mouse;
mod overlays;
mod scroll;
mod search;
mod selftest;
pub(crate) mod settings;
pub(crate) mod surface;

use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use angelly_core::config::Keybinds;
use crate::menu::{ContextMenuExt as _, Menu};
use gpui_kit::*;
use terminal::{Frame, Progress, ProgressState, Session, SessionConfig};

use crate::keybinds::Matcher;
use crate::theme::Theme;

use self::element::{Painter, TerminalElement};
use self::font::{FontSet, Metrics};
use self::mouse::MouseState;
use self::scroll::{RowAccumulator, ScrollerVisibility};
use self::search::SearchState;
use self::settings::Settings;
pub use self::clipboard::ClipboardPrompt;
pub(crate) use self::surface::{stage as stage_surface, unstage as unstage_surface, Surface};

actions!(terminal, [Copy, Paste, SelectAll]);

/// Notifications a view raises for the rest of the app. Subscribe with
/// `cx.subscribe(&view, ...)`.
#[derive(Clone, Debug, PartialEq)]
pub enum TerminalEvent {
    /// The bell rang. `has_bell()` stays true until the user interacts with
    /// the view or it regains focus.
    Bell,
    Title(String),
    /// The working directory changed (OSC 7).
    Pwd(String),
    /// OSC 9;4 progress changed; `None` when the report was removed.
    Progress(Option<Progress>),
    /// OSC 9 / OSC 777 desktop notification request.
    Notification {
        title: String,
        body: String,
    },
    /// A command finished (OSC 133;D). `notify` is true when the
    /// `notify-on-command-finish` settings ask for a notification.
    CommandFinished {
        exit_code: Option<i32>,
        duration: Duration,
        notify: bool,
    },
    /// A program reported its status (OSC 7501).
    ProgramStatus(terminal::ProgramStatus),
    /// The program asked to resize the window.
    ResizeRequest {
        cols: u16,
        rows: u16,
    },
    /// `toggle_readonly` changed the read-only state.
    ReadOnlyChanged(bool),
    /// The program's clipboard request needs the user's answer in the window's clipboard sheet.
    Clipboard(ClipboardPrompt),
}

impl EventEmitter<TerminalEvent> for TerminalView {}

/// Text copied by `copy-on-select = primary` and read by `paste_from_selection`.
#[derive(Default)]
pub struct SelectionClipboard(pub Option<String>);

impl Global for SelectionClipboard {}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Layout {
    /// Bounds of the whole element, padding included.
    pub bounds: Bounds<Pixels>,
    /// Top-left corner of the cell grid.
    pub origin: Point<Pixels>,
    pub grid: (u16, u16),
}

/// A transient "C x R" indicator shown while the grid is being resized.
#[derive(Clone, Copy)]
pub(crate) struct ResizeToast {
    pub size: (u16, u16),
    pub until: Instant,
}

pub struct TerminalView {
    pub(crate) session: Session,
    pub(crate) frame: Frame,
    pub(crate) painter: Painter,
    pub(crate) settings: Rc<Settings>,
    pub(crate) fonts: Rc<FontSet>,
    pub(crate) keybinds: Rc<Keybinds>,
    pub(crate) font_size: f32,
    pub(crate) focus: FocusHandle,
    pub(crate) metrics: Option<Metrics>,
    pub(crate) layout: Layout,
    pub(crate) keys: Matcher<input::QueuedKey>,
    pub(crate) mouse: MouseState,
    pub(crate) rows: RowAccumulator,
    pub(crate) scroller: ScrollerVisibility,
    pub(crate) blink_on: bool,
    pub(crate) last_input: Instant,
    pub(crate) preedit: Option<input::Preedit>,
    pub(crate) search: Option<SearchState>,
    pub(crate) hover_link: Option<mouse::Link>,
    pub(crate) bell: bool,
    pub(crate) progress: Option<Progress>,
    pub(crate) readonly: bool,
    pub(crate) mouse_reporting_enabled: bool,
    pub(crate) title_override: Option<String>,
    pub(crate) toast: Option<ResizeToast>,
    pub(crate) stats: element::PaintStats,
    pub(crate) at_prompt: bool,
    pub(crate) command_started: Option<Instant>,
    pub(crate) dimmed: bool,
    pub(crate) created: Instant,
    pub(crate) style_epoch: u64,
    pub(crate) exit: exit::ExitState,
    window_ready: bool,
    focused: bool,
    /// Secure keyboard entry was requested for this view's password prompt.
    secure_input: bool,
    subscriptions: Vec<Subscription>,
    pub(crate) tasks: Tasks,
}

#[derive(Default)]
pub(crate) struct Tasks {
    _wake: Option<Task<()>>,
    _blink: Option<Task<()>>,
    pub autoscroll: Option<Task<()>>,
    pub toast: Option<Task<()>>,
}

const BLINK_INTERVAL: Duration = Duration::from_millis(600);

impl TerminalView {
    pub fn new(cwd: Option<PathBuf>, cx: &mut Context<Self>) -> Self {
        let staged = surface::take_staged(cx).unwrap_or_default();
        let staged = Surface { cwd: cwd.or(staged.cwd), ..staged };
        let (tx, rx) = async_channel::unbounded::<()>();
        let theme = cx.global::<Theme>();
        let settings = Settings::from_config(&theme.config);
        let fonts = FontSet::new(&settings, &theme.config);
        let keybinds = theme.config.keybinds().clone();
        let launch = surface::launch(&theme.config, &settings, staged, &surface::Environment::current());
        let session_config = SessionConfig { colors: Some(theme.colors.clone()), ..launch.session };
        let font_size = launch.font_size;
        let session = Session::spawn(session_config, move || {
            let _ = tx.try_send(());
        })
        .expect("spawn shell");
        if !launch.initial_input.is_empty() {
            session.write(&launch.initial_input);
        }
        session.term().set_default_cursor(settings.cursor_style, settings.cursor_blink.unwrap_or(true));
        cx.set_cursor_hide_mode(hide_mode(&settings));

        let wake = cx.spawn(async move |this, cx| {
            while rx.recv().await.is_ok() {
                if this.update(cx, |this, cx| this.on_wake(cx)).is_err() {
                    break;
                }
            }
        });
        let blink = cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(BLINK_INTERVAL).await;
            if this.update(cx, |this, cx| this.on_blink_tick(cx)).is_err() {
                break;
            }
        });

        let id = cx.entity_id().as_u64();
        cx.on_release(move |this, cx| this.request_secure_input(id, false, cx)).detach();
        let theme_subscription = cx.observe_global::<Theme>(|this, cx| this.reload_config(cx));

        TerminalView {
            session,
            frame: Frame::default(),
            painter: Painter::default(),
            settings: Rc::new(settings),
            fonts: Rc::new(fonts),
            keybinds: Rc::new(keybinds),
            font_size,
            focus: cx.focus_handle(),
            metrics: None,
            layout: Layout::default(),
            keys: Matcher::default(),
            mouse: MouseState::default(),
            rows: RowAccumulator::default(),
            scroller: ScrollerVisibility::default(),
            blink_on: true,
            last_input: Instant::now(),
            preedit: None,
            search: None,
            hover_link: None,
            bell: false,
            progress: None,
            readonly: false,
            mouse_reporting_enabled: true,
            title_override: None,
            toast: None,
            stats: element::PaintStats::default(),
            at_prompt: false,
            command_started: None,
            dimmed: false,
            created: Instant::now(),
            style_epoch: 0,
            exit: exit::ExitState::new(launch.wait_after_command, launch.command_line),
            window_ready: false,
            focused: false,
            secure_input: false,
            subscriptions: vec![theme_subscription],
            tasks: Tasks { _wake: Some(wake), _blink: Some(blink), ..Tasks::default() },
        }
    }

    pub fn title(&self) -> String {
        self.title_override.clone().unwrap_or_else(|| self.session.term().title())
    }

    pub fn cwd(&self) -> Option<PathBuf> {
        self.session.cwd()
    }

    pub fn exit_status(&self) -> Option<terminal::ExitStatus> {
        self.session.exit_status()
    }

    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus
    }

    /// Whether the terminal rang its bell since it was last looked at.
    pub fn has_bell(&self) -> bool {
        self.bell
    }

    pub fn clear_bell(&mut self) {
        self.bell = false;
    }

    /// Whether a command other than the shell runs in the foreground.
    pub fn has_running_command(&self) -> bool {
        self.exit_status().is_none()
            && self.session.foreground_process().is_some_and(|p| Some(p.pid) != self.session.pid())
    }

    pub fn progress(&self) -> Option<Progress> {
        self.progress
    }

    pub fn is_read_only(&self) -> bool {
        self.readonly
    }

    pub fn session(&self) -> &Session {
        &self.session
    }

    /// Whether this view paints the dimming overlay while unfocused. The split
    /// container turns it on while the tab has more than one pane.
    pub fn set_dim_when_unfocused(&mut self, dim: bool, cx: &mut Context<Self>) {
        if self.dimmed != dim {
            self.dimmed = dim;
            cx.notify();
        }
    }

    /// Sends text to the child as typed input.
    pub fn send_text(&mut self, text: &str, cx: &mut Context<Self>) {
        if self.readonly || text.is_empty() {
            return;
        }
        self.session.term().scroll_to_bottom();
        self.session.write(text.as_bytes());
        self.note_input(cx);
    }

    /// Re-reads the settings and colors from the global [`Theme`]. Call after
    /// a config reload.
    pub fn reload_config(&mut self, cx: &mut Context<Self>) {
        let theme = cx.global::<Theme>();
        let settings = Settings::from_config(&theme.config);
        self.fonts = Rc::new(FontSet::new(&settings, &theme.config));
        self.keybinds = Rc::new(theme.config.keybinds().clone());
        {
            let mut term = self.session.term();
            term.set_colors(&theme.colors);
            term.set_option_as_alt(settings.option_as_alt);
            term.set_default_cursor(settings.cursor_style, settings.cursor_blink.unwrap_or(true));
        }
        cx.set_cursor_hide_mode(hide_mode(&settings));
        if settings.font_size != self.settings.font_size {
            self.font_size = settings.font_size;
        }
        self.settings = Rc::new(settings);
        self.keys.reset();
        self.invalidate_metrics();
        cx.notify();
    }

    pub(crate) fn invalidate_metrics(&mut self) {
        self.metrics = None;
        self.style_epoch += 1;
    }

    /// Whether the view has the keyboard focus within an active window.
    pub(crate) fn is_focused(&self) -> bool {
        self.focused
    }

    pub(crate) fn note_input(&mut self, cx: &mut Context<Self>) {
        self.last_input = Instant::now();
        self.blink_on = true;
        if self.bell {
            self.bell = false;
            cx.notify();
        }
    }

    fn on_blink_tick(&mut self, cx: &mut Context<Self>) {
        self.sync_secure_input(cx);
        let blinking = self.focused && self.frame.cursor.blinking && self.frame.cursor.visible;
        if !blinking {
            if !self.blink_on {
                self.blink_on = true;
                cx.notify();
            }
            return;
        }
        if self.last_input.elapsed() < BLINK_INTERVAL {
            return;
        }
        self.blink_on = !self.blink_on;
        cx.notify();
    }

    fn on_wake(&mut self, cx: &mut Context<Self>) {
        let events = {
            let mut term = self.session.term();
            if self.settings.scroll_to_bottom.output && !term.viewport_at_bottom() {
                term.scroll_to_bottom();
            }
            term.take_events()
        };
        for event in events {
            self.handle_event(event, cx);
        }
        self.announce_exit();
        self.sync_secure_input(cx);
        cx.notify();
    }

    /// Once the child has exited, writes the exit note into the terminal and decides whether
    /// the terminal stays open (`wait-after-command`, launch failures).
    fn announce_exit(&mut self) {
        if self.exit.is_announced() {
            return;
        }
        let Some(status) = self.session.exit_status() else { return };
        let exit = exit::Exit {
            status: &status,
            runtime: self.created.elapsed(),
            abnormal_below: self.settings.abnormal_exit_runtime,
            cols: self.layout.grid.0.max(1),
            screen_has_text: self.frame.rows.iter().any(|row| !row.text().is_empty()),
        };
        if let Some(output) = self.exit.announce(&exit) {
            self.session.term().write(&output);
        }
    }

    /// Whether the pane should go away: its child exited and nothing asks to keep it open.
    pub fn should_close(&self) -> bool {
        self.exit.should_close()
    }

    fn handle_event(&mut self, event: terminal::Event, cx: &mut Context<Self>) {
        use terminal::Event;
        match event {
            Event::Bell => self.ring_bell(cx),
            Event::Title(title) => cx.emit(TerminalEvent::Title(title)),
            Event::Pwd(pwd) => cx.emit(TerminalEvent::Pwd(pwd)),
            Event::Progress(p) => {
                self.progress = (p.state != ProgressState::Remove).then_some(p);
                cx.emit(TerminalEvent::Progress(self.progress));
            }
            Event::Notification { title, body } => cx.emit(TerminalEvent::Notification { title, body }),
            Event::ClipboardWrite { location, text } => self.osc52_write(location, text, cx),
            Event::ClipboardRead { location } => self.osc52_read(location, cx),
            Event::ProgramStatus(s) => cx.emit(TerminalEvent::ProgramStatus(s)),
            Event::SemanticPrompt { kind, exit_code } => self.handle_prompt_mark(kind, exit_code, cx),
            Event::ResizeRequest { cols, rows } => cx.emit(TerminalEvent::ResizeRequest { cols, rows }),
        }
    }

    fn handle_prompt_mark(&mut self, kind: terminal::PromptKind, exit_code: Option<i32>, cx: &mut Context<Self>) {
        use terminal::PromptKind::*;
        match kind {
            PromptStart | InputStart => self.at_prompt = true,
            OutputStart => {
                self.at_prompt = false;
                self.command_started = Some(Instant::now());
            }
            CommandEnd => {
                self.at_prompt = true;
                let Some(started) = self.command_started.take() else { return };
                let duration = started.elapsed();
                let s = &self.settings;
                let notify = duration >= s.notify_after
                    && match s.notify_on_finish {
                        settings::NotifyOnFinish::Never => false,
                        settings::NotifyOnFinish::Always => true,
                        settings::NotifyOnFinish::Unfocused => !self.focused,
                    };
                cx.emit(TerminalEvent::CommandFinished { exit_code, duration, notify });
            }
        }
    }

    fn ring_bell(&mut self, cx: &mut Context<Self>) {
        let features = self.settings.bell;
        if features.system {
            macos::beep();
        }
        if features.audio {
            if let Some(path) = &self.settings.bell_audio_path {
                macos::play_audio(path, self.settings.bell_audio_volume);
            }
        }
        if !self.focused || features.border {
            self.bell = true;
        }
        cx.emit(TerminalEvent::Bell);
    }

    /// Hooks that need a window: focus tracking, window background and the
    /// selftest script.
    fn init_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.window_ready = true;
        let focus = self.focus.clone();
        self.subscriptions.push(cx.on_focus_in(&focus, window, |this, window, cx| this.focus_changed(window, cx)));
        self.subscriptions.push(cx.on_focus_out(&focus, window, |this, _, window, cx| this.focus_changed(window, cx)));
        self.subscriptions
            .push(cx.observe_window_activation(window, |this, window, cx| this.focus_changed(window, cx)));
        self.focused = self.focus.is_focused(window) && window.is_window_active();
        window.set_background_appearance(window_background(&self.settings));
        selftest::start(self, window, cx);
    }

    /// Asks for secure keyboard entry while this focused view shows a password prompt.
    fn sync_secure_input(&mut self, cx: &mut Context<Self>) {
        let wanted = self.focused && self.session.password_input();
        self.request_secure_input(cx.entity_id().as_u64(), wanted, cx);
    }

    fn request_secure_input(&mut self, id: u64, wanted: bool, cx: &mut App) {
        if self.secure_input != wanted {
            self.secure_input = wanted;
            crate::platform::secure_input::request(id, wanted, cx);
        }
    }

    fn focus_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let focused = self.focus.is_focused(window) && window.is_window_active();
        if focused == self.focused {
            return;
        }
        self.focused = focused;
        self.blink_on = true;
        self.last_input = Instant::now();
        if focused {
            self.bell = false;
        } else {
            self.keys.reset();
            self.hover_link = None;
        }
        self.sync_secure_input(cx);
        let bytes = self.session.term().encode_focus(focused);
        if !bytes.is_empty() {
            self.session.write(&bytes);
        }
        cx.notify();
    }

    /// Reports a new grid size to the child and starts the resize toast.
    pub(crate) fn apply_grid(&mut self, cols: u16, rows: u16, cx: &mut Context<Self>) {
        let Some(metrics) = self.metrics else { return };
        let (cw, ch) = metrics.device_cell();
        self.session.resize(cols, rows, cw, ch);
        let first = self.layout.grid == (0, 0);
        self.layout.grid = (cols, rows);
        if first && self.settings.resize_overlay != settings::ResizeOverlay::Always {
            return;
        }
        self.show_resize_toast(cols, rows, cx);
    }
}

fn hide_mode(settings: &Settings) -> CursorHideMode {
    if settings.hide_while_typing {
        CursorHideMode::OnTyping
    } else {
        CursorHideMode::Never
    }
}

/// The window background appearance the settings call for: opaque, or
/// transparent (blurred with `background-blur`) when the background is
/// see-through. The view applies it when its window first renders; apply it
/// again from the window owner after config reloads.
pub fn window_background(settings: &Settings) -> WindowBackgroundAppearance {
    if settings.background_opacity >= 1.0 {
        WindowBackgroundAppearance::Opaque
    } else if settings.blur {
        WindowBackgroundAppearance::Blurred
    } else {
        WindowBackgroundAppearance::Transparent
    }
}

impl Render for TerminalView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.window_ready {
            self.init_window(window, cx);
        }
        let focus = self.focus.clone();
        let root = div()
            .id("terminal")
            .size_full()
            .relative()
            .key_context("Terminal")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key_down))
            .on_key_up(cx.listener(Self::on_key_up))
            .on_modifiers_changed(cx.listener(Self::on_modifiers_changed))
            .on_action(cx.listener(Self::copy_action))
            .on_action(cx.listener(Self::paste_action))
            .on_action(cx.listener(Self::select_all_action));
        self.menu_actions(root, cx)
            .child(TerminalElement::new(cx.entity()))
            .children(self.render_overlays(window, cx))
            .context_menu(move |_, _| context_menu(&focus))
    }
}

fn context_menu(focus: &FocusHandle) -> Menu {
    use crate::platform::menus::{ChangeTerminalTitle, Find, ToggleReadonly};
    use crate::splits::{SplitDown, SplitLeft, SplitRight, SplitUp};
    Menu::new().action_context(focus.clone())
        .action("Copy", Box::new(Copy))
        .action("Paste", Box::new(Paste))
        .action("Select All", Box::new(SelectAll))
        .separator()
        .action("Find\u{2026}", Box::new(Find))
        .separator()
        .action("Split Right", Box::new(SplitRight))
        .action("Split Left", Box::new(SplitLeft))
        .action("Split Down", Box::new(SplitDown))
        .action("Split Up", Box::new(SplitUp))
        .separator()
        .action("Terminal Read-only", Box::new(ToggleReadonly))
        .action("Change Terminal Title\u{2026}", Box::new(ChangeTerminalTitle))
}

impl Focusable for TerminalView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
