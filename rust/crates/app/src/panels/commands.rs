//! The entries the command palette offers: update commands, "Focus: tab"
//! jumps and Ghostty's `command-palette-entry` commands.

use angelly_core::config::{Config, Trigger, TriggerKey};
use angelly_core::update::UpdateState;
use gpui_kit::assets::IconName;
use gpui_kit::*;

use super::default_commands::DEFAULT_COMMANDS;
use super::format::abbreviate_home;
use super::fuzzy::{palette_sort_key, Matchable};
use super::update::icon_of;

/// What running a palette entry does.
#[derive(Debug, Clone, PartialEq)]
pub enum CommandAction {
    /// A Ghostty binding action such as `new_split:right`, run by the host.
    Binding(String),
    /// Select the tab at this index.
    FocusTab(usize),
    InstallUpdate,
    CancelUpdate,
}

#[derive(Debug, Clone)]
pub struct CommandOption {
    pub title: String,
    pub subtitle: Option<String>,
    pub description: Option<String>,
    pub symbols: Vec<String>,
    pub icon: Option<IconName>,
    pub color: Option<Hsla>,
    pub badge: Option<String>,
    pub emphasis: bool,
    pub action: CommandAction,
}

impl CommandOption {
    fn new(title: impl Into<String>, action: CommandAction) -> Self {
        Self {
            title: title.into(),
            subtitle: None,
            description: None,
            symbols: Vec::new(),
            icon: None,
            color: None,
            badge: None,
            emphasis: false,
            action,
        }
    }

    /// Subtitle shown under the title: the explicit one, else the description.
    pub fn detail(&self) -> Option<&str> {
        self.subtitle.as_deref().or(self.description.as_deref())
    }
}

impl Matchable for CommandOption {
    fn title(&self) -> &str {
        &self.title
    }

    fn subtitle(&self) -> Option<&str> {
        self.subtitle.as_deref()
    }

    fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    fn leading_color(&self) -> Option<[f32; 3]> {
        self.color.map(|c| {
            let rgba = Rgba::from(c);
            [rgba.r, rgba.g, rgba.b]
        })
    }
}

/// A tab the palette can jump to.
#[derive(Debug, Clone)]
pub struct TabEntry {
    pub title: String,
    pub pwd: Option<String>,
    pub color: Option<Hsla>,
}

/// A `command-palette-entry`: title, description and the action to run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteCommand {
    pub title: String,
    pub description: String,
    pub action: String,
}

impl PaletteCommand {
    /// The action name without its parameter, e.g. `goto_split`.
    pub fn action_key(&self) -> &str {
        self.action.split(':').next().unwrap_or("")
    }

    pub fn is_supported(&self) -> bool {
        !UNSUPPORTED_ACTIONS.contains(&self.action_key())
    }
}

/// Actions that make no sense for this app.
const UNSUPPORTED_ACTIONS: &[&str] = &[
    "toggle_tab_overview",
    "toggle_window_decorations",
    "prompt_window_title",
    "show_gtk_inspector",
];

pub fn default_commands() -> Vec<PaletteCommand> {
    DEFAULT_COMMANDS
        .iter()
        .map(|(title, description, action)| PaletteCommand {
            title: title.to_string(),
            description: description.to_string(),
            action: action.to_string(),
        })
        .collect()
}

/// Parses `title:Foo,description:"Bar, baz",action:goto_split:left`.
/// Values may be double-quoted with `\"` and `\\` escapes; the action keeps any
/// colons it contains. Returns `None` when title or action is missing.
pub fn parse_command(value: &str) -> Option<PaletteCommand> {
    let mut command = PaletteCommand { title: String::new(), description: String::new(), action: String::new() };
    let mut chars = value.chars().peekable();
    loop {
        let mut key = String::new();
        while let Some(&c) = chars.peek() {
            if c == ':' || c == ',' {
                break;
            }
            key.push(c);
            chars.next();
        }
        if chars.next() != Some(':') {
            return None;
        }
        let mut text = String::new();
        if chars.peek() == Some(&'"') {
            chars.next();
            while let Some(c) = chars.next() {
                match c {
                    '"' => break,
                    '\\' => match chars.next() {
                        Some(escaped) => text.push(escaped),
                        None => return None,
                    },
                    c => text.push(c),
                }
            }
        } else {
            while let Some(&c) = chars.peek() {
                if c == ',' {
                    break;
                }
                text.push(c);
                chars.next();
            }
        }
        match key.trim() {
            "title" => command.title = text,
            "description" => command.description = text,
            "action" => command.action = text,
            _ => return None,
        }
        match chars.next() {
            Some(',') => continue,
            None => break,
            Some(_) => return None,
        }
    }
    (!command.title.is_empty() && !command.action.is_empty()).then_some(command)
}

