use std::collections::BTreeMap;
use std::fmt;

use super::key_names::KEY_NAMES;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeybindError {
    InvalidFormat,
    InvalidAction,
}

impl fmt::Display for KeybindError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidFormat => f.write_str("invalid format"),
            Self::InvalidAction => f.write_str("invalid action"),
        }
    }
}

impl std::error::Error for KeybindError {}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Mods {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub super_key: bool,
}

impl Mods {
    pub const NONE: Mods = Mods { shift: false, ctrl: false, alt: false, super_key: false };

    pub fn shift(mut self) -> Self {
        self.shift = true;
        self
    }

    pub fn ctrl(mut self) -> Self {
        self.ctrl = true;
        self
    }

    pub fn alt(mut self) -> Self {
        self.alt = true;
        self
    }

    pub fn super_key(mut self) -> Self {
        self.super_key = true;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TriggerKey {
    /// A physical key identified by its W3C-derived snake case name, such as `arrow_left`.
    Physical(String),
    Unicode(char),
    CatchAll,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Trigger {
    pub key: TriggerKey,
    pub mods: Mods,
}

const MOD_ALIASES: &[(&str, &str)] = &[
    ("cmd", "super"),
    ("command", "super"),
    ("opt", "alt"),
    ("option", "alt"),
    ("control", "ctrl"),
];

const COMPAT_UNICODE: &[(&str, char)] = &[
    ("zero", '0'),
    ("one", '1'),
    ("two", '2'),
    ("three", '3'),
    ("four", '4'),
    ("five", '5'),
    ("six", '6'),
    ("seven", '7'),
    ("eight", '8'),
    ("nine", '9'),
    ("plus", '+'),
    ("apostrophe", '\''),
];

const COMPAT_PHYSICAL: &[(&str, &str)] = &[
    ("grave_accent", "backquote"),
    ("left_bracket", "bracket_left"),
    ("right_bracket", "bracket_right"),
    ("up", "arrow_up"),
    ("down", "arrow_down"),
    ("left", "arrow_left"),
    ("right", "arrow_right"),
    ("kp_0", "numpad_0"),
    ("kp_1", "numpad_1"),
    ("kp_2", "numpad_2"),
    ("kp_3", "numpad_3"),
    ("kp_4", "numpad_4"),
    ("kp_5", "numpad_5"),
    ("kp_6", "numpad_6"),
    ("kp_7", "numpad_7"),
    ("kp_8", "numpad_8"),
    ("kp_9", "numpad_9"),
    ("kp_add", "numpad_add"),
    ("kp_subtract", "numpad_subtract"),
    ("kp_multiply", "numpad_multiply"),
    ("kp_divide", "numpad_divide"),
    ("kp_decimal", "numpad_decimal"),
    ("kp_enter", "numpad_enter"),
    ("kp_equal", "numpad_equal"),
    ("kp_separator", "numpad_separator"),
    ("kp_left", "numpad_left"),
    ("kp_right", "numpad_right"),
    ("kp_up", "numpad_up"),
    ("kp_down", "numpad_down"),
    ("kp_page_up", "numpad_page_up"),
    ("kp_page_down", "numpad_page_down"),
    ("kp_home", "numpad_home"),
    ("kp_end", "numpad_end"),
    ("kp_insert", "numpad_insert"),
    ("kp_delete", "numpad_delete"),
    ("kp_begin", "numpad_begin"),
    ("left_shift", "shift_left"),
    ("right_shift", "shift_right"),
    ("left_control", "control_left"),
    ("right_control", "control_right"),
    ("left_alt", "alt_left"),
    ("right_alt", "alt_right"),
    ("left_super", "meta_left"),
    ("right_super", "meta_right"),
];

fn physical(name: &str) -> TriggerKey {
    TriggerKey::Physical(name.to_string())
}

fn key_from_w3c(code: &str) -> Option<&'static str> {
    if code.len() > 128 {
        return None;
    }
    let lower = code.to_ascii_lowercase();
    if let Some(k) = KEY_NAMES.iter().find(|k| **k == lower) {
        return Some(k);
    }
    let mut snake = String::new();
    for (i, ch) in code.chars().enumerate() {
        match ch {
            'a'..='z' => snake.push(ch),
            'A'..='Z' | '0'..='9' => {
                if i > 0 {
                    snake.push('_');
                }
                snake.push(ch.to_ascii_lowercase());
            }
            _ => return None,
        }
    }
    KEY_NAMES.iter().find(|k| **k == snake).copied()
}

fn compat_key(part: &str) -> Option<TriggerKey> {
    let (physical_prefix, name) = match part.strip_prefix("physical:") {
        Some(rest) => (true, rest),
        None => (false, part),
    };
    if let Some((_, ch)) = COMPAT_UNICODE.iter().find(|(n, _)| *n == name) {
        return Some(if physical_prefix {
            match ch {
                '0'..='9' => physical(&format!("digit_{ch}")),
                '\'' => physical("quote"),
                _ => return None,
            }
        } else {
            TriggerKey::Unicode(*ch)
        });
    }
    COMPAT_PHYSICAL.iter().find(|(n, _)| *n == name).map(|(_, k)| physical(k))
}

impl Trigger {
    pub fn parse(input: &str) -> Result<Self, KeybindError> {
        if input.is_empty() {
            return Err(KeybindError::InvalidFormat);
        }
        let mut mods = Mods::NONE;
        let mut key: Option<TriggerKey> = None;
        let mut rest = input;
        while !rest.is_empty() {
            let idx = rest.find('+').unwrap_or(rest.len());
            let part = &rest[..idx];
            rest = if idx >= rest.len() { "" } else { &rest[idx + 1..] };

            let mod_name = MOD_ALIASES
                .iter()
                .find(|(alias, _)| *alias == part)
                .map(|(_, m)| *m)
                .unwrap_or(part);
            let slot = match mod_name {
                "shift" => Some(&mut mods.shift),
                "ctrl" => Some(&mut mods.ctrl),
                "alt" => Some(&mut mods.alt),
                "super" => Some(&mut mods.super_key),
                _ => None,
            };
            if let Some(slot) = slot {
                if *slot {
                    return Err(KeybindError::InvalidFormat);
                }
                *slot = true;
                continue;
            }

            if key.is_some() {
                return Err(KeybindError::InvalidFormat);
            }
            if part.is_empty() {
                key = Some(TriggerKey::Unicode('+'));
                continue;
            }
            if let Some(k) = KEY_NAMES.iter().find(|k| **k == part) {
                key = Some(physical(k));
                continue;
            }
            let mut chars = part.chars();
            if let (Some(c), None) = (chars.next(), chars.next()) {
                key = Some(TriggerKey::Unicode(c));
                continue;
            }
            if let Some(k) = key_from_w3c(part) {
                key = Some(physical(k));
                continue;
            }
            if part == "catch_all" {
                key = Some(TriggerKey::CatchAll);
                continue;
            }
            if let Some(k) = compat_key(part) {
                key = Some(k);
                continue;
            }
            return Err(KeybindError::InvalidFormat);
        }
        Ok(Trigger { key: key.unwrap_or_else(|| physical("unidentified")), mods })
    }
}

impl fmt::Display for Trigger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.mods.super_key {
            f.write_str("super+")?;
        }
        if self.mods.ctrl {
            f.write_str("ctrl+")?;
        }
        if self.mods.alt {
            f.write_str("alt+")?;
        }
        if self.mods.shift {
            f.write_str("shift+")?;
        }
        match &self.key {
            TriggerKey::Physical(name) => f.write_str(name),
            TriggerKey::Unicode(c) => write!(f, "{c}"),
            TriggerKey::CatchAll => f.write_str("catch_all"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Param {
    None,
    Text,
    Float,
    Unsigned,
    Signed,
    I16,
    Enum(&'static [&'static str], Option<&'static str>),
    WriteScreen,
    SplitResize,
    Unsupported,
}

const SPLIT_DIRECTIONS: &[&str] = &["right", "down", "left", "up", "auto"];
const SPLIT_FOCUS: &[&str] = &["previous", "next", "up", "left", "down", "right", "top", "bottom"];

const ACTIONS: &[(&str, Param)] = &[
    ("ignore", Param::None),
    ("unbind", Param::None),
    ("csi", Param::Text),
    ("esc", Param::Text),
    ("text", Param::Text),
    ("cursor_key", Param::Unsupported),
    ("reset", Param::None),
    (
        "copy_to_clipboard",
        Param::Enum(&["plain", "vt", "html", "mixed"], Some("mixed")),
    ),
    ("paste_from_clipboard", Param::None),
    ("paste_from_selection", Param::None),
    ("copy_url_to_clipboard", Param::None),
    ("copy_title_to_clipboard", Param::None),
    ("increase_font_size", Param::Float),
    ("decrease_font_size", Param::Float),
    ("reset_font_size", Param::None),
    ("set_font_size", Param::Float),
    ("search", Param::Text),
    ("search_selection", Param::None),
    ("navigate_search", Param::Enum(&["previous", "next"], None)),
    ("start_search", Param::None),
    ("end_search", Param::None),
    ("clear_screen", Param::None),
    ("select_all", Param::None),
    ("scroll_to_top", Param::None),
    ("scroll_to_bottom", Param::None),
    ("scroll_to_selection", Param::None),
    ("scroll_to_row", Param::Unsigned),
    ("scroll_page_up", Param::None),
    ("scroll_page_down", Param::None),
    ("scroll_page_fractional", Param::Float),
    ("scroll_page_lines", Param::I16),
    (
        "adjust_selection",
        Param::Enum(
            &[
                "left", "right", "up", "down", "page_up", "page_down", "home", "end",
                "beginning_of_line", "end_of_line", "left_word", "right_word",
            ],
            None,
        ),
    ),
    ("jump_to_prompt", Param::I16),
    ("write_scrollback_file", Param::WriteScreen),
    ("write_screen_file", Param::WriteScreen),
    ("write_selection_file", Param::WriteScreen),
    ("new_window", Param::None),
    ("new_tab", Param::None),
    ("previous_tab", Param::None),
    ("next_tab", Param::None),
    ("last_tab", Param::None),
    ("goto_tab", Param::Unsigned),
    ("move_tab", Param::Signed),
    ("move_tab_to_new_window", Param::None),
    ("toggle_tab_overview", Param::None),
    ("prompt_surface_title", Param::None),
    ("prompt_tab_title", Param::None),
    ("prompt_window_title", Param::None),
    ("set_surface_title", Param::Text),
    ("set_tab_title", Param::Text),
    ("set_window_title", Param::Text),
    ("new_split", Param::Enum(SPLIT_DIRECTIONS, Some("auto"))),
    ("goto_split", Param::Enum(SPLIT_FOCUS, None)),
    ("goto_window", Param::Enum(&["previous", "next"], None)),
    ("toggle_split_zoom", Param::None),
    ("toggle_readonly", Param::None),
    ("resize_split", Param::SplitResize),
    ("equalize_splits", Param::None),
    ("reset_window_size", Param::None),
    ("inspector", Param::Enum(&["toggle", "show", "hide"], None)),
    ("show_gtk_inspector", Param::None),
    ("show_on_screen_keyboard", Param::None),
    ("open_config", Param::Enum(&["os_open", "new_window"], Some("os_open"))),
    ("reload_config", Param::None),
    ("close_surface", Param::None),
    ("close_tab", Param::Enum(&["this", "other", "right"], Some("this"))),
    ("close_window", Param::None),
    ("close_all_windows", Param::None),
    ("toggle_maximize", Param::None),
    ("toggle_fullscreen", Param::None),
    ("toggle_window_decorations", Param::None),
    ("toggle_window_float_on_top", Param::None),
    ("toggle_secure_input", Param::None),
    ("toggle_mouse_reporting", Param::None),
    ("toggle_command_palette", Param::None),
    ("toggle_quick_terminal", Param::None),
    ("toggle_visibility", Param::None),
    ("toggle_background_opacity", Param::None),
    ("check_for_updates", Param::None),
    ("undo", Param::None),
    ("redo", Param::None),
    ("end_key_sequence", Param::None),
    ("activate_key_table", Param::Text),
    ("activate_key_table_once", Param::Text),
    ("deactivate_key_table", Param::None),
    ("deactivate_all_key_tables", Param::None),
    ("quit", Param::None),
    ("crash", Param::Enum(&["main", "io", "render"], None)),
];

/// A keybind action such as `new_split:right`. The parameter is validated against
/// the action's parameter type but kept in its textual form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Action {
    pub name: String,
    pub param: Option<String>,
}

impl Action {
    pub fn new(name: &str, param: Option<&str>) -> Self {
        Self { name: name.to_string(), param: param.map(str::to_string) }
    }

    pub fn parse(input: &str) -> Result<Self, KeybindError> {
        let (name, param) = match input.split_once(':') {
            Some((n, p)) => (n, Some(p)),
            None => (input, None),
        };
        if name.is_empty() {
            return Err(KeybindError::InvalidFormat);
        }
        let kind = ACTIONS
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, k)| *k)
            .ok_or(KeybindError::InvalidAction)?;

        let valid = match (kind, param) {
            (Param::Unsupported, _) => return Err(KeybindError::InvalidAction),
            (Param::None, None) => true,
            (Param::None, Some(_)) => false,
            (Param::Text, Some(_)) => true,
            (Param::Enum(_, Some(_)), None) => true,
            (_, None) => false,
            (Param::Float, Some(p)) => p.parse::<f32>().is_ok(),
            (Param::Unsigned, Some(p)) => p.parse::<usize>().is_ok(),
            (Param::Signed, Some(p)) => p.parse::<isize>().is_ok(),
            (Param::I16, Some(p)) => p.parse::<i16>().is_ok(),
            (Param::Enum(values, _), Some(p)) => values.contains(&p),
            (Param::WriteScreen, Some(p)) => {
                let (action, format) = p.split_once(',').unwrap_or((p, "plain"));
                ["copy", "paste", "open"].contains(&action) && ["plain", "vt", "html"].contains(&format)
            }
            (Param::SplitResize, Some(p)) => match p.split_once(',') {
                Some((dir, amount)) => {
                    ["up", "down", "left", "right"].contains(&dir) && amount.parse::<u16>().is_ok()
                }
                None => false,
            },
        };
        if !valid {
            return Err(KeybindError::InvalidFormat);
        }
        Ok(Self { name: name.to_string(), param: param.map(str::to_string) })
    }

    fn matches(&self, other: &Action) -> bool {
        if self.name != other.name {
            return false;
        }
        let default = ACTIONS.iter().find(|(n, _)| *n == self.name).and_then(|(_, k)| match k {
            Param::Enum(_, d) => *d,
            _ => None,
        });
        let a = self.param.as_deref().or(default);
        let b = other.param.as_deref().or(default);
        a == b
    }
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.param {
            Some(p) => write!(f, "{}:{}", self.name, p),
            None => f.write_str(&self.name),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Flags {
    pub consumed: bool,
    pub all: bool,
    pub global: bool,
    pub performable: bool,
}

impl Default for Flags {
    fn default() -> Self {
        Self { consumed: true, all: false, global: false, performable: false }
    }
}

/// A binding: a trigger sequence (more than one trigger for `a>b` sequences),
/// its action, optional chained actions and flags.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    pub triggers: Vec<Trigger>,
    pub action: Action,
    pub chained: Vec<Action>,
    pub flags: Flags,
}

impl Binding {
    /// The final trigger of the sequence.
    pub fn trigger(&self) -> &Trigger {
        self.triggers.last().expect("binding has at least one trigger")
    }

    pub fn is_sequence(&self) -> bool {
        self.triggers.len() > 1
    }
}

enum Parsed {
    Binding(Binding),
    Chain(Action),
}

fn parse_flags(raw: &str) -> Result<(Flags, usize), KeybindError> {
    let mut flags = Flags::default();
    let mut start = 0;
    let mut input = raw;
    while let Some(idx) = input.find(':') {
        match &input[..idx] {
            "all" => {
                if flags.all {
                    return Err(KeybindError::InvalidFormat);
                }
                flags.all = true;
            }
            "global" => {
                if flags.global {
                    return Err(KeybindError::InvalidFormat);
                }
                flags.global = true;
            }
            "unconsumed" => {
                if !flags.consumed {
                    return Err(KeybindError::InvalidFormat);
                }
                flags.consumed = false;
            }
            "performable" => {
                if flags.performable {
                    return Err(KeybindError::InvalidFormat);
                }
                flags.performable = true;
            }
            _ => break,
        }
        start += idx + 1;
        input = &input[idx + 1..];
    }
    Ok((flags, start))
}

fn parse_single(raw: &str) -> Result<Parsed, KeybindError> {
    let (flags, start) = parse_flags(raw)?;
    let input = &raw[start..];

    let bytes = input.as_bytes();
    let mut offset = 0;
    let eq_idx = loop {
        let Some(rel) = input[offset..].find('=') else {
            return Err(KeybindError::InvalidFormat);
        };
        let idx = offset + rel;
        if idx + 1 < bytes.len() && (bytes[idx + 1] == b'+' || bytes[idx + 1] == b'=') {
            offset = idx + 1;
            continue;
        }
        break idx;
    };

    let chain = &input[..eq_idx] == "chain";
    if chain && start > 0 {
        return Err(KeybindError::InvalidFormat);
    }
    let action = Action::parse(&input[eq_idx + 1..])?;
    if chain {
        return Ok(Parsed::Chain(action));
    }

    let mut triggers = Vec::new();
    for part in input[..eq_idx].split('>') {
        triggers.push(Trigger::parse(part)?);
    }
    if triggers.len() > 1 && (flags.global || flags.all) {
        return Err(KeybindError::InvalidFormat);
    }
    Ok(Parsed::Binding(Binding { triggers, action, chained: Vec::new(), flags }))
}

/// An ordered set of bindings. Adding a binding with an existing trigger sequence
/// replaces it, and the replacement counts as the most recently added.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BindingSet {
    bindings: Vec<Binding>,
}

impl BindingSet {
    pub fn iter(&self) -> impl Iterator<Item = &Binding> {
        self.bindings.iter()
    }

