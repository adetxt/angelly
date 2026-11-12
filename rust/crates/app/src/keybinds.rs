//! Ghostty keybind dispatch.
//!
//! The user's `keybind` config entries (parsed by `angelly_core::config`) are
//! matched against key presses by a [`Matcher`], and the winning binding's
//! action is parsed into a [`Command`]:
//!
//! * [`Command::Send`] covers `text:`, `csi:` and `esc:`; the bytes go to the
//!   child process.
//! * [`Command::Surface`] actions act on one terminal (copy, paste, scroll,
//!   search, font size, ...). The focused [`TerminalView`] executes them.
//! * [`Command::App`] actions concern the window, tabs and splits. The view
//!   does not know how to run them: it dispatches [`Perform`] through gpui so
//!   that whichever ancestor owns the feature handles it.
//! * [`Command::Table`] manages key tables and is handled by the matcher's
//!   owner.
//!
//! # Handling app-level actions
//!
//! `Perform` is a single gpui action carrying an [`AppAction`]. A handler is
//! registered on any element above the terminal (usually the workspace):
//!
//! ```ignore
//! root.on_action(cx.listener(|this, Perform(action), window, cx| match action {
//!     AppAction::NewTab => this.add_tab(None, window, cx),
//!     AppAction::GotoTab(n) => this.select(n - 1, window, cx),
//!     _ => cx.propagate(),
//! }))
//! ```
//!
//! A handler that does not recognize an action calls `cx.propagate()` so an
//! outer handler can try. Bindings flagged `global:` are not dispatched from
//! here; the platform layer registers those as system-wide hot keys.
//!
//! [`TerminalView`]: crate::terminal_view::TerminalView

use angelly_core::config::{Action as ConfigAction, Binding, BindingSet, Keybinds, Mods, Trigger, TriggerKey};
use gpui_kit::{Action, Keystroke};
use terminal::{Key, TextFormat};