/// The effective command list: Ghostty's defaults, adjusted by the user's
/// `command-palette-entry` lines (`clear` drops everything so far, an empty
/// value restores the defaults).
pub fn commands_for(config: &Config) -> Vec<PaletteCommand> {
    let mut commands = default_commands();
    for entry in config.entries().iter().filter(|e| e.key == "command-palette-entry") {
        match entry.value.as_deref() {
            Some("clear") => commands.clear(),
            Some("") | None => commands = default_commands(),
            Some(value) => commands.extend(parse_command(value)),
        }
    }
    commands
}

/// Glyphs for a trigger in the order macOS shows them: ⌃⌥⇧⌘ then the key.
pub fn shortcut_symbols(trigger: &Trigger) -> Vec<String> {
    let mut symbols = Vec::new();
    let mods = trigger.mods;
    for (on, glyph) in [(mods.ctrl, "⌃"), (mods.alt, "⌥"), (mods.shift, "⇧"), (mods.super_key, "⌘")] {
        if on {
            symbols.push(glyph.to_string());
        }
    }
    symbols.push(match &trigger.key {
        TriggerKey::Unicode(c) => c.to_uppercase().to_string(),
        TriggerKey::Physical(name) => key_glyph(name),
        TriggerKey::CatchAll => "*".to_string(),
    });
    symbols
}

fn key_glyph(name: &str) -> String {
    let fixed = match name {
        "arrow_up" => "↑",
        "arrow_down" => "↓",
        "arrow_left" => "←",
        "arrow_right" => "→",
        "enter" | "numpad_enter" => "↩",
        "tab" => "⇥",
        "escape" => "⎋",
        "space" => "␣",
        "backspace" => "⌫",
        "delete" => "⌦",
        "home" => "↖",
        "end" => "↘",
        "page_up" => "⇞",
        "page_down" => "⇟",
        "comma" => ",",
        "period" => ".",
        "slash" => "/",
        "semicolon" => ";",
        "quote" => "'",
        "minus" => "-",
        "equal" => "=",
        "bracket_left" => "[",
        "bracket_right" => "]",
        "backslash" => "\\",
        "backquote" => "`",
        _ => "",
    };
    if !fixed.is_empty() {
        return fixed.to_string();
    }
    if let Some(letter) = name.strip_prefix("key_").or_else(|| name.strip_prefix("digit_")) {
        return letter.to_uppercase();
    }
    name.to_uppercase().replace('_', " ")
}

/// What the palette needs to know about the app to list its commands.
pub struct PaletteSource<'a> {
    pub config: &'a Config,
    pub tabs: &'a [TabEntry],
    pub update: &'a UpdateState,
    pub home: Option<&'a str>,
}

fn update_options(state: &UpdateState) -> Vec<CommandOption> {
    let mut options = Vec::new();
    if state.is_installable() {
        let title = match state {
            UpdateState::UpdateAvailable(_) => "Update Ghostty and Restart".to_string(),
            other => other.text(),
        };
        let mut option = CommandOption::new(title, CommandAction::InstallUpdate);
        option.description = Some(state.description());
        option.icon = Some(state.icon().map(icon_of).unwrap_or(IconName::Package2));
        option.badge = state.badge();
        option.emphasis = true;
        options.push(option);
    }
    if state.is_cancellable() {
        let mut option = CommandOption::new("Cancel or Skip Update", CommandAction::CancelUpdate);
        option.description = Some("Dismiss the current update process".to_string());
        options.push(option);
    }
    options
}

fn jump_options(tabs: &[TabEntry], home: Option<&str>) -> Vec<CommandOption> {
    tabs.iter()
        .enumerate()
        .map(|(index, tab)| {
            let title = if tab.title.is_empty() { "Untitled" } else { tab.title.as_str() };
            let pwd = tab.pwd.as_deref().map(|p| abbreviate_home(p, home));
            let mut option = CommandOption::new(format!("Focus: {title}"), CommandAction::FocusTab(index));
            option.subtitle = pwd.filter(|p| !title.contains(p.as_str()));
            option.icon = Some(IconName::SquareStack);
            option.color = tab.color;
            option
        })
        .collect()
}

fn terminal_options(config: &Config) -> Vec<CommandOption> {
    commands_for(config)
        .into_iter()
        .filter(PaletteCommand::is_supported)
        .map(|command| {
            let symbols = config
                .keybinds()
                .trigger_for_action(&command.action)
                .filter(|binding| !binding.is_sequence())
                .map(|binding| shortcut_symbols(binding.trigger()))
                .unwrap_or_default();
            let mut option = CommandOption::new(command.title, CommandAction::Binding(command.action));
            option.description = Some(command.description).filter(|d| !d.is_empty());
            option.symbols = symbols;
            option
        })
        .collect()
}