    pub fn len(&self) -> usize {
        self.bindings.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }

    pub fn get(&self, sequence: &[Trigger]) -> Option<&Binding> {
        self.bindings.iter().find(|b| b.triggers == sequence)
    }

    fn put(&mut self, binding: Binding) {
        self.bindings.retain(|b| b.triggers != binding.triggers);
        if binding.action.name == "unbind" {
            return;
        }
        self.bindings.push(binding);
    }

    fn parse_and_put(&mut self, input: &str) -> Result<(), KeybindError> {
        match parse_single(input)? {
            Parsed::Binding(b) => {
                self.put(b);
                Ok(())
            }
            Parsed::Chain(action) => match self.bindings.last_mut() {
                Some(last) => {
                    last.chained.push(action);
                    Ok(())
                }
                None => Err(KeybindError::InvalidFormat),
            },
        }
    }

    /// The most recently added binding for `action`.
    pub fn trigger_for(&self, action: &Action) -> Option<&Binding> {
        self.bindings.iter().rev().find(|b| b.action.matches(action))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ChainTarget {
    Root,
    Table(String),
}

/// The effective keybinds of a config: the root set plus named key tables.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keybinds {
    pub set: BindingSet,
    pub tables: BTreeMap<String, BindingSet>,
    chain_target: ChainTarget,
}

impl Default for Keybinds {
    fn default() -> Self {
        Self { set: BindingSet::default(), tables: BTreeMap::new(), chain_target: ChainTarget::Root }
    }
}

impl Keybinds {
    /// Keybinds for the given platform with only the built-in defaults.
    pub fn defaults(macos: bool) -> Self {
        let mut keybinds = Self::default();
        keybinds.reset_to_defaults(macos);
        keybinds
    }

