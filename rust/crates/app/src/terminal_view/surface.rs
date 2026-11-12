//! Per-terminal startup settings: what to run, where, with which environment and input.
//!
//! A [`Surface`] merges the config (`command`, `initial-command`, `working-directory`, `env`,
//! `input`, `wait-after-command`, shell integration) with what the creator asked for, such as
//! the AppleScript `surface configuration`. [`TerminalView::new`](super::TerminalView::new)
//! cannot take it as an argument without touching every split and tab call site, so the
//! creator stages it with [`stage`] right before the view is built and the view takes it.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use angelly_core::config::Config;
use gpui_kit::{App, Global};
use terminal::shell_integration::{Features, Mode, ShellIntegration};
use terminal::{Command, SessionConfig};

use super::settings::Settings;
use crate::platform::automation::SurfaceConfig;

/// What the creator of a terminal wants on top of the config.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Surface {
    pub cwd: Option<PathBuf>,
    /// A command line in the `command` config syntax.
    pub command: Option<String>,
    pub env: Vec<(String, String)>,
    pub font_size: Option<f32>,
    pub initial_input: Option<String>,
    pub wait_after_command: bool,
}

impl From<&SurfaceConfig> for Surface {
    fn from(config: &SurfaceConfig) -> Self {
        Surface {
            cwd: config.working_directory.clone(),
            command: config.command.clone().filter(|c| !c.trim().is_empty()),
            env: config.environment.clone(),
            font_size: config.font_size.filter(|size| *size > 0.0),
            initial_input: config.initial_input.clone().filter(|i| !i.is_empty()),
            wait_after_command: config.wait_after_command,
        }
    }
}

#[derive(Default)]
struct Staged(Option<Surface>);

impl Global for Staged {}

/// Makes `surface` the settings of the next [`TerminalView`](super::TerminalView) created.
/// Call [`unstage`] afterwards in case no view got created.
pub fn stage(cx: &mut App, surface: Surface) {
    cx.set_global(Staged(Some(surface)));
}

pub fn unstage(cx: &mut App) {
    cx.set_global(Staged(None));
}

pub(super) fn take_staged(cx: &mut App) -> Option<Surface> {
    cx.try_global::<Staged>()?;
    cx.global_mut::<Staged>().0.take()
}

/// `initial-command` runs once, in the first terminal of the process.
static INITIAL_COMMAND_USED: AtomicBool = AtomicBool::new(false);

/// Everything the view needs to start a terminal.
pub(super) struct Launch {
    pub session: SessionConfig,
    pub initial_input: Vec<u8>,
    pub wait_after_command: bool,
    pub font_size: f32,
    /// The command as shown in the "failed to launch" message.
    pub command_line: String,
}

pub(super) struct Environment {
    pub resources: Option<PathBuf>,
    pub version: Option<String>,
    pub home: Option<PathBuf>,
    pub process_cwd: Option<PathBuf>,
}

impl Environment {
    pub fn current() -> Environment {
        Environment {
            resources: std::env::var_os("GHOSTTY_RESOURCES_DIR").filter(|v| !v.is_empty()).map(PathBuf::from),
            version: Some(crate::panels::about::AppInfo::current().version),
            home: std::env::var_os("HOME").map(PathBuf::from),
            process_cwd: std::env::current_dir().ok(),
        }
    }
}

pub(super) fn launch(config: &Config, settings: &Settings, mut surface: Surface, env: &Environment) -> Launch {
    let first = !INITIAL_COMMAND_USED.swap(true, Ordering::Relaxed);
    let command = surface
        .command
        .take()
        .or_else(|| first.then(|| config.get("initial-command")).flatten().map(str::to_string))
        .or_else(|| config.command().map(str::to_string))
        .and_then(|line| Command::parse(&line));

    let mut session_env = config.env();
    for (key, value) in surface.env {
        match session_env.iter_mut().find(|(k, _)| *k == key) {
            Some(slot) => slot.1 = value,
            None => session_env.push((key, value)),
        }
    }

    let mut input = parse_input(&config.get_all("input"));
    if let Some(initial) = &surface.initial_input {
        input.extend_from_slice(initial.as_bytes());
    }

    let integration = ShellIntegration {
        mode: Mode::parse(config.get("shell-integration").unwrap_or("detect")).unwrap_or_default(),
        features: Features::parse(config.get("shell-integration-features").unwrap_or("")),
        cursor_blink: settings.cursor_blink.unwrap_or(true),
    };
    let command_line = match &command {
        Some(Command::Direct(args)) => args.join(" "),
        Some(Command::Shell(line)) => line.clone(),
        None => String::new(),
    };
    let session = SessionConfig {
        colors: None,
        cwd: resolve_cwd(surface.cwd, config.working_directory(), env),
        command: command.map(|c| c.argv()),
        env: session_env,
        term: config.term().to_string(),
        resources_dir: env.resources.clone(),
        shell_integration: integration,
        term_program_version: env.version.clone(),
        scrollback_bytes: Some(config.scrollback_limit_bytes()),
        option_as_alt: settings.option_as_alt,
        grapheme_clustering: config.get("grapheme-width-method") != Some("legacy"),
        ..SessionConfig::default()
    };
    Launch {
        session,
        initial_input: input,
        wait_after_command: surface.wait_after_command || config.get("wait-after-command").is_some_and(is_true),
        font_size: surface.font_size.unwrap_or(settings.font_size),
        command_line,
    }
}