/// Update commands first, then focus jumps and terminal commands sorted by
/// title (":" before space), with the original order as the tie-break.
pub fn build_options(source: &PaletteSource) -> Vec<CommandOption> {
    let mut options = update_options(source.update);
    let mut rest = jump_options(source.tabs, source.home);
    rest.extend(terminal_options(source.config));
    rest.sort_by_cached_key(|option| palette_sort_key(&option.title));
    options.extend(rest);
    options
}

#[cfg(test)]
mod tests {
    use std::prelude::v1::test;

    use super::*;
    use angelly_core::config::{ColorScheme, Env, Platform};
    use angelly_core::update::{ReleaseInfo, UpdateState};

    fn env() -> Env {
        Env { platform: Platform::MacOs, home: None, xdg_config_home: None, resources_dir: None }
    }

    fn config(text: &str) -> Config {
        Config::from_str(text, &env(), ColorScheme::Dark)
    }

    #[test]
    fn defaults_are_loaded_and_unsupported_actions_filtered() {
        let commands = default_commands();
        assert!(commands.len() > 80);
        assert!(commands.iter().any(|c| c.title == "New Tab" && c.action == "new_tab"));
        let options = terminal_options(&config(""));
        assert!(options.iter().all(|o| o.title != "Change Window Title…"));
        assert!(options.iter().any(|o| o.title == "New Tab"));
    }

    #[test]
    fn parses_quoted_values_and_actions_with_colons() {
        let command = parse_command(r#"title:"Say, hi",description:"A \"quote\"",action:goto_split:left"#).unwrap();
        assert_eq!(command.title, "Say, hi");
        assert_eq!(command.description, "A \"quote\"");
        assert_eq!(command.action, "goto_split:left");
        assert_eq!(parse_command("title:x"), None);
        assert_eq!(parse_command("bogus"), None);
    }

    #[test]
    fn user_entries_extend_clear_and_reset_defaults() {
        let extra = "command-palette-entry = title:Mine,action:text:hi\n";
        let with = commands_for(&config(extra));
        assert_eq!(with.last().unwrap().title, "Mine");
        let cleared = commands_for(&config("command-palette-entry = clear\ncommand-palette-entry = title:Only,action:reset\n"));
        assert_eq!(cleared.len(), 1);
        let reset = commands_for(&config("command-palette-entry = clear\ncommand-palette-entry =\n"));
        assert_eq!(reset.len(), default_commands().len());
    }

    #[test]
    fn shortcut_glyphs_follow_mac_order() {
        let trigger = Trigger::parse("super+shift+key_p").unwrap();
        assert_eq!(shortcut_symbols(&trigger), ["⇧", "⌘", "P"]);
        let trigger = Trigger::parse("ctrl+alt+arrow_left").unwrap();
        assert_eq!(shortcut_symbols(&trigger), ["⌃", "⌥", "←"]);
    }

    #[test]
    fn jump_entries_abbreviate_paths_and_skip_redundant_subtitles() {
        let tabs = [
            TabEntry { title: "vim".into(), pwd: Some("/Users/me/src".into()), color: None },
            TabEntry { title: "~/src".into(), pwd: Some("/Users/me/src".into()), color: None },
            TabEntry { title: String::new(), pwd: None, color: None },
        ];
        let options = jump_options(&tabs, Some("/Users/me"));
        assert_eq!(options[0].title, "Focus: vim");
        assert_eq!(options[0].subtitle.as_deref(), Some("~/src"));
        assert_eq!(options[1].subtitle, None);
        assert_eq!(options[2].title, "Focus: Untitled");
    }

    #[test]
    fn update_commands_come_first_and_depend_on_state() {
        let release = ReleaseInfo {
            tag: "v1.2.3".into(),
            name: None,
            page_url: String::new(),
            published_at: None,
            body: None,
            package: None,
            checksum: None,
        };
        let cfg = config("");
        let state = UpdateState::UpdateAvailable(release);
        let options = build_options(&PaletteSource { config: &cfg, tabs: &[], update: &state, home: None });
        assert_eq!(options[0].title, "Update Ghostty and Restart");
        assert!(options[0].emphasis);
        assert_eq!(options[0].badge.as_deref(), Some("1.2.3"));
        assert_eq!(options[1].title, "Cancel or Skip Update");

        let idle = build_options(&PaletteSource { config: &cfg, tabs: &[], update: &UpdateState::Idle, home: None });
        assert!(idle.iter().all(|o| o.action != CommandAction::InstallUpdate));
    }

    #[test]
    fn terminal_and_jump_entries_sort_by_title() {
        let cfg = config("");
        let tabs = [TabEntry { title: "zsh".into(), pwd: None, color: None }];
        let options = build_options(&PaletteSource { config: &cfg, tabs: &tabs, update: &UpdateState::Idle, home: None });
        let titles: Vec<_> = options.iter().map(|o| o.title.to_lowercase()).collect();
        let mut sorted = titles.clone();
        sorted.sort_by_key(|t| palette_sort_key(t));
        assert_eq!(titles, sorted);
    }
}