    pub fn reset_to_defaults(&mut self, macos: bool) {
        *self = Self::default();
        for line in default_keybinds(macos) {
            self.set
                .parse_and_put(&line)
                .unwrap_or_else(|e| panic!("invalid default keybind {line:?}: {e}"));
        }
    }

    /// Applies one `keybind = ...` value following Ghostty's rules: empty restores the
    /// defaults, `clear` removes everything, `table/binding` targets a key table and
    /// `chain=action` extends the previous binding.
    pub fn apply(&mut self, value: &str, macos: bool) -> Result<(), KeybindError> {
        if value.is_empty() {
            self.reset_to_defaults(macos);
            return Ok(());
        }
        if value == "clear" {
            *self = Self::default();
            return Ok(());
        }

        let eq_idx = value.find('=').unwrap_or(value.len());
        if let Some(slash) = value[..eq_idx].find('/') {
            let table_name = &value[..slash];
            if !table_name.is_empty() && !table_name.contains(['+', '>']) {
                let binding = &value[slash + 1..];
                let table = self.tables.entry(table_name.to_string()).or_default();
                if binding.is_empty() {
                    *table = BindingSet::default();
                    self.chain_target = ChainTarget::Root;
                    return Ok(());
                }
                if binding.starts_with("chain=") {
                    return Err(KeybindError::InvalidFormat);
                }
                table.parse_and_put(binding)?;
                self.chain_target = ChainTarget::Table(table_name.to_string());
                return Ok(());
            }
        }

        if value.starts_with("chain=") {
            return match self.chain_target.clone() {
                ChainTarget::Root => self.set.parse_and_put(value),
                ChainTarget::Table(name) => match self.tables.get_mut(&name) {
                    Some(table) => table.parse_and_put(value),
                    None => {
                        self.chain_target = ChainTarget::Root;
                        Err(KeybindError::InvalidFormat)
                    }
                },
            };
        }

        self.set.parse_and_put(value)?;
        self.chain_target = ChainTarget::Root;
        Ok(())
    }

