//! Ghostty configuration: parsing, includes, themes and typed access.
//!
//! The file format, load order, `config-file` handling and theme layering follow
//! Ghostty's own loader. Keys Ghostty renamed are rewritten on load, values are
//! validated per key, and anything that fails to parse becomes a [`Diagnostic`]
//! while the previous value stays in effect.

mod color;
mod file;
mod key_names;
mod keybind;
mod known_keys;
mod paths;
mod values;
mod x11_colors;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub use color::{default_palette, parse_palette_entry, ColorParseError, Palette, PaletteEntryError, Rgb, TerminalColor};
pub use file::ConfigFile;
pub use keybind::{Action, Binding, BindingSet, Flags, KeybindError, Keybinds, Mods, Trigger, TriggerKey};
pub use paths::{Env, Platform, APP_ID};
pub use values::{
    parse_bool, parse_duration, parse_int_auto, AutoUpdate, BackgroundBlur, ClipboardAccess, ConfirmCloseSurface,
    CopyOnSelect, CursorStyle, MacTitlebarProxyIcon, MacTitlebarStyle, OptionAsAlt, ShellIntegration,
    WindowDecoration, WindowNewTabPosition, WindowPadding, WindowPaddingBalance, WindowPaddingColor,
    WindowSaveState, WindowTheme,
};

const ANGELLY_DARK: &str = include_str!("../../themes/angelly-dark");
const ANGELLY_LIGHT: &str = include_str!("../../themes/angelly-light");
const DEFAULT_THEME: &str = "Angelly Dark";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorScheme {
    Light,
    Dark,
}

