//! What a session's child process is started with: the command line and environment, built
//! the way Ghostty's `termio/Exec.zig` does (terminfo, resource and binary directories,
//! shell integration, then the user's `env` overrides).

use std::path::{Path, PathBuf};

use crate::shell_integration::{self, Env};

const GHOSTTY_TERM: &str = "xterm-ghostty";
const FALLBACK_TERM: &str = "xterm-256color";
const PATH_SEPARATOR: char = if cfg!(windows) { ';' } else { ':' };

pub(crate) struct Launch {
    pub argv: Vec<String>,
    pub env: Env,
    /// The `$TERM` the child sees.
    pub term: String,
}

pub(crate) struct Inputs<'a> {
    /// The program and arguments to run (the user's login shell by default).
    pub command: &'a [String],
    pub env: &'a [(String, String)],
    /// The configured `term`.
    pub term: &'a str,
    /// Directory holding `shell-integration` and the like (`Contents/Resources/ghostty`).
    pub resources: Option<&'a Path>,
    pub integration: &'a shell_integration::ShellIntegration,
    pub exe_dir: Option<&'a Path>,
    pub term_program_version: Option<&'a str>,
}

/// `<resources>/../terminfo`, the layout of an app bundle and of `<prefix>/share`.
fn terminfo_dir(resources: &Path) -> Option<PathBuf> {
    let dir = resources.parent()?.join("terminfo");
    dir.is_dir().then_some(dir)
}

fn append_path(env: &mut Env, key: &str, value: &str, default: &str) {
    let current = env.get(key).cloned().unwrap_or_else(|| default.to_string());
    let joined = if current.is_empty() { value.to_string() } else { format!("{current}:{value}") };
    env.insert(key.to_string(), joined);
}