fn is_true(value: &str) -> bool {
    angelly_core::config::parse_bool(value).unwrap_or(false)
}

/// The directory a terminal starts in: the creator's choice, then `working-directory`
/// (`home`, `inherit` or a path), then the home directory when the process was started
/// from the desktop (its own directory is `/`).
pub(super) fn resolve_cwd(explicit: Option<PathBuf>, configured: Option<&str>, env: &Environment) -> Option<PathBuf> {
    if explicit.is_some() {
        return explicit;
    }
    let home = || env.home.clone();
    match configured.map(str::trim).filter(|v| !v.is_empty()) {
        Some("home") => home(),
        Some("inherit") => env.process_cwd.clone(),
        Some(path) => match path.strip_prefix("~/") {
            Some(rest) => env.home.as_ref().map(|h| h.join(rest)),
            None if path == "~" => home(),
            None => Some(PathBuf::from(path)),
        },
        None => match &env.process_cwd {
            Some(cwd) if cwd.as_path() != std::path::Path::new("/") => Some(cwd.clone()),
            _ => home(),
        },
    }
}

/// Parses the `input` entries: `raw:<text>` (Zig string escapes), `path:<file>` or bare text.
pub(super) fn parse_input(values: &[&str]) -> Vec<u8> {
    const MAX_FILE: u64 = 10 * 1024 * 1024;
    let mut out = Vec::new();
    for value in values {
        if let Some(path) = value.strip_prefix("path:") {
            let path = path.trim();
            match std::fs::metadata(path) {
                Ok(meta) if meta.is_file() && meta.len() <= MAX_FILE => {
                    if let Ok(bytes) = std::fs::read(path) {
                        out.extend(bytes);
                    }
                }
                _ => {}
            }
        } else {
            out.extend(unescape(value.strip_prefix("raw:").unwrap_or(value)));
        }
    }
    out
}