/// One `key = value` line. `value` is `None` for a bare key and `Some("")` for a
/// key that resets itself to the default.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub key: String,
    pub value: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    pub path: PathBuf,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub message: String,
    pub location: Option<Location>,
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.location {
            Some(l) => write!(f, "{}:{}: {}", l.path.display(), l.line, self.message),
            None => f.write_str(&self.message),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThemeSource {
    Embedded,
    File(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedTheme {
    pub name: String,
    pub source: ThemeSource,
}

#[derive(Debug, Clone)]
pub struct Config {
    platform: Platform,
    entries: Vec<Entry>,
    diagnostics: Vec<Diagnostic>,
    theme: Option<LoadedTheme>,
    palette: Palette,
    keybinds: Keybinds,
}

/// Splits one config line into its key and value. Blank lines and `#` comments
/// yield `None`; a single pair of surrounding quotes is stripped from the value.
pub fn parse_line(line: &str) -> Option<(String, Option<String>)> {
    let line = line.trim_matches([' ', '\t', '\r']);
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let Some((key, value)) = line.split_once('=') else {
        return Some((line.to_string(), None));
    };
    let value = value.trim_matches([' ', '\t']);
    let value = match value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
        Some(inner) if value.len() >= 2 => inner,
        _ => value,
    };
    Some((key.trim_matches([' ', '\t']).to_string(), Some(value.to_string())))
}

impl Config {
    /// Loads the default config files and everything they include.
    pub fn load(env: &Env, scheme: ColorScheme) -> Self {
        Self::load_files(env, &env.default_config_paths(), scheme)
    }

    /// Loads the given files in order, then their `config-file` includes.
    pub fn load_files(env: &Env, files: &[PathBuf], scheme: ColorScheme) -> Self {
        let mut loader = Loader::new(env, true);
        for path in files {
            match std::fs::read_to_string(path) {
                Ok(text) => loader.read(&text, Some(path)),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => loader.diagnose(format!("error reading config file {}: {e}", path.display()), None),
            }
        }
        loader.load_includes();
        Self::finish(env, loader, scheme)
    }

    /// Parses config text that doesn't come from a file. Relative `config-file`
    /// paths can't be resolved and are reported.
    pub fn from_str(text: &str, env: &Env, scheme: ColorScheme) -> Self {
        let mut loader = Loader::new(env, true);
        loader.read(text, None);
        loader.load_includes();
        Self::finish(env, loader, scheme)
    }

    fn finish(env: &Env, mut loader: Loader, scheme: ColorScheme) -> Self {
        let (theme, theme_entries) = loader.resolve_theme(scheme);
        let mut entries = theme_entries;
        entries.append(&mut loader.entries);
        Self::build(env.platform, entries, loader.diagnostics, theme)
    }

    fn build(platform: Platform, entries: Vec<Entry>, mut diagnostics: Vec<Diagnostic>, theme: Option<LoadedTheme>) -> Self {
        let macos = platform == Platform::MacOs;
        let mut palette = default_palette();
        let mut keybinds = Keybinds::defaults(macos);
        for entry in &entries {
            match (entry.key.as_str(), entry.value.as_deref()) {
                ("palette", Some("")) => palette = default_palette(),
                ("palette", Some(v)) => {
                    if let Ok((index, color)) = parse_palette_entry(v) {
                        palette[usize::from(index)] = color;
                    }
                }
                ("keybind", Some(v)) => {
                    if let Err(e) = keybinds.apply(v, macos) {
                        diagnostics.push(Diagnostic { message: format!("keybind: {e}"), location: None });
                    }
                }
                _ => {}
            }
        }
        Self { platform, entries, diagnostics, theme, palette, keybinds }
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    pub fn theme(&self) -> Option<&LoadedTheme> {
        self.theme.as_ref()
    }

    pub fn platform(&self) -> Platform {
        self.platform
    }

    fn last(&self, key: &str) -> Option<&Entry> {
        self.entries.iter().rev().find(|e| e.key == key)
    }

    /// The effective value of a key; `None` when unset, reset or given without a value.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.last(key).and_then(|e| e.value.as_deref()).filter(|v| !v.is_empty())
    }

    /// Every value of a repeatable key. An empty value clears what came before it.
    pub fn get_all(&self, key: &str) -> Vec<&str> {
        let mut values = Vec::new();
        for entry in self.entries.iter().filter(|e| e.key == key) {
            match entry.value.as_deref() {
                Some("") => values.clear(),
                Some(v) => values.push(v),
                None => {}
            }
        }
        values
    }

    fn bool_or(&self, key: &str, default: bool) -> bool {
        match self.last(key).map(|e| e.value.as_deref()) {
            None | Some(Some("")) => default,
            Some(None) => true,
            Some(Some(v)) => parse_bool(v).unwrap_or(default),
        }
    }

    fn optional_bool(&self, key: &str) -> Option<bool> {
        match self.last(key).map(|e| e.value.as_deref()) {
            None | Some(Some("")) => None,
            Some(None) => Some(true),
            Some(Some(v)) => parse_bool(v),
        }
    }

    fn number<T: std::str::FromStr>(&self, key: &str, default: T) -> T {
        self.get(key).and_then(|v| v.parse().ok()).unwrap_or(default)
    }

    fn enumeration<T>(&self, key: &str, parse: fn(&str) -> Option<T>, default: T) -> T {
        self.get(key).and_then(parse).unwrap_or(default)
    }

    fn color(&self, key: &str, default: Rgb) -> Rgb {
        self.get(key).and_then(|v| Rgb::parse(v).ok()).unwrap_or(default)
    }

    fn terminal_color(&self, key: &str) -> Option<TerminalColor> {
        self.get(key).and_then(|v| TerminalColor::parse(v).ok())
    }

    fn duration(&self, key: &str, default: Duration) -> Duration {
        self.get(key).and_then(parse_duration).unwrap_or(default)
    }

    pub fn font_family(&self) -> Vec<&str> {
        self.get_all("font-family")
    }

    pub fn font_size(&self) -> f32 {
        let default = if self.platform == Platform::MacOs { 13.0 } else { 12.0 };
        self.number("font-size", default)
    }

    pub fn font_thicken(&self) -> bool {
        self.bool_or("font-thicken", false)
    }

    pub fn foreground(&self) -> Rgb {
        self.color("foreground", Rgb { r: 0xFF, g: 0xFF, b: 0xFF })
    }

    pub fn background(&self) -> Rgb {
        self.color("background", Rgb { r: 0x28, g: 0x2C, b: 0x34 })
    }

    pub fn selection_foreground(&self) -> Option<TerminalColor> {
        self.terminal_color("selection-foreground")
    }

    pub fn selection_background(&self) -> Option<TerminalColor> {
        self.terminal_color("selection-background")
    }

    pub fn cursor_color(&self) -> Option<TerminalColor> {
        self.terminal_color("cursor-color")
    }

    pub fn cursor_text(&self) -> Option<TerminalColor> {
        self.terminal_color("cursor-text")
    }

    pub fn palette(&self) -> &Palette {
        &self.palette
    }

    pub fn minimum_contrast(&self) -> f64 {
        self.number("minimum-contrast", 1.0f64).clamp(1.0, 21.0)
    }

    /// `background-opacity`, clamped to 0...1 as Ghostty does.
    pub fn background_opacity(&self) -> f64 {
        self.number("background-opacity", 1.0f64).clamp(0.0, 1.0)
    }

    pub fn background_blur(&self) -> BackgroundBlur {
        match self.last("background-blur").map(|e| e.value.as_deref()) {
            None | Some(Some("")) => BackgroundBlur::Disabled,
            Some(v) => BackgroundBlur::parse(v).unwrap_or(BackgroundBlur::Disabled),
        }
    }

    pub fn cursor_style(&self) -> CursorStyle {
        self.enumeration("cursor-style", CursorStyle::parse, CursorStyle::Block)
    }

    pub fn cursor_style_blink(&self) -> Option<bool> {
        self.optional_bool("cursor-style-blink")
    }

    pub fn cursor_opacity(&self) -> f64 {
        self.number("cursor-opacity", 1.0f64).clamp(0.0, 1.0)
    }

    pub fn cursor_click_to_move(&self) -> bool {
        self.bool_or("cursor-click-to-move", true)
    }

    pub fn mouse_hide_while_typing(&self) -> bool {
        self.bool_or("mouse-hide-while-typing", false)
    }

    pub fn scrollback_limit_bytes(&self) -> usize {
        match self.get("scrollback-limit-bytes") {
            Some("unlimited") => usize::MAX,
            Some(v) => parse_int_auto::<usize>(v).unwrap_or(50_000_000),
            None => 50_000_000,
        }
    }

    pub fn window_padding_x(&self) -> WindowPadding {
        self.padding("window-padding-x")
    }

    pub fn window_padding_y(&self) -> WindowPadding {
        self.padding("window-padding-y")
    }

    fn padding(&self, key: &str) -> WindowPadding {
        self.get(key)
            .and_then(WindowPadding::parse)
            .unwrap_or(WindowPadding { top_left: 2, bottom_right: 2 })
    }

    pub fn window_padding_balance(&self) -> WindowPaddingBalance {
        self.enumeration("window-padding-balance", WindowPaddingBalance::parse, WindowPaddingBalance::False)
    }

    pub fn window_padding_color(&self) -> WindowPaddingColor {
        self.enumeration("window-padding-color", WindowPaddingColor::parse, WindowPaddingColor::Background)
    }

    /// Initial width in cells; `0` means unset, and set values have a floor of 10.
    pub fn window_width(&self) -> u32 {
        match self.number("window-width", 0) {
            0 => 0,
            n => n.max(10),
        }
    }

    /// Initial height in cells; `0` means unset, and set values have a floor of 4.
    pub fn window_height(&self) -> u32 {
        match self.number("window-height", 0) {
            0 => 0,
            n => n.max(4),
        }
    }

    pub fn window_decoration(&self) -> WindowDecoration {
        match self.last("window-decoration").map(|e| e.value.as_deref()) {
            None | Some(Some("")) => WindowDecoration::Auto,
            Some(v) => WindowDecoration::parse_config(v).unwrap_or(WindowDecoration::Auto),
        }
    }

    pub fn window_theme(&self) -> WindowTheme {
        self.enumeration("window-theme", WindowTheme::parse, WindowTheme::Auto)
    }

    pub fn window_save_state(&self) -> WindowSaveState {
        self.enumeration("window-save-state", WindowSaveState::parse, WindowSaveState::Always)
    }

    pub fn window_new_tab_position(&self) -> WindowNewTabPosition {
        self.enumeration("window-new-tab-position", WindowNewTabPosition::parse, WindowNewTabPosition::Current)
    }

    pub fn macos_titlebar_style(&self) -> MacTitlebarStyle {
        self.enumeration("macos-titlebar-style", MacTitlebarStyle::parse, MacTitlebarStyle::Transparent)
    }

    pub fn macos_titlebar_proxy_icon(&self) -> MacTitlebarProxyIcon {
        self.enumeration("macos-titlebar-proxy-icon", MacTitlebarProxyIcon::parse, MacTitlebarProxyIcon::Visible)
    }

    pub fn macos_option_as_alt(&self) -> Option<OptionAsAlt> {
        self.get("macos-option-as-alt").and_then(OptionAsAlt::parse)
    }

    pub fn copy_on_select(&self) -> CopyOnSelect {
        let default = if self.platform == Platform::Linux { CopyOnSelect::Primary } else { CopyOnSelect::None };
        self.enumeration("copy-on-select", CopyOnSelect::parse, default)
    }

    pub fn confirm_close_surface(&self) -> ConfirmCloseSurface {
        self.enumeration("confirm-close-surface", ConfirmCloseSurface::parse, ConfirmCloseSurface::True)
    }

    pub fn clipboard_read(&self) -> ClipboardAccess {
        self.enumeration("clipboard-read", ClipboardAccess::parse, ClipboardAccess::Ask)
    }

    pub fn clipboard_write(&self) -> ClipboardAccess {
        self.enumeration("clipboard-write", ClipboardAccess::parse, ClipboardAccess::Allow)
    }

    pub fn clipboard_paste_protection(&self) -> bool {
        self.bool_or("clipboard-paste-protection", true)
    }

    pub fn clipboard_trim_trailing_spaces(&self) -> bool {
        self.bool_or("clipboard-trim-trailing-spaces", true)
    }

    pub fn shell_integration(&self) -> ShellIntegration {
        self.enumeration("shell-integration", ShellIntegration::parse, ShellIntegration::Detect)
    }

    pub fn auto_update(&self) -> Option<AutoUpdate> {
        match self.last("auto-update").map(|e| e.value.as_deref()) {
            None | Some(Some("")) => Some(AutoUpdate::Check),
            Some(v) => v.and_then(AutoUpdate::parse),
        }
    }

    pub fn auto_update_channel(&self) -> Option<&str> {
        self.get("auto-update-channel")
    }

    pub fn link_url(&self) -> bool {
        self.bool_or("link-url", true)
    }

    pub fn focus_follows_mouse(&self) -> bool {
        self.bool_or("focus-follows-mouse", false)
    }

    pub fn initial_window(&self) -> bool {
        self.bool_or("initial-window", true)
    }

    pub fn quit_after_last_window_closed(&self) -> bool {
        self.bool_or("quit-after-last-window-closed", self.platform == Platform::Linux)
    }

    pub fn unfocused_split_opacity(&self) -> f64 {
        self.number("unfocused-split-opacity", 0.7f64).clamp(0.15, 1.0)
    }

    pub fn unfocused_split_fill(&self) -> Option<Rgb> {
        self.get("unfocused-split-fill").and_then(|v| Rgb::parse(v).ok())
    }

    pub fn split_divider_color(&self) -> Option<Rgb> {
        self.get("split-divider-color").and_then(|v| Rgb::parse(v).ok())
    }

    pub fn undo_timeout(&self) -> Duration {
        self.duration("undo-timeout", Duration::from_secs(5))
    }

    pub fn title(&self) -> Option<&str> {
        self.get("title")
    }

    pub fn command(&self) -> Option<&str> {
        self.get("command")
    }

    pub fn working_directory(&self) -> Option<&str> {
        self.get("working-directory")
    }

    pub fn term(&self) -> &str {
        self.get("term").unwrap_or("xterm-ghostty")
    }

    /// `env = KEY=VALUE` entries, with later assignments replacing earlier ones and
    /// `env = KEY` removing a variable. Order of first appearance is preserved.
    pub fn env(&self) -> Vec<(String, String)> {
        let mut result: Vec<(String, String)> = Vec::new();
        for value in self.get_all("env") {
            match value.split_once('=') {
                Some((k, v)) => {
                    let k = k.trim();
                    match result.iter_mut().find(|(rk, _)| rk == k) {
                        Some(slot) => slot.1 = v.to_string(),
                        None => result.push((k.to_string(), v.to_string())),
                    }
                }
                None => result.retain(|(k, _)| k != value.trim()),
            }
        }
        result
    }

    pub fn keybinds(&self) -> &Keybinds {
        &self.keybinds
    }
}

struct Include {
    path: PathBuf,
    optional: bool,
}

struct Loader<'a> {
    env: &'a Env,
    allow_includes: bool,
    entries: Vec<Entry>,
    diagnostics: Vec<Diagnostic>,
    includes: Vec<Include>,
    next_include: usize,
    loaded: HashSet<PathBuf>,
}

impl<'a> Loader<'a> {
    fn new(env: &'a Env, allow_includes: bool) -> Self {
        Self {
            env,
            allow_includes,
            entries: Vec::new(),
            diagnostics: Vec::new(),
            includes: Vec::new(),
            next_include: 0,
            loaded: HashSet::new(),
        }
    }

    fn diagnose(&mut self, message: String, location: Option<Location>) {
        self.diagnostics.push(Diagnostic { message, location });
    }

    fn read(&mut self, text: &str, path: Option<&Path>) {
        for (index, line) in text.lines().enumerate() {
            let Some((key, value)) = parse_line(line) else { continue };
            let location = path.map(|p| Location { path: p.to_path_buf(), line: index + 1 });
            self.add(key, value, location, path);
        }
    }

    fn add(&mut self, key: String, value: Option<String>, location: Option<Location>, source: Option<&Path>) {
        for (key, value) in compat(&key, value.as_deref(), self.env.platform) {
            if !known_keys::KNOWN_KEYS.contains(&key.as_str()) {
                self.diagnose(format!("{key}: unknown field"), location.clone());
                continue;
            }
            if let Err(message) = validate(&key, value.as_deref()) {
                self.diagnose(format!("{key}: {message}"), location.clone());
                continue;
            }
            if key == "config-file" {
                if !self.allow_includes {
                    continue;
                }
                self.add_include(value.as_deref(), &location, source);
            }
            self.entries.push(Entry { key, value });
        }
    }

    fn add_include(&mut self, value: Option<&str>, location: &Option<Location>, source: Option<&Path>) {
        let Some(value) = value else { return };
        if value.is_empty() {
            self.includes.truncate(self.next_include);
            return;
        }
        let (optional, path) = match value.strip_prefix('?') {
            Some(rest) => (true, rest),
            None => (false, value),
        };
        let path = match path.strip_prefix('"').and_then(|p| p.strip_suffix('"')) {
            Some(inner) if path.len() >= 2 => inner,
            _ => path,
        };
        if path.is_empty() {
            return;
        }
        let expanded = self.env.expand_home(path);
        let resolved = if expanded.is_absolute() {
            expanded
        } else if let Some(base) = source.and_then(Path::parent) {
            base.join(expanded)
        } else {
            self.diagnose(format!("config-file {path}: relative path without a base directory"), location.clone());
            return;
        };
        self.includes.push(Include { path: resolved, optional });
    }

    /// Loads queued `config-file` includes in order. Files they include are queued
    /// behind the rest, and a file is read at most once.
    fn load_includes(&mut self) {
        while self.next_include < self.includes.len() {
            let Include { path, optional } = {
                let include = &self.includes[self.next_include];
                Include { path: include.path.clone(), optional: include.optional }
            };
            self.next_include += 1;

            let identity = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
            if !self.loaded.insert(identity) {
                self.diagnose(format!("config-file {}: cycle detected", path.display()), None);
                continue;
            }
            let metadata = match std::fs::metadata(&path) {
                Ok(m) => m,
                Err(e) => {
                    if e.kind() != std::io::ErrorKind::NotFound || !optional {
                        self.diagnose(format!("error opening config-file {}: {e}", path.display()), None);
                    }
                    continue;
                }
            };
            if !metadata.is_file() {
                self.diagnose(format!("config-file {}: not reading because it is not a regular file", path.display()), None);
                continue;
            }
            match std::fs::read_to_string(&path) {
                Ok(text) => self.read(&text, Some(&path)),
                Err(e) => self.diagnose(format!("error opening config-file {}: {e}", path.display()), None),
            }
        }
    }

    /// Finds the theme the config selects for `scheme` and returns its entries, which
    /// sit underneath everything the user wrote.
    fn resolve_theme(&mut self, scheme: ColorScheme) -> (Option<LoadedTheme>, Vec<Entry>) {
        let selection = match self.entries.iter().rev().find(|e| e.key == "theme") {
            None => Some(DEFAULT_THEME.to_string()),
            Some(Entry { value: Some(v), .. }) if v.is_empty() => None,
            Some(Entry { value: Some(v), .. }) => parse_theme(v).map(|(light, dark)| match scheme {
                ColorScheme::Light => light,
                ColorScheme::Dark => dark,
            }),
            Some(_) => None,
        };
        let Some(name) = selection else { return (None, Vec::new()) };
        let name = self.env.expand_home(&name).to_string_lossy().into_owned();

        let Some((source, text)) = self.open_theme(&name) else { return (None, Vec::new()) };
        let mut sub = Loader::new(self.env, false);
        let origin = match &source {
            ThemeSource::File(p) => Some(p.clone()),
            ThemeSource::Embedded => None,
        };
        sub.read(&text, origin.as_deref());
        self.diagnostics.append(&mut sub.diagnostics);
        let entries = sub.entries.into_iter().filter(|e| e.key != "theme").collect();
        (Some(LoadedTheme { name, source }), entries)
    }

    fn open_theme(&mut self, name: &str) -> Option<(ThemeSource, String)> {
        let read = |this: &mut Self, path: &Path| -> Option<(ThemeSource, String)> {
            match std::fs::read_to_string(path) {
                Ok(text) => Some((ThemeSource::File(path.to_path_buf()), text)),
                Err(e) => {
                    this.diagnose(format!("failed to load theme \"{name}\" from the file \"{}\": {e}", path.display()), None);
                    None
                }
            }
        };

        if Path::new(name).is_absolute() {
            return match std::fs::metadata(name) {
                Ok(m) if m.is_file() => read(self, Path::new(name)),
                Ok(_) => {
                    self.diagnose(format!("not reading theme from \"{name}\": it is not a regular file"), None);
                    None
                }
                Err(_) => {
                    self.diagnose(format!("failed to load theme from the path \"{name}\""), None);
                    None
                }
            };
        }
        if Path::new(name).file_name().map(|f| f != name).unwrap_or(true) {
            self.diagnose(format!("theme \"{name}\" cannot include path separators unless it is an absolute path"), None);
            return None;
        }

        let dirs: Vec<PathBuf> = [self.env.user_themes_dir(), self.env.resource_themes_dir()].into_iter().flatten().collect();
        for dir in &dirs {
            let path = dir.join(name);
            match std::fs::metadata(&path) {
                Ok(m) if m.is_file() => return read(self, &path),
                Ok(_) => {
                    self.diagnose(format!("not reading theme from \"{name}\": it is not a regular file"), None);
                    return None;
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => {
                    self.diagnose(format!("failed to load theme \"{name}\" from the file \"{}\": {e}", path.display()), None);
                    return None;
                }
            }
        }
        if let Some(text) = embedded_theme(name) {
            return Some((ThemeSource::Embedded, text.to_string()));
        }
        for dir in &dirs {
            self.diagnose(format!("theme \"{name}\" not found, tried path \"{}\"", dir.join(name).display()), None);
        }
        None
    }
}

/// The themes shipped with Angelly, used when no theme file of that name exists.
fn embedded_theme(name: &str) -> Option<&'static str> {
    match name {
        "Angelly Dark" => Some(ANGELLY_DARK),
        "Angelly Light" => Some(ANGELLY_LIGHT),
        _ => None,
    }
}

/// A `theme` value is either one name or `light:NAME,dark:NAME`.
fn parse_theme(input: &str) -> Option<(String, String)> {
    let paired = input.contains([',', '=', ':']);
    if !paired {
        let name = input.trim_matches([' ', '\t']).to_string();
        return Some((name.clone(), name));
    }
    let (mut light, mut dark) = (None, None);
    for part in input.split(',') {
        let (key, value) = part.split_once([':', '='])?;
        let value = value.trim_matches([' ', '\t']);
        let value = match value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
            Some(inner) if value.len() >= 2 => inner,
            _ => value,
        };
        match key.trim_matches([' ', '\t']) {
            "light" => light = Some(value.to_string()),
            "dark" => dark = Some(value.to_string()),
            _ => return None,
        }
    }
    Some((light?, dark?))
}