/// The single gpui action used to hand an [`AppAction`] to the rest of the app.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = angelly, no_json, no_register)]
pub struct Perform(pub AppAction);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplitDirection {
    Right,
    Down,
    Left,
    Up,
    /// Right or down depending on the shape of the surface being split.
    Auto,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplitFocus {
    Previous,
    Next,
    Up,
    Left,
    Down,
    Right,
    Top,
    Bottom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseTabMode {
    This,
    Other,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TitleTarget {
    Surface,
    Tab,
    Window,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InspectorMode {
    Toggle,
    Show,
    Hide,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenConfigTarget {
    OsOpen,
    NewWindow,
}

/// Actions the terminal view cannot run itself. See the module docs.
#[derive(Clone, Debug, PartialEq)]
pub enum AppAction {
    NewWindow,
    NewTab,
    PreviousTab,
    NextTab,
    LastTab,
    /// One-based tab number.
    GotoTab(usize),
    MoveTab(isize),
    MoveTabToNewWindow,
    ToggleTabOverview,
    /// Ask the user for a new title.
    PromptTitle(TitleTarget),
    /// Set a title directly (`set_tab_title`, `set_window_title`).
    SetTitle(TitleTarget, String),
    NewSplit(SplitDirection),
    GotoSplit(SplitFocus),
    GotoWindow {
        next: bool,
    },
    ToggleSplitZoom,
    ResizeSplit(Direction, u16),
    EqualizeSplits,
    ResetWindowSize,
    Inspector(InspectorMode),
    OpenConfig(OpenConfigTarget),
    ReloadConfig,
    /// Close the focused split (and its tab or window when it is the last).
    CloseSurface,
    CloseTab(CloseTabMode),
    CloseWindow,
    CloseAllWindows,
    ToggleMaximize,
    ToggleFullscreen,
    ToggleWindowDecorations,
    ToggleWindowFloatOnTop,
    ToggleSecureInput,
    ToggleCommandPalette,
    ToggleQuickTerminal,
    ToggleVisibility,
    ToggleBackgroundOpacity,
    CheckForUpdates,
    Undo,
    Redo,
    Quit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyFormat {
    Plain,
    Vt,
    Html,
    Mixed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchDirection {
    Previous,
    Next,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Adjust {
    Left,
    Right,
    Up,
    Down,
    PageUp,
    PageDown,
    Home,
    End,
    BeginningOfLine,
    EndOfLine,
    LeftWord,
    RightWord,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileSource {
    Scrollback,
    Screen,
    Selection,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileDelivery {
    Copy,
    Paste,
    Open,
}

/// Actions executed by the focused terminal.
#[derive(Clone, Debug, PartialEq)]
pub enum SurfaceAction {
    Reset,
    CopyToClipboard(CopyFormat),
    PasteFromClipboard,
    PasteFromSelection,
    CopyUrlToClipboard,
    CopyTitleToClipboard,
    IncreaseFontSize(f32),
    DecreaseFontSize(f32),
    ResetFontSize,
    SetFontSize(f32),
    Search(String),
    SearchSelection,
    NavigateSearch(SearchDirection),
    StartSearch,
    EndSearch,
    ClearScreen,
    SelectAll,
    ScrollToTop,
    ScrollToBottom,
    ScrollToSelection,
    ScrollToRow(usize),
    /// Scroll by a fraction of the viewport height (negative is up).
    ScrollPage(f32),
    ScrollLines(i16),
    AdjustSelection(Adjust),
    JumpToPrompt(i16),
    WriteFile(FileSource, FileDelivery, TextFormat),
    ToggleReadonly,
    ToggleMouseReporting,
    SetSurfaceTitle(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TableAction {
    Activate { name: String, once: bool },
    Deactivate,
    DeactivateAll,
    EndSequence,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Ignore,
    Send(Vec<u8>),
    Surface(SurfaceAction),
    App(AppAction),
    Table(TableAction),
}

impl Command {
    /// Interprets a config action. `None` for actions this app has no meaning
    /// for (`crash`, GTK-only actions, ...); such bindings pass the key through.
    pub fn parse(action: &ConfigAction) -> Option<Command> {
        let param = action.param.as_deref();
        let num = |default: f32| param.and_then(|p| p.parse().ok()).unwrap_or(default);
        use AppAction as A;
        use SurfaceAction as S;
        let surface = |s| Some(Command::Surface(s));
        let app = |a| Some(Command::App(a));
        match action.name.as_str() {
            "ignore" => Some(Command::Ignore),
            "text" => Some(Command::Send(unescape(param?))),
            "csi" => Some(Command::Send([b"\x1b[".as_slice(), param?.as_bytes()].concat())),
            "esc" => Some(Command::Send([b"\x1b".as_slice(), param?.as_bytes()].concat())),
            "reset" => surface(S::Reset),
            "copy_to_clipboard" => surface(S::CopyToClipboard(match param {
                Some("plain") => CopyFormat::Plain,
                Some("vt") => CopyFormat::Vt,
                Some("html") => CopyFormat::Html,
                _ => CopyFormat::Mixed,
            })),
            "paste_from_clipboard" => surface(S::PasteFromClipboard),
            "paste_from_selection" => surface(S::PasteFromSelection),
            "copy_url_to_clipboard" => surface(S::CopyUrlToClipboard),
            "copy_title_to_clipboard" => surface(S::CopyTitleToClipboard),
            "increase_font_size" => surface(S::IncreaseFontSize(num(1.0))),
            "decrease_font_size" => surface(S::DecreaseFontSize(num(1.0))),
            "reset_font_size" => surface(S::ResetFontSize),
            "set_font_size" => surface(S::SetFontSize(num(0.0))),
            "search" => surface(S::Search(param?.to_string())),
            "search_selection" => surface(S::SearchSelection),
            "navigate_search" => surface(S::NavigateSearch(match param? {
                "previous" => SearchDirection::Previous,
                _ => SearchDirection::Next,
            })),
            "start_search" => surface(S::StartSearch),
            "end_search" => surface(S::EndSearch),
            "clear_screen" => surface(S::ClearScreen),
            "select_all" => surface(S::SelectAll),
            "scroll_to_top" => surface(S::ScrollToTop),
            "scroll_to_bottom" => surface(S::ScrollToBottom),
            "scroll_to_selection" => surface(S::ScrollToSelection),
            "scroll_to_row" => surface(S::ScrollToRow(param?.parse().ok()?)),
            "scroll_page_up" => surface(S::ScrollPage(-1.0)),
            "scroll_page_down" => surface(S::ScrollPage(1.0)),
            "scroll_page_fractional" => surface(S::ScrollPage(num(0.0))),
            "scroll_page_lines" => surface(S::ScrollLines(param?.parse().ok()?)),
            "adjust_selection" => surface(S::AdjustSelection(match param? {
                "left" => Adjust::Left,
                "right" => Adjust::Right,
                "up" => Adjust::Up,
                "down" => Adjust::Down,
                "page_up" => Adjust::PageUp,
                "page_down" => Adjust::PageDown,
                "home" => Adjust::Home,
                "end" => Adjust::End,
                "beginning_of_line" => Adjust::BeginningOfLine,
                "end_of_line" => Adjust::EndOfLine,
                "left_word" => Adjust::LeftWord,
                "right_word" => Adjust::RightWord,
                _ => return None,
            })),
            "jump_to_prompt" => surface(S::JumpToPrompt(param?.parse().ok()?)),
            "write_scrollback_file" => write_file(FileSource::Scrollback, param),
            "write_screen_file" => write_file(FileSource::Screen, param),
            "write_selection_file" => write_file(FileSource::Selection, param),
            "toggle_readonly" => surface(S::ToggleReadonly),
            "toggle_mouse_reporting" => surface(S::ToggleMouseReporting),
            "set_surface_title" => surface(S::SetSurfaceTitle(param?.to_string())),
            "prompt_surface_title" => app(A::PromptTitle(TitleTarget::Surface)),
            "prompt_tab_title" => app(A::PromptTitle(TitleTarget::Tab)),
            "prompt_window_title" => app(A::PromptTitle(TitleTarget::Window)),
            "set_tab_title" => app(A::SetTitle(TitleTarget::Tab, param?.to_string())),
            "set_window_title" => app(A::SetTitle(TitleTarget::Window, param?.to_string())),
            "new_window" => app(A::NewWindow),
            "new_tab" => app(A::NewTab),
            "previous_tab" => app(A::PreviousTab),
            "next_tab" => app(A::NextTab),
            "last_tab" => app(A::LastTab),
            "goto_tab" => app(A::GotoTab(param?.parse().ok()?)),
            "move_tab" => app(A::MoveTab(param?.parse().ok()?)),
            "move_tab_to_new_window" => app(A::MoveTabToNewWindow),
            "toggle_tab_overview" => app(A::ToggleTabOverview),
            "new_split" => app(A::NewSplit(match param.unwrap_or("auto") {
                "right" => SplitDirection::Right,
                "down" => SplitDirection::Down,
                "left" => SplitDirection::Left,
                "up" => SplitDirection::Up,
                _ => SplitDirection::Auto,
            })),
            "goto_split" => app(A::GotoSplit(match param? {
                "previous" => SplitFocus::Previous,
                "next" => SplitFocus::Next,
                "up" => SplitFocus::Up,
                "left" => SplitFocus::Left,
                "down" => SplitFocus::Down,
                "right" => SplitFocus::Right,
                "top" => SplitFocus::Top,
                "bottom" => SplitFocus::Bottom,
                _ => return None,
            })),
            "goto_window" => app(A::GotoWindow { next: param? == "next" }),
            "toggle_split_zoom" => app(A::ToggleSplitZoom),
            "resize_split" => {
                let (dir, amount) = param?.split_once(',')?;
                let dir = match dir {
                    "up" => Direction::Up,
                    "down" => Direction::Down,
                    "left" => Direction::Left,
                    "right" => Direction::Right,
                    _ => return None,
                };
                app(A::ResizeSplit(dir, amount.parse().ok()?))
            }
            "equalize_splits" => app(A::EqualizeSplits),
            "reset_window_size" => app(A::ResetWindowSize),
            "inspector" => app(A::Inspector(match param? {
                "show" => InspectorMode::Show,
                "hide" => InspectorMode::Hide,
                _ => InspectorMode::Toggle,
            })),
            "open_config" => app(A::OpenConfig(match param {
                Some("new_window") => OpenConfigTarget::NewWindow,
                _ => OpenConfigTarget::OsOpen,
            })),
            "reload_config" => app(A::ReloadConfig),
            "close_surface" => app(A::CloseSurface),
            "close_tab" => app(A::CloseTab(match param {
                Some("other") => CloseTabMode::Other,
                Some("right") => CloseTabMode::Right,
                _ => CloseTabMode::This,
            })),
            "close_window" => app(A::CloseWindow),
            "close_all_windows" => app(A::CloseAllWindows),
            "toggle_maximize" => app(A::ToggleMaximize),
            "toggle_fullscreen" => app(A::ToggleFullscreen),
            "toggle_window_decorations" => app(A::ToggleWindowDecorations),
            "toggle_window_float_on_top" => app(A::ToggleWindowFloatOnTop),
            "toggle_secure_input" => app(A::ToggleSecureInput),
            "toggle_command_palette" => app(A::ToggleCommandPalette),
            "toggle_quick_terminal" => app(A::ToggleQuickTerminal),
            "toggle_visibility" => app(A::ToggleVisibility),
            "toggle_background_opacity" => app(A::ToggleBackgroundOpacity),
            "check_for_updates" => app(A::CheckForUpdates),
            "undo" => app(A::Undo),
            "redo" => app(A::Redo),
            "quit" => app(A::Quit),
            "activate_key_table" => {
                Some(Command::Table(TableAction::Activate { name: param?.to_string(), once: false }))
            }
            "activate_key_table_once" => {
                Some(Command::Table(TableAction::Activate { name: param?.to_string(), once: true }))
            }
            "deactivate_key_table" => Some(Command::Table(TableAction::Deactivate)),
            "deactivate_all_key_tables" => Some(Command::Table(TableAction::DeactivateAll)),
            "end_key_sequence" => Some(Command::Table(TableAction::EndSequence)),
            _ => None,
        }
    }
}

fn write_file(source: FileSource, param: Option<&str>) -> Option<Command> {
    let mut parts = param?.split(',');
    let delivery = match parts.next() {
        Some("copy") => FileDelivery::Copy,
        Some("paste") => FileDelivery::Paste,
        Some("open") => FileDelivery::Open,
        _ => return None,
    };
    let format = match parts.next() {
        Some("vt") => TextFormat::Vt,
        Some("html") => TextFormat::Html,
        _ => TextFormat::Plain,
    };
    Some(Command::Surface(SurfaceAction::WriteFile(source, delivery, format)))
}

/// Decodes the string-literal escapes accepted by `text:` bindings: `\n`,
/// `\r`, `\t`, `\\`, `\"`, `\'`, `\e`, `\xNN` and `\u{N...}`. Anything else is
/// kept as written.
pub fn unescape(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    let push_char = |out: &mut Vec<u8>, c: char| {
        let mut buf = [0u8; 4];
        out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
    };
    while let Some(c) = chars.next() {
        if c != '\\' {
            push_char(&mut out, c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push(b'\n'),
            Some('r') => out.push(b'\r'),
            Some('t') => out.push(b'\t'),
            Some('e') => out.push(0x1b),
            Some('\\') => out.push(b'\\'),
            Some('"') => out.push(b'"'),
            Some('\'') => out.push(b'\''),
            Some('x') => {
                let hex: String = (0..2).filter_map(|_| chars.next_if(char::is_ascii_hexdigit)).collect();
                match u8::from_str_radix(&hex, 16) {
                    Ok(b) if hex.len() == 2 => out.push(b),
                    _ => out.extend_from_slice(format!("\\x{hex}").as_bytes()),
                }
            }
            Some('u') if chars.peek() == Some(&'{') => {
                chars.next();
                let mut hex = String::new();
                while let Some(h) = chars.next_if(|c| *c != '}') {
                    hex.push(h);
                }
                chars.next();
                match u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                    Some(c) => push_char(&mut out, c),
                    None => out.extend_from_slice(format!("\\u{{{hex}}}").as_bytes()),
                }
            }
            Some(other) => {
                out.push(b'\\');
                push_char(&mut out, other);
            }
            None => out.push(b'\\'),
        }
    }
    out
}

/// US-layout symbol keys: unshifted character, shifted character, W3C key name.
const SYMBOLS: &[(char, char, &str)] = &[
    ('`', '~', "backquote"),
    ('1', '!', "digit_1"),
    ('2', '@', "digit_2"),
    ('3', '#', "digit_3"),
    ('4', '$', "digit_4"),
    ('5', '%', "digit_5"),
    ('6', '^', "digit_6"),
    ('7', '&', "digit_7"),
    ('8', '*', "digit_8"),
    ('9', '(', "digit_9"),
    ('0', ')', "digit_0"),
    ('-', '_', "minus"),
    ('=', '+', "equal"),
    ('[', '{', "bracket_left"),
    (']', '}', "bracket_right"),
    ('\\', '|', "backslash"),
    (';', ':', "semicolon"),
    ('\'', '"', "quote"),
    (',', '<', "comma"),
    ('.', '>', "period"),
    ('/', '?', "slash"),
    (' ', ' ', "space"),
];

const LETTER_KEYS: [&str; 26] = [
    "key_a", "key_b", "key_c", "key_d", "key_e", "key_f", "key_g", "key_h", "key_i", "key_j", "key_k", "key_l",
    "key_m", "key_n", "key_o", "key_p", "key_q", "key_r", "key_s", "key_t", "key_u", "key_v", "key_w", "key_x",
    "key_y", "key_z",
];

const NAMED_KEYS: &[(&str, &str)] = &[
    ("up", "arrow_up"),
    ("down", "arrow_down"),
    ("left", "arrow_left"),
    ("right", "arrow_right"),
    ("pageup", "page_up"),
    ("pagedown", "page_down"),
    ("home", "home"),
    ("end", "end"),
    ("insert", "insert"),
    ("delete", "delete"),
    ("backspace", "backspace"),
    ("enter", "enter"),
    ("escape", "escape"),
    ("tab", "tab"),
    ("space", "space"),
];

/// A key press reduced to what trigger matching needs.
#[derive(Clone, Debug, PartialEq)]
pub struct Chord {
    /// The key as a UI toolkit names it, with symbols reduced to their
    /// unshifted form (`"["` for a `{` press, `"pageup"`, `"a"`). Feed this to
    /// `terminal::Key::from_name`.
    pub base: String,
    /// W3C key name on a US layout, when the key has one.
    physical: Option<&'static str>,
    /// Modifiers held, counting Shift when it produced a shifted symbol.
    pub mods: Mods,
    /// How a `Unicode` trigger may spell this press: the produced character
    /// with the modifiers left over once the layout consumed Shift, and the
    /// unshifted character with Shift still counted.
    unicode: Vec<(char, Mods)>,
}

impl Chord {
    pub fn new(key: &str, mods: Mods) -> Chord {
        let mut chars = key.chars();
        let (first, single) = match (chars.next(), chars.next()) {
            (Some(c), None) => (Some(c), true),
            _ => (None, false),
        };
        if !single {
            let physical = NAMED_KEYS.iter().find(|(n, _)| *n == key).map(|(_, p)| *p).or_else(|| {
                let digits = key.strip_prefix('f')?;
                let n: u8 = digits.parse().ok()?;
                (1..=25).contains(&n).then(|| F_KEYS[usize::from(n) - 1])
            });
            return Chord { base: key.to_string(), physical, mods, unicode: Vec::new() };
        }
        let c = first.expect("single char");
        let lower: char = c.to_lowercase().next().unwrap_or(c);
        if let Some((base, _, physical)) = SYMBOLS.iter().find(|(b, s, _)| *b == c || *s == c && *b != *s) {
            let shifted = c != *base;
            let mut with_shift = mods;
            with_shift.shift |= shifted;
            let mut unicode = vec![(*base, with_shift)];
            if shifted {
                unicode.push((c, mods));
            }
            return Chord { base: base.to_string(), physical: Some(physical), mods: with_shift, unicode };
        }
        let mut mods = mods;
        mods.shift |= lower != c;
        let physical = lower.is_ascii_lowercase().then(|| LETTER_KEYS[usize::from(lower as u8 - b'a')]);
        Chord { base: lower.to_string(), physical, mods, unicode: vec![(lower, mods)] }
    }

    /// `key_code` is the platform's virtual key code of the press, when known. It names the key
    /// by position: layouts without ASCII letters (Cyrillic, Greek, Hebrew...) are matched by
    /// the US key they sit on, and `physical` triggers follow the position on every layout.
    pub fn from_keystroke(ks: &Keystroke, key_code: Option<u16>) -> Chord {
        let m = ks.modifiers;
        let mods = Mods { shift: m.shift, ctrl: m.control, alt: m.alt, super_key: m.platform };
        let chord = Chord::new(&ks.key, mods);
        let Some(positional) = key_code.and_then(Key::us_char_for_mac_keycode) else { return chord };
        let positional = Chord::new(&positional.to_string(), mods);
        if ks.key.is_ascii() {
            Chord { physical: positional.physical, ..chord }
        } else {
            positional
        }
    }

    pub fn matches(&self, trigger: &Trigger) -> bool {
        match &trigger.key {
            TriggerKey::CatchAll => trigger.mods == self.mods,
            TriggerKey::Physical(name) => trigger.mods == self.mods && self.physical == Some(name.as_str()),
            TriggerKey::Unicode(c) => {
                let c = c.to_lowercase().next().unwrap_or(*c);
                self.unicode.iter().any(|(u, m)| *u == c && *m == trigger.mods)
            }
        }
    }
}

const F_KEYS: [&str; 25] = [
    "f1", "f2", "f3", "f4", "f5", "f6", "f7", "f8", "f9", "f10", "f11", "f12", "f13", "f14", "f15", "f16", "f17",
    "f18", "f19", "f20", "f21", "f22", "f23", "f24", "f25",
];

#[derive(Debug)]
pub enum Outcome<'a, T> {
    /// No binding applies; the key is ordinary input.
    Unbound,
    /// The key started or continued a sequence and is held back.
    Pending,
    Match(&'a Binding),
    /// A sequence was abandoned. Replay these held-back keys as ordinary
    /// input, then process the current key again.
    Abort(Vec<T>),
}

#[derive(Clone, Debug)]
struct ActiveTable {
    name: String,
    once: bool,
}

/// Matches key presses against a [`Keybinds`] set, tracking multi-key
/// sequences and the stack of active key tables. `T` is whatever the caller
/// needs to replay a held-back key.
#[derive(Debug)]
pub struct Matcher<T> {
    pending: Vec<(Chord, T)>,
    tables: Vec<ActiveTable>,
}

impl<T> Default for Matcher<T> {
    fn default() -> Self {
        Matcher { pending: Vec::new(), tables: Vec::new() }
    }
}

impl<T> Matcher<T> {
    pub fn process<'a>(&mut self, binds: &'a Keybinds, chord: Chord, payload: T) -> Outcome<'a, T> {
        let consulted_once = self.tables.last().is_some_and(|t| t.once);
        let mut sequence: Vec<&Chord> = self.pending.iter().map(|(c, _)| c).collect();
        sequence.push(&chord);

        let mut exact: Option<&Binding> = None;
        let mut extends = false;
        for set in self.active_sets(binds) {
            for b in set.iter().filter(|b| !b.flags.global && b.triggers.len() >= sequence.len()) {
                let hit = sequence.iter().zip(&b.triggers).all(|(c, t)| c.matches(t));
                if !hit {
                    continue;
                }
                if b.triggers.len() > sequence.len() {
                    extends = true;
                    continue;
                }
                let catch_all = matches!(b.trigger().key, TriggerKey::CatchAll);
                let better = match exact {
                    None => true,
                    Some(prev) => matches!(prev.trigger().key, TriggerKey::CatchAll) && !catch_all,
                };
                if better {
                    exact = Some(b);
                }
            }
            if exact.is_some() || extends {
                break;
            }
        }

        if let Some(b) = exact {
            self.pending.clear();
            self.finish_key(consulted_once);
            return Outcome::Match(b);
        }
        if extends {
            self.pending.push((chord, payload));
            return Outcome::Pending;
        }
        if !self.pending.is_empty() {
            let held = self.pending.drain(..).map(|(_, p)| p).collect();
            return Outcome::Abort(held);
        }
        self.finish_key(consulted_once);
        Outcome::Unbound
    }

    fn finish_key(&mut self, consulted_once: bool) {
        if consulted_once {
            self.tables.pop();
        }
    }

    fn active_sets<'a>(&self, binds: &'a Keybinds) -> Vec<&'a BindingSet> {
        self.tables.iter().rev().filter_map(|t| binds.tables.get(&t.name)).chain(std::iter::once(&binds.set)).collect()
    }

    pub fn apply(&mut self, action: &TableAction) {
        match action {
            TableAction::Activate { name, once } => self.tables.push(ActiveTable { name: name.clone(), once: *once }),
            TableAction::Deactivate => {
                self.tables.pop();
            }
            TableAction::DeactivateAll => self.tables.clear(),
            TableAction::EndSequence => self.pending.clear(),
        }
    }

    /// Names of the active key tables, outermost first.
    pub fn active_tables(&self) -> impl Iterator<Item = &str> {
        self.tables.iter().map(|t| t.name.as_str())
    }

    pub fn is_idle(&self) -> bool {
        self.pending.is_empty() && self.tables.is_empty()
    }

    /// Forgets any pending sequence and active tables (focus loss, config reload).
    pub fn reset(&mut self) {
        self.pending.clear();
        self.tables.clear();
    }

    /// The held-back keys in order, for display.
    pub fn pending_chords(&self) -> impl Iterator<Item = &Chord> {
        self.pending.iter().map(|(c, _)| c)
    }
}

#[cfg(test)]
mod tests {
    use std::prelude::v1::test;

    use super::*;

    fn binds(lines: &[&str]) -> Keybinds {
        let mut b = Keybinds::default();
        for l in lines {
            b.apply(l, true).unwrap();
        }
        b
    }

    fn chord(s: &str) -> Chord {
        Chord::from_keystroke(&Keystroke::parse_with_os(s, gpui_kit::OperatingSystem::Mac).unwrap(), None)
    }

    #[test]
    fn layouts_without_ascii_are_matched_by_key_position() {
        let ks = Keystroke::parse_with_os("ctrl-с", gpui_kit::OperatingSystem::Mac).unwrap();
        assert_eq!(Chord::from_keystroke(&ks, None).base, "с");
        let by_position = Chord::from_keystroke(&ks, Some(0x08));
        assert_eq!(by_position.base, "c");
        assert!(by_position.mods.ctrl);
        assert_eq!(by_position, chord("ctrl-c"));

        let shifted = Keystroke::parse_with_os("shift-с", gpui_kit::OperatingSystem::Mac).unwrap();
        assert_eq!(Chord::from_keystroke(&shifted, Some(0x08)), chord("shift-c"));
    }

    #[test]
    fn ascii_layouts_keep_their_characters_but_gain_the_position() {
        // Dvorak: the key at the US S position types "o".
        let ks = Keystroke::parse_with_os("ctrl-o", gpui_kit::OperatingSystem::Mac).unwrap();
        let chord = Chord::from_keystroke(&ks, Some(0x01));
        assert_eq!(chord.base, "o");
        assert_eq!(chord.physical, Some("key_s"));
    }

    fn action<'a>(o: Outcome<'a, ()>) -> Option<&'a Binding> {
        match o {
            Outcome::Match(b) => Some(b),
            _ => None,
        }
    }

    #[test]
    fn default_macos_bindings_match_gpui_keystrokes() {
        let b = Keybinds::defaults(true);
        let mut m = Matcher::<()>::default();
        let name = |s: &str, m: &mut Matcher<()>| action(m.process(&b, chord(s), ())).map(|b| b.action.to_string());
        assert_eq!(name("cmd-t", &mut m).as_deref(), Some("new_tab"));
        assert_eq!(name("cmd-shift-[", &mut m).as_deref(), Some("previous_tab"));
        assert_eq!(name("cmd-{", &mut m).as_deref(), Some("previous_tab"));
        assert_eq!(name("cmd-shift-]", &mut m).as_deref(), Some("next_tab"));
        assert_eq!(name("cmd-+", &mut m).as_deref(), Some("increase_font_size:1"));
        assert_eq!(name("cmd-=", &mut m).as_deref(), Some("increase_font_size:1"));
        assert_eq!(name("cmd--", &mut m).as_deref(), Some("decrease_font_size:1"));
        assert_eq!(name("cmd-0", &mut m).as_deref(), Some("reset_font_size"));
        assert_eq!(name("cmd-3", &mut m).as_deref(), Some("goto_tab:3"));
        assert_eq!(name("cmd-9", &mut m).as_deref(), Some("last_tab"));
        assert_eq!(name("cmd-left", &mut m).as_deref(), Some("text:\\x01"));
        assert_eq!(name("alt-left", &mut m).as_deref(), Some("esc:b"));
        assert_eq!(name("cmd-alt-up", &mut m).as_deref(), Some("goto_split:up"));
        assert_eq!(name("ctrl-tab", &mut m).as_deref(), Some("next_tab"));
        assert_eq!(name("a", &mut m), None);
        assert_eq!(name("cmd-shift-d", &mut m).as_deref(), Some("new_split:down"));
    }

    #[test]
    fn chord_normalizes_shifted_symbols() {
        let c = chord("cmd-{");
        assert_eq!(c.base, "[");
        assert!(c.mods.shift && c.mods.super_key);
        let c = Chord::new("A", Mods::NONE);
        assert_eq!(c.base, "a");
        assert!(c.mods.shift);
        let c = chord("pageup");
        assert_eq!(c.base, "pageup");
        assert!(c.matches(&Trigger::parse("page_up").unwrap()));
        assert!(chord("f5").matches(&Trigger::parse("f5").unwrap()));
        assert!(chord("space").matches(&Trigger::parse("space").unwrap()));
    }

    #[test]
    fn physical_and_unicode_triggers_agree() {
        let c = chord("cmd-1");
        assert!(c.matches(&Trigger::parse("super+digit_1").unwrap()));
        assert!(c.matches(&Trigger::parse("super+1").unwrap()));
        assert!(!c.matches(&Trigger::parse("1").unwrap()));
        assert!(chord("cmd-,").matches(&Trigger::parse("super+,").unwrap()));
        assert!(chord("cmd-shift-,").matches(&Trigger::parse("super+shift+,").unwrap()));
    }

    #[test]
    fn sequences_hold_keys_and_fire() {
        let b = binds(&["ctrl+a>n=new_tab", "ctrl+a>shift+n=new_window"]);
        let mut m = Matcher::<char>::default();
        assert!(matches!(m.process(&b, chord("ctrl-a"), 'a'), Outcome::Pending));
        assert_eq!(m.pending_chords().count(), 1);
        match m.process(&b, chord("n"), 'n') {
            Outcome::Match(b) => assert_eq!(b.action.name, "new_tab"),
            other => panic!("{other:?}"),
        }
        assert!(m.is_idle());
        assert!(matches!(m.process(&b, chord("ctrl-a"), 'a'), Outcome::Pending));
        match m.process(&b, chord("shift-n"), 'N') {
            Outcome::Match(b) => assert_eq!(b.action.name, "new_window"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn broken_sequence_returns_held_keys() {
        let b = binds(&["ctrl+a>n=new_tab", "x=new_window"]);
        let mut m = Matcher::<char>::default();
        assert!(matches!(m.process(&b, chord("ctrl-a"), 'a'), Outcome::Pending));
        match m.process(&b, chord("q"), 'q') {
            Outcome::Abort(held) => assert_eq!(held, vec!['a']),
            other => panic!("{other:?}"),
        }
        assert!(matches!(m.process(&b, chord("q"), 'q'), Outcome::Unbound));
        assert!(matches!(m.process(&b, chord("x"), 'x'), Outcome::Match(_)));
    }

    #[test]
    fn key_tables_shadow_root_and_once_pops() {
        let b = binds(&[
            "ctrl+t=activate_key_table:nav",
            "ctrl+o=activate_key_table_once:nav",
            "nav/j=scroll_page_lines:1",
            "nav/escape=deactivate_key_table",
            "j=text:root",
        ]);
        let mut m = Matcher::<()>::default();
        let cmd = |o: Outcome<()>| match o {
            Outcome::Match(b) => Command::parse(&b.action),
            _ => None,
        };
        let Some(Command::Table(t)) = cmd(m.process(&b, chord("ctrl-t"), ())) else { panic!() };
        m.apply(&t);
        assert_eq!(m.active_tables().collect::<Vec<_>>(), ["nav"]);
        assert_eq!(cmd(m.process(&b, chord("j"), ())), Some(Command::Surface(SurfaceAction::ScrollLines(1))));
        let Some(Command::Table(t)) = cmd(m.process(&b, chord("escape"), ())) else { panic!() };
        m.apply(&t);
        assert!(m.is_idle());
        assert_eq!(cmd(m.process(&b, chord("j"), ())), Some(Command::Send(b"root".to_vec())));

        let Some(Command::Table(t)) = cmd(m.process(&b, chord("ctrl-o"), ())) else { panic!() };
        m.apply(&t);
        assert!(cmd(m.process(&b, chord("j"), ())).is_some());
        assert!(m.is_idle(), "a once table pops after the next key");
    }

    #[test]
    fn global_bindings_are_skipped_and_catch_all_is_last() {
        let b = binds(&["global:ctrl+g=toggle_visibility", "catch_all=ignore", "x=text:x"]);
        let mut m = Matcher::<()>::default();
        assert!(matches!(m.process(&b, chord("ctrl-g"), ()), Outcome::Unbound));
        assert_eq!(action(m.process(&b, chord("x"), ())).map(|b| b.action.name.clone()).as_deref(), Some("text"));
        assert_eq!(action(m.process(&b, chord("y"), ())).map(|b| b.action.name.clone()).as_deref(), Some("ignore"));
    }

    fn parse(s: &str) -> Option<Command> {
        Command::parse(&ConfigAction::parse(s).unwrap())
    }

    #[test]
    fn parses_commands() {
        assert_eq!(parse("text:\\x05"), Some(Command::Send(vec![5])));
        assert_eq!(parse("esc:b"), Some(Command::Send(b"\x1bb".to_vec())));
        assert_eq!(parse("csi:1;5D"), Some(Command::Send(b"\x1b[1;5D".to_vec())));
        assert_eq!(parse("new_split:down"), Some(Command::App(AppAction::NewSplit(SplitDirection::Down))));
        assert_eq!(parse("new_split"), Some(Command::App(AppAction::NewSplit(SplitDirection::Auto))));
        assert_eq!(parse("resize_split:left,10"), Some(Command::App(AppAction::ResizeSplit(Direction::Left, 10))));
        assert_eq!(parse("goto_tab:4"), Some(Command::App(AppAction::GotoTab(4))));
        assert_eq!(parse("jump_to_prompt:-1"), Some(Command::Surface(SurfaceAction::JumpToPrompt(-1))));
        assert_eq!(
            parse("write_screen_file:open,html"),
            Some(Command::Surface(SurfaceAction::WriteFile(FileSource::Screen, FileDelivery::Open, TextFormat::Html)))
        );
        assert_eq!(
            parse("write_scrollback_file:copy"),
            Some(Command::Surface(SurfaceAction::WriteFile(FileSource::Scrollback, FileDelivery::Copy, TextFormat::Plain)))
        );
        assert_eq!(
            parse("write_selection_file:paste,vt"),
            Some(Command::Surface(SurfaceAction::WriteFile(FileSource::Selection, FileDelivery::Paste, TextFormat::Vt)))
        );
        assert_eq!(
            parse("copy_to_clipboard"),
            Some(Command::Surface(SurfaceAction::CopyToClipboard(CopyFormat::Mixed)))
        );
        assert_eq!(parse("scroll_page_up"), Some(Command::Surface(SurfaceAction::ScrollPage(-1.0))));
        assert_eq!(parse("crash:main"), None);
    }

    #[test]
    fn every_default_binding_parses() {
        for mac in [true, false] {
            for b in Keybinds::defaults(mac).set.iter() {
                assert!(Command::parse(&b.action).is_some(), "{} unsupported", b.action);
            }
        }
    }

    #[test]
    fn unescapes_zig_literals() {
        assert_eq!(unescape("a\\nb"), b"a\nb");
        assert_eq!(unescape("\\x1b[A"), b"\x1b[A");
        assert_eq!(unescape("\\u{1F600}"), "\u{1F600}".as_bytes());
        assert_eq!(unescape("\\e[3~"), b"\x1b[3~");
        assert_eq!(unescape("plain\\"), b"plain\\");
        assert_eq!(unescape("\\xZ"), b"\\xZ");
    }
}