pub(crate) fn build(inputs: &Inputs, base: Env) -> Launch {
    let mut env = base;
    for key in ["COLUMNS", "LINES", "VTE_VERSION"] {
        env.remove(key);
    }

    let mut term = inputs.term.to_string();
    env.insert("COLORTERM".into(), "truecolor".into());
    match inputs.resources {
        Some(resources) => {
            env.insert("GHOSTTY_RESOURCES_DIR".into(), resources.to_string_lossy().into_owned());
            match terminfo_dir(resources) {
                Some(dir) => {
                    env.insert("TERMINFO".into(), dir.to_string_lossy().into_owned());
                }
                None if term == GHOSTTY_TERM => term = FALLBACK_TERM.to_string(),
                None => {}
            }
        }
        None if term == GHOSTTY_TERM => term = FALLBACK_TERM.to_string(),
        None => {}
    }
    env.insert("TERM".into(), term.clone());

    if cfg!(unix) && !env.contains_key("LANG") {
        let lang = if cfg!(target_os = "macos") { "en_US.UTF-8" } else { "C.UTF-8" };
        env.insert("LANG".into(), lang.into());
    }

    if let Some(dir) = inputs.exe_dir {
        let dir = dir.to_string_lossy().into_owned();
        env.insert("GHOSTTY_BIN_DIR".into(), dir.clone());
        match env.get("PATH").cloned() {
            Some(path) if path.split(PATH_SEPARATOR).any(|entry| entry == dir) => {}
            Some(path) if !path.is_empty() => {
                env.insert("PATH".into(), format!("{path}{PATH_SEPARATOR}{dir}"));
            }
            _ => {
                env.insert("PATH".into(), dir);
            }
        }
    }

    if cfg!(target_os = "macos") {
        if let Some(resources) = inputs.resources {
            let data = format!("{}/..", resources.display());
            append_path(&mut env, "XDG_DATA_DIRS", &data, "/usr/local/share:/usr/share");
            // Always colon-joined: an empty MANPATH must stay an extra entry, not replace the defaults.
            let man = format!("{}/../man", resources.display());
            let current = env.get("MANPATH").cloned().unwrap_or_default();
            env.insert("MANPATH".into(), format!("{current}:{man}"));
        }
    }

    env.insert("TERM_PROGRAM".into(), "ghostty".into());
    if let Some(version) = inputs.term_program_version {
        env.insert("TERM_PROGRAM_VERSION".into(), version.to_string());
    }

    let mut argv = inputs.command.to_vec();

    let integration = inputs.integration;
    if let Some(features) = integration.features.env_value(integration.cursor_blink) {
        env.insert("GHOSTTY_SHELL_FEATURES".into(), features);
    }
    if let Some(resources) = inputs.resources {
        if let Some(done) = shell_integration::setup(argv.clone(), resources, integration.mode, &mut env) {
            argv = done.argv;
        }
    }

    for (key, value) in inputs.env {
        env.insert(key.clone(), value.clone());
    }
    Launch { argv, env, term }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell_integration::{Features, Mode, Shell, ShellIntegration};

    fn bundle(terminfo: bool) -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("ghostty/shell-integration/zsh")).unwrap();
        if terminfo {
            std::fs::create_dir_all(root.path().join("terminfo/78")).unwrap();
        }
        root
    }

    fn zsh() -> Vec<String> {
        vec!["/bin/zsh".to_string(), "-l".to_string()]
    }

    fn launch(inputs: &Inputs, env: &[(&str, &str)]) -> Launch {
        build(inputs, env.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect())
    }

    fn inputs<'a>(
        command: &'a [String],
        resources: Option<&'a Path>,
        integration: &'a ShellIntegration,
    ) -> Inputs<'a> {
        Inputs {
            command,
            env: &[],
            term: "xterm-ghostty",
            resources,
            integration,
            exe_dir: None,
            term_program_version: Some("1.2.3"),
        }
    }

    #[test]
    fn xterm_ghostty_needs_the_bundled_terminfo() {
        let integration = ShellIntegration::default();
        let command = zsh();
        let with = bundle(true);
        let resources = with.path().join("ghostty");
        let l = launch(&inputs(&command, Some(&resources), &integration), &[]);
        assert_eq!(l.term, "xterm-ghostty");
        assert_eq!(l.env["TERM"], "xterm-ghostty");
        assert_eq!(l.env["TERMINFO"], with.path().join("terminfo").to_string_lossy());
        assert_eq!(l.env["GHOSTTY_RESOURCES_DIR"], resources.to_string_lossy());

        let without = bundle(false);
        let resources = without.path().join("ghostty");
        let l = launch(&inputs(&command, Some(&resources), &integration), &[]);
        assert_eq!(l.term, "xterm-256color");
        assert!(!l.env.contains_key("TERMINFO"));

        let l = launch(&inputs(&command, None, &integration), &[]);
        assert_eq!(l.env["TERM"], "xterm-256color");
        assert_eq!(l.env["COLORTERM"], "truecolor");
        assert!(!l.env.contains_key("GHOSTTY_RESOURCES_DIR"));
    }

    #[test]
    fn an_explicit_term_is_kept() {
        let integration = ShellIntegration::default();
        let command = zsh();
        let mut i = inputs(&command, None, &integration);
        i.term = "screen-256color";
        assert_eq!(launch(&i, &[]).env["TERM"], "screen-256color");
    }

    #[test]
    fn identifies_the_terminal_and_cleans_the_environment() {
        let integration = ShellIntegration::default();
        let command = zsh();
        let i = inputs(&command, None, &integration);
        let l = launch(&i, &[("COLUMNS", "9"), ("LINES", "9"), ("VTE_VERSION", "7000")]);
        assert_eq!(l.env["TERM_PROGRAM"], "ghostty");
        assert_eq!(l.env["TERM_PROGRAM_VERSION"], "1.2.3");
        for key in ["COLUMNS", "LINES", "VTE_VERSION"] {
            assert!(!l.env.contains_key(key));
        }
        assert_eq!(l.env.contains_key("LANG"), cfg!(unix));
        assert_eq!(launch(&i, &[("LANG", "de_DE.UTF-8")]).env["LANG"], "de_DE.UTF-8");
    }

    #[cfg(unix)]
    #[test]
    fn appends_the_binary_directory_once() {
        let integration = ShellIntegration::default();
        let command = zsh();
        let exe = Path::new("/App/Contents/MacOS");
        let mut i = inputs(&command, None, &integration);
        i.exe_dir = Some(exe);
        let l = launch(&i, &[("PATH", "/usr/bin")]);
        assert_eq!(l.env["PATH"], "/usr/bin:/App/Contents/MacOS");
        assert_eq!(l.env["GHOSTTY_BIN_DIR"], "/App/Contents/MacOS");
        let l = launch(&i, &[("PATH", "/App/Contents/MacOS:/usr/bin")]);
        assert_eq!(l.env["PATH"], "/App/Contents/MacOS:/usr/bin");
        assert_eq!(launch(&i, &[]).env["PATH"], "/App/Contents/MacOS");
    }

    #[test]
    fn injects_shell_integration_and_lets_config_env_win() {
        let integration = ShellIntegration {
            mode: Mode::Detect,
            features: Features::parse("sudo"),
            cursor_blink: true,
        };
        let command = zsh();
        let bundle = bundle(true);
        let resources = bundle.path().join("ghostty");

        let l = launch(&inputs(&command, Some(&resources), &integration), &[]);
        assert_eq!(l.argv, command);
        assert_eq!(l.env["GHOSTTY_SHELL_FEATURES"], "cursor:blink,path,sudo,title");
        assert_eq!(l.env["ZDOTDIR"], resources.join("shell-integration/zsh").to_string_lossy());

        let overrides = [("ZDOTDIR".to_string(), "/mine".to_string())];
        let mut i = inputs(&command, Some(&resources), &integration);
        i.env = &overrides;
        assert_eq!(launch(&i, &[]).env["ZDOTDIR"], "/mine");

        let off = ShellIntegration { mode: Mode::None, ..integration.clone() };
        let l = launch(&inputs(&command, Some(&resources), &off), &[]);
        assert!(!l.env.contains_key("ZDOTDIR"));
        assert!(l.env.contains_key("GHOSTTY_SHELL_FEATURES"));

        let forced = ShellIntegration { mode: Mode::Force(Shell::Zsh), ..integration };
        let other = ["/usr/bin/true".to_string()];
        let l = launch(&inputs(&other, Some(&resources), &forced), &[]);
        assert_eq!(l.argv, other);
        assert!(l.env.contains_key("ZDOTDIR"));
    }
}