/// Rewrites keys and values Ghostty renamed or reshaped over time.
fn compat(key: &str, value: Option<&str>, platform: Platform) -> Vec<(String, Option<String>)> {
    let same = || vec![(key.to_string(), value.map(str::to_string))];
    let pair = |k: &str, v: &str| (k.to_string(), Some(v.to_string()));
    let flag = || parse_bool(value.unwrap_or("t"));
    match key {
        "background-blur-radius" => vec![("background-blur".to_string(), value.map(str::to_string))],
        "adw-toolbar-style" => vec![("gtk-toolbar-style".to_string(), value.map(str::to_string))],
        "scrollback-limit" => vec![("scrollback-limit-bytes".to_string(), value.map(str::to_string))],
        "copy-on-select" => match value {
            Some("true") => {
                vec![pair("copy-on-select", if platform == Platform::Linux { "primary" } else { "clipboard" })]
            }
            Some("false") => vec![pair("copy-on-select", "none")],
            _ => same(),
        },
        "macos-dock-drop-behavior" if value == Some("window") => vec![pair(key, "new-window")],
        "gtk-single-instance" if value == Some("desktop") => vec![pair(key, "detect")],
        "gtk-tabs-location" if value == Some("hidden") => vec![pair("window-show-tab-bar", "never")],
        "bold-is-bright" => match flag() {
            Some(true) => vec![pair("bold-color", "bright")],
            Some(false) => Vec::new(),
            None => same(),
        },
        "cursor-invert-fg-bg" => match flag() {
            Some(true) => vec![pair("cursor-color", "cell-foreground"), pair("cursor-text", "cell-background")],
            Some(false) => Vec::new(),
            None => same(),
        },
        "selection-invert-fg-bg" => match flag() {
            Some(true) => vec![pair("selection-foreground", "cell-background"), pair("selection-background", "cell-foreground")],
            Some(false) => Vec::new(),
            None => same(),
        },
        _ => same(),
    }
}