    /// The trigger bound to an action string such as `new_split:right`.
    pub fn trigger_for_action(&self, action: &str) -> Option<&Binding> {
        let action = Action::parse(action).ok()?;
        self.set.trigger_for(&action)
    }
}

fn default_keybinds(macos: bool) -> Vec<String> {
    let cs = if macos { "super" } else { "ctrl" };
    let mut v: Vec<String> = Vec::new();
    let mut add = |s: &str| v.push(s.to_string());

    add(&format!("{cs}+shift+,=reload_config"));
    add(&format!("{cs}+,=open_config"));
    add("copy=copy_to_clipboard");
    add("paste=paste_from_clipboard");
    if !macos {
        add("ctrl+insert=copy_to_clipboard");
        add("shift+insert=paste_from_clipboard");
    }
    let clip = if macos { "super" } else { "ctrl+shift" };
    add(&format!("performable:{clip}+c=copy_to_clipboard"));
    add(&format!("performable:{clip}+v=paste_from_clipboard"));

    add(&format!("{cs}+==increase_font_size:1"));
    add(&format!("{cs}++=increase_font_size:1"));
    add(&format!("{cs}+-=decrease_font_size:1"));
    add(&format!("{cs}+0=reset_font_size"));

    add("shift+ctrl+super+j=write_screen_file:copy");
    add(&format!("{cs}+shift+j=write_screen_file:paste"));
    add(&format!("{cs}+shift+alt+j=write_screen_file:open"));

    for (key, dir) in [
        ("arrow_left", "left"),
        ("arrow_right", "right"),
        ("arrow_up", "up"),
        ("arrow_down", "down"),
        ("page_up", "page_up"),
        ("page_down", "page_down"),
        ("home", "home"),
        ("end", "end"),
    ] {
        add(&format!("performable:shift+{key}=adjust_selection:{dir}"));
    }
    add("performable:shift+alt+arrow_left=adjust_selection:left_word");
    add("performable:shift+alt+arrow_right=adjust_selection:right_word");

    add("ctrl+shift+tab=previous_tab");
    add("ctrl+tab=next_tab");

    if !macos {
        add("ctrl+shift+n=new_window");
        add("ctrl+shift+w=close_surface");
        add("ctrl+shift+q=quit");
        add("alt+f4=close_window");
        add("ctrl+shift+t=new_tab");
        add("ctrl+shift+w=close_tab:this");
        add("performable:ctrl+shift+arrow_left=previous_tab");
        add("performable:ctrl+shift+arrow_right=next_tab");
        add("performable:ctrl+page_up=previous_tab");
        add("performable:ctrl+page_down=next_tab");
        add("ctrl+shift+o=new_split:right");
        add("ctrl+shift+e=new_split:down");
        add("performable:ctrl+super+[=goto_split:previous");
        add("performable:ctrl+super+]=goto_split:next");
        add("performable:ctrl+alt+arrow_up=goto_split:up");
        add("performable:ctrl+alt+arrow_down=goto_split:down");
        add("performable:ctrl+alt+arrow_left=goto_split:left");
        add("performable:ctrl+alt+arrow_right=goto_split:right");
        for dir in ["up", "down", "left", "right"] {
            add(&format!("performable:super+ctrl+shift+arrow_{dir}=resize_split:{dir},10"));
        }
        add("shift+home=scroll_to_top");
        add("shift+end=scroll_to_bottom");
        add("shift+page_up=scroll_page_up");
        add("shift+page_down=scroll_page_down");
        add("shift+ctrl+arrow_up=jump_to_prompt:-1");
        add("shift+ctrl+arrow_down=jump_to_prompt:1");
        add("performable:shift+ctrl+page_up=move_tab:-1");
        add("performable:shift+ctrl+page_down=move_tab:1");
        add("performable:ctrl+shift+f=start_search");
        add("performable:escape=end_search");
        add("shift+ctrl+i=inspector:toggle");
        add("shift+ctrl+a=select_all");
        add("shift+insert=paste_from_selection");
    }

    let tab_mod = if macos { "super" } else { "alt" };
    let perf = if macos { "" } else { "performable:" };
    for i in 1..=8 {
        add(&format!("{perf}{tab_mod}+digit_{i}=goto_tab:{i}"));
        add(&format!("{perf}{tab_mod}+{i}=goto_tab:{i}"));
    }
    add(&format!("{perf}{tab_mod}+9=last_tab"));

    add(&format!("{cs}+enter=toggle_fullscreen"));
    add(&format!("{cs}+shift+enter=toggle_split_zoom"));
    add(&format!("{cs}+shift+p=toggle_command_palette"));

    if macos {
        for line in [
            "super+q=quit",
            "performable:super+k=clear_screen",
            "super+a=select_all",
            "performable:super+shift+t=undo",
            "performable:super+z=undo",
            "performable:super+shift+z=redo",
            "super+home=scroll_to_top",
            "super+end=scroll_to_bottom",
            "super+page_up=scroll_page_up",
            "super+page_down=scroll_page_down",
            "performable:super+j=scroll_to_selection",
            "super+shift+arrow_up=jump_to_prompt:-1",
            "super+shift+arrow_down=jump_to_prompt:1",
            "super+n=new_window",
            "super+w=close_surface",
            "super+alt+w=close_tab:this",
            "super+shift+w=close_window",
            "super+shift+alt+w=close_all_windows",
            "super+t=new_tab",
            "super+shift+[=previous_tab",
            "super+shift+]=next_tab",
            "super+d=new_split:right",
            "super+shift+d=new_split:down",
            "super+[=goto_split:previous",
            "super+]=goto_split:next",
            "super+alt+arrow_up=goto_split:up",
            "super+alt+arrow_down=goto_split:down",
            "super+alt+arrow_left=goto_split:left",
            "super+alt+arrow_right=goto_split:right",
            "super+ctrl+arrow_up=resize_split:up,10",
            "super+ctrl+arrow_down=resize_split:down,10",
            "super+ctrl+arrow_left=resize_split:left,10",
            "super+ctrl+arrow_right=resize_split:right,10",
            "super+ctrl+==equalize_splits",
            "super+arrow_up=jump_to_prompt:-1",
            "super+arrow_down=jump_to_prompt:1",
            "performable:super+f=start_search",
            "performable:super+e=search_selection",
            "performable:super+shift+f=end_search",
            "performable:escape=end_search",
            "performable:super+g=navigate_search:next",
            "performable:super+shift+g=navigate_search:previous",
            "alt+super+i=inspector:toggle",
            "super+ctrl+f=toggle_fullscreen",
            "super+shift+v=paste_from_selection",
            "super+arrow_right=text:\\x05",
            "super+arrow_left=text:\\x01",
            "super+backspace=text:\\x15",
            "alt+arrow_left=esc:b",
            "alt+arrow_right=esc:f",
        ] {
            add(line);
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trigger(s: &str) -> Trigger {
        Trigger::parse(s).unwrap()
    }

    #[test]
    fn parses_modifiers_aliases_and_keys() {
        let t = trigger("cmd+shift+d");
        assert_eq!(t.mods, Mods::NONE.super_key().shift());
        assert_eq!(t.key, TriggerKey::Unicode('d'));
        let t = trigger("opt+control+arrow_left");
        assert_eq!(t.mods, Mods::NONE.alt().ctrl());
        assert_eq!(t.key, TriggerKey::Physical("arrow_left".into()));
        assert_eq!(trigger("ctrl++").key, TriggerKey::Unicode('+'));
        assert_eq!(trigger("ctrl+ArrowUp").key, TriggerKey::Physical("arrow_up".into()));
        assert_eq!(trigger("ctrl+KeyA").key, TriggerKey::Physical("key_a".into()));
        assert_eq!(trigger("up").key, TriggerKey::Physical("arrow_up".into()));
        assert_eq!(trigger("zero").key, TriggerKey::Unicode('0'));
        assert_eq!(trigger("catch_all").key, TriggerKey::CatchAll);
    }

    #[test]
    fn rejects_bad_triggers() {
        assert!(Trigger::parse("").is_err());
        assert!(Trigger::parse("ctrl+ctrl+a").is_err());
        assert!(Trigger::parse("a+b").is_err());
        assert!(Trigger::parse("ctrl+nonsense").is_err());
    }

    #[test]
    fn parses_bindings_with_flags_and_sequences() {
        let Parsed::Binding(b) = parse_single("performable:super+shift+d=new_split:right").unwrap() else {
            panic!()
        };
        assert!(b.flags.performable);
        assert_eq!(b.action, Action::new("new_split", Some("right")));
        assert_eq!(b.trigger().to_string(), "super+shift+d");

        let Parsed::Binding(b) = parse_single("ctrl+a>ctrl+b>n=new_window").unwrap() else { panic!() };
        assert_eq!(b.triggers.len(), 3);
        assert!(b.is_sequence());

        assert!(parse_single("global:ctrl+a>b=quit").is_err());
        assert!(parse_single("ctrl+a").is_err());
        assert!(parse_single("ctrl+a=not_an_action").is_err());
        assert!(parse_single("ctrl+a=quit:1").is_err());
        assert!(parse_single("ctrl+a=goto_tab").is_err());
        assert!(parse_single("ctrl+a=goto_tab:x").is_err());
    }

    #[test]
    fn equals_key_and_plus_key_are_handled() {
        let Parsed::Binding(b) = parse_single("ctrl+==increase_font_size:1").unwrap() else { panic!() };
        assert_eq!(b.trigger().key, TriggerKey::Unicode('='));
        let Parsed::Binding(b) = parse_single("ctrl++=increase_font_size:1").unwrap() else { panic!() };
        assert_eq!(b.trigger().key, TriggerKey::Unicode('+'));
    }

    #[test]
    fn defaults_expose_trigger_for_action() {
        let keybinds = Keybinds::defaults(true);
        let b = keybinds.trigger_for_action("new_split:right").unwrap();
        assert_eq!(b.trigger().to_string(), "super+d");
        let b = keybinds.trigger_for_action("goto_tab:3").unwrap();
        assert_eq!(b.trigger().key, TriggerKey::Unicode('3'));
        assert!(Keybinds::defaults(false).trigger_for_action("new_split:right").unwrap().trigger().mods.ctrl);
    }

    #[test]
    fn apply_override_clear_and_reset() {
        let mut k = Keybinds::defaults(true);
        k.apply("super+d=new_split:down", true).unwrap();
        assert_eq!(
            k.set.get(&[trigger("super+d")]).unwrap().action,
            Action::new("new_split", Some("down"))
        );
        k.apply("super+d=unbind", true).unwrap();
        assert!(k.set.get(&[trigger("super+d")]).is_none());
        k.apply("clear", true).unwrap();
        assert!(k.set.is_empty());
        k.apply("", true).unwrap();
        assert!(!k.set.is_empty());
    }

    #[test]
    fn key_tables_and_chains() {
        let mut k = Keybinds::default();
        k.apply("foo/shift+a=copy_to_clipboard", false).unwrap();
        k.apply("foo/shift+b=paste_from_clipboard", false).unwrap();
        assert_eq!(k.tables["foo"].len(), 2);
        assert!(k.set.is_empty());
        k.apply("foo/", false).unwrap();
        assert_eq!(k.tables["foo"].len(), 0);
        assert!(k.apply("/shift+a=copy_to_clipboard", false).is_err());

        let mut k = Keybinds::default();
        k.apply("ctrl+/=text:foo", false).unwrap();
        k.apply("ctrl+a=text:/hello", false).unwrap();
        k.apply("shift+/=ignore", false).unwrap();
        assert_eq!(k.set.len(), 3);
        assert!(k.tables.is_empty());

        k.apply("ctrl+a=new_tab", false).unwrap();
        k.apply("chain=goto_tab:2", false).unwrap();
        assert_eq!(k.set.get(&[trigger("ctrl+a")]).unwrap().chained.len(), 1);
        assert!(k.apply("foo/chain=quit", false).is_err());
    }

    #[test]
    fn action_defaults_compare_equal() {
        let k = Keybinds::defaults(true);
        assert!(k.trigger_for_action("new_split").is_none());
        assert!(k.trigger_for_action("close_tab").is_some());
        assert!(k.trigger_for_action("copy_to_clipboard").is_some());
    }
}