fn unescape(text: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut buf = [0u8; 4];
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            continue;
        }
        match chars.next() {
            Some('n') => out.push(b'\n'),
            Some('r') => out.push(b'\r'),
            Some('t') => out.push(b'\t'),
            Some('e') => out.push(0x1b),
            Some('0') => out.push(0),
            Some('x') => {
                let hex: String = (0..2).filter_map(|_| chars.next_if(char::is_ascii_hexdigit)).collect();
                match u8::from_str_radix(&hex, 16) {
                    Ok(byte) if hex.len() == 2 => out.push(byte),
                    _ => {
                        out.extend_from_slice(b"\\x");
                        out.extend_from_slice(hex.as_bytes());
                    }
                }
            }
            Some('u') if chars.peek() == Some(&'{') => {
                chars.next();
                let mut hex = String::new();
                for h in chars.by_ref() {
                    if h == '}' {
                        break;
                    }
                    hex.push(h);
                }
                if let Some(ch) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                    out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                }
            }
            Some(other) => out.extend_from_slice(other.encode_utf8(&mut buf).as_bytes()),
            None => out.push(b'\\'),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use angelly_core::config::{ColorScheme, Env};

    fn env(home: &str, cwd: &str) -> Environment {
        Environment {
            resources: Some(PathBuf::from("/res/ghostty")),
            version: Some("9.9".into()),
            home: Some(PathBuf::from(home)),
            process_cwd: Some(PathBuf::from(cwd)),
        }
    }

    fn config(text: &str) -> Config {
        Config::from_str(text, &Env::from_process(), ColorScheme::Dark)
    }

    #[test]
    fn working_directory_precedence() {
        let desktop = env("/Users/me", "/");
        let shell = env("/Users/me", "/work/proj");
        assert_eq!(resolve_cwd(Some("/x".into()), Some("home"), &desktop), Some("/x".into()));
        assert_eq!(resolve_cwd(None, None, &desktop), Some("/Users/me".into()));
        assert_eq!(resolve_cwd(None, None, &shell), Some("/work/proj".into()));
        assert_eq!(resolve_cwd(None, Some("home"), &shell), Some("/Users/me".into()));
        assert_eq!(resolve_cwd(None, Some("inherit"), &desktop), Some("/".into()));
        assert_eq!(resolve_cwd(None, Some("~/src"), &shell), Some("/Users/me/src".into()));
        assert_eq!(resolve_cwd(None, Some("/opt"), &shell), Some("/opt".into()));
    }

    #[test]
    fn input_forms() {
        assert_eq!(parse_input(&["Hello"]), b"Hello");
        assert_eq!(parse_input(&["raw:ls\\n", "a\\x1bb", "\\u{e9}"]), "ls\na\u{1b}b\u{e9}".as_bytes());
        assert_eq!(parse_input(&["\\q", "tail\\"]), b"qtail\\");
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("in.txt");
        std::fs::write(&file, "from file").unwrap();
        assert_eq!(parse_input(&[&format!("path:{}", file.display()), "path:/nonexistent"]), b"from file");
    }

    #[test]
    fn launch_merges_config_and_surface() {
        let c = config(
            "command = fish -l\nenv = A=1\nenv = B=2\nworking-directory = home\n\
             shell-integration = none\nshell-integration-features = sudo\n\
             input = echo hi\\n\nwait-after-command = true\nterm = xterm-256color\n",
        );
        let settings = Settings::from_config(&c);
        let surface = Surface {
            env: vec![("B".into(), "3".into()), ("C".into(), "4".into())],
            font_size: Some(18.0),
            initial_input: Some("more".into()),
            ..Surface::default()
        };
        let l = launch(&c, &settings, surface, &env("/Users/me", "/"));
        assert_eq!(l.session.command, Some(vec!["fish".to_string(), "-l".into()]));
        assert_eq!(
            l.session.env,
            vec![("A".to_string(), "1".to_string()), ("B".into(), "3".into()), ("C".into(), "4".into())]
        );
        assert_eq!(l.session.cwd, Some("/Users/me".into()));
        assert_eq!(l.session.term, "xterm-256color");
        assert_eq!(l.session.shell_integration.mode, Mode::None);
        assert!(l.session.shell_integration.features.sudo);
        assert_eq!(l.session.resources_dir, Some("/res/ghostty".into()));
        assert_eq!(l.initial_input, b"echo hi\nmore");
        assert!(l.wait_after_command);
        assert_eq!(l.font_size, 18.0);
        assert_eq!(l.command_line, "fish -l");
    }

    #[test]
    fn surface_command_beats_config_and_defaults_apply_otherwise() {
        let c = config("command = fish\n");
        let settings = Settings::from_config(&c);
        let surface = Surface { command: Some("direct:vim a b".into()), ..Surface::default() };
        let l = launch(&c, &settings, surface, &env("/h", "/"));
        assert_eq!(l.session.command, Some(vec!["vim".to_string(), "a".into(), "b".into()]));
        assert!(!l.wait_after_command);
        assert_eq!(l.font_size, settings.font_size);
        assert_eq!(l.session.shell_integration.mode, Mode::Detect);

        let l = launch(&config(""), &settings, Surface::default(), &env("/h", "/"));
        assert_eq!(l.session.command, None);
        assert_eq!(l.session.term, "xterm-ghostty");
    }

    #[test]
    fn automation_config_converts() {
        let automation = SurfaceConfig {
            font_size: Some(0.0),
            working_directory: Some("/tmp".into()),
            command: Some("  ".into()),
            initial_input: Some("x".into()),
            wait_after_command: true,
            environment: vec![("K".into(), "V".into())],
        };
        let surface = Surface::from(&automation);
        assert_eq!(surface.font_size, None);
        assert_eq!(surface.command, None);
        assert_eq!(surface.cwd, Some("/tmp".into()));
        assert!(surface.wait_after_command);
        assert_eq!(surface.env, vec![("K".to_string(), "V".to_string())]);
    }
}