/// Checks a value against the type of its key. Keys without an entry here are stored as written.
fn validate(key: &str, value: Option<&str>) -> Result<(), String> {
    let required = || value.ok_or_else(|| "value required".to_string());
    let invalid = || "invalid value".to_string();
    let enumerated = |is_valid: fn(&str) -> bool| match value {
        Some("") => Ok(()),
        Some(v) if is_valid(v) => Ok(()),
        Some(_) => Err(invalid()),
        None => Err("value required".to_string()),
    };
    let boolean = || match value {
        None | Some("") => Ok(()),
        Some(v) => parse_bool(v).map(|_| ()).ok_or_else(invalid),
    };
    let parsed = |ok: bool| match value {
        Some("") => Ok(()),
        Some(_) if ok => Ok(()),
        Some(_) => Err(invalid()),
        None => Err("value required".to_string()),
    };
    let text = || value.unwrap_or("");

    match key {
        "font-thicken" | "clipboard-paste-protection" | "clipboard-trim-trailing-spaces" | "mouse-hide-while-typing"
        | "cursor-click-to-move" | "focus-follows-mouse" | "initial-window" | "quit-after-last-window-closed"
        | "link-url" | "cursor-style-blink" => boolean(),
        "font-size" => parsed(text().parse::<f32>().is_ok()),
        "background-opacity" | "cursor-opacity" | "minimum-contrast" | "unfocused-split-opacity" => {
            parsed(text().parse::<f64>().is_ok())
        }
        "window-width" | "window-height" => parsed(text().parse::<u32>().is_ok()),
        "foreground" | "background" | "unfocused-split-fill" | "split-divider-color" => {
            parsed(Rgb::parse(text()).is_ok())
        }
        "selection-foreground" | "selection-background" | "cursor-color" | "cursor-text" => {
            parsed(TerminalColor::parse(text()).is_ok())
        }
        "palette" => match value {
            Some("") => Ok(()),
            Some(v) => parse_palette_entry(v).map(|_| ()).map_err(|e| match e {
                PaletteEntryError::Overflow => "palette index out of range".to_string(),
                PaletteEntryError::InvalidFormat => invalid(),
            }),
            None => Err("value required".to_string()),
        },
        "undo-timeout" => parsed(parse_duration(text()).is_some()),
        "scrollback-limit-bytes" => parsed(text() == "unlimited" || parse_int_auto::<usize>(text()).is_some()),
        "window-padding-x" | "window-padding-y" => parsed(WindowPadding::parse(text()).is_some()),
        "background-blur" => match value {
            Some("") => Ok(()),
            v => BackgroundBlur::parse(v).map(|_| ()).ok_or_else(invalid),
        },
        "window-decoration" => match value {
            Some("") => Ok(()),
            v => WindowDecoration::parse_config(v).map(|_| ()).ok_or_else(invalid),
        },
        "cursor-style" => enumerated(CursorStyle::is_valid),
        "macos-titlebar-style" => enumerated(MacTitlebarStyle::is_valid),
        "macos-titlebar-proxy-icon" => enumerated(MacTitlebarProxyIcon::is_valid),
        "macos-option-as-alt" => enumerated(OptionAsAlt::is_valid),
        "window-theme" => enumerated(WindowTheme::is_valid),
        "window-save-state" => enumerated(WindowSaveState::is_valid),
        "window-new-tab-position" => enumerated(WindowNewTabPosition::is_valid),
        "window-padding-balance" => enumerated(WindowPaddingBalance::is_valid),
        "window-padding-color" => enumerated(WindowPaddingColor::is_valid),
        "copy-on-select" => enumerated(CopyOnSelect::is_valid),
        "confirm-close-surface" => enumerated(ConfirmCloseSurface::is_valid),
        "clipboard-read" | "clipboard-write" => enumerated(ClipboardAccess::is_valid),
        "shell-integration" => enumerated(ShellIntegration::is_valid),
        "auto-update" => enumerated(AutoUpdate::is_valid),
        "theme" => match value {
            Some("") => Ok(()),
            Some(v) if parse_theme(v).is_some() => Ok(()),
            Some(_) => Err(invalid()),
            None => Err("value required".to_string()),
        },
        "font-family" | "title" | "command" | "working-directory" | "keybind" | "env" | "config-file" => {
            required().map(|_| ())
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests;
