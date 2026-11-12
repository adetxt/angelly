//! Automatic shell integration, injected the way Ghostty's `termio/shell_integration.zig`
//! does it: environment variables and command line tweaks that make the shell load the
//! scripts under `<resources>/shell-integration`, which emit OSC 133 prompt marks, report the
//! working directory and wire up the `sudo`, `ssh-*`, `title`, `cursor` and `path` features.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

/// The environment a child is started with, edited by the setup functions.
pub type Env = BTreeMap<String, String>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shell {
    Bash,
    Elvish,
    Fish,
    Nushell,
    Zsh,
}

/// The `shell-integration` config value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Mode {
    /// Never inject.
    None,
    /// Inject for the shell found in the command (the default).
    #[default]
    Detect,
    /// Inject for this shell whatever the command is.
    Force(Shell),
}

impl Mode {
    pub fn parse(value: &str) -> Option<Mode> {
        Some(match value.trim() {
            "none" => Mode::None,
            "detect" => Mode::Detect,
            "bash" => Mode::Force(Shell::Bash),
            "elvish" => Mode::Force(Shell::Elvish),
            "fish" => Mode::Force(Shell::Fish),
            "nushell" => Mode::Force(Shell::Nushell),
            "zsh" => Mode::Force(Shell::Zsh),
            _ => return None,
        })
    }
}

/// The `shell-integration-features` config value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Features {
    pub cursor: bool,
    pub sudo: bool,
    pub title: bool,
    pub ssh_env: bool,
    pub ssh_terminfo: bool,
    pub path: bool,
}

impl Default for Features {
    fn default() -> Self {
        Features { cursor: true, sudo: false, title: true, ssh_env: false, ssh_terminfo: false, path: true }
    }
}

impl Features {
    /// Parses a comma separated list such as `no-cursor,sudo`; `true`/`false` switch everything.
    pub fn parse(value: &str) -> Features {
        let mut features = Features::default();
        for part in value.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            match part {
                "true" | "false" => {
                    let all = part == "true";
                    features = Features {
                        cursor: all,
                        sudo: all,
                        title: all,
                        ssh_env: all,
                        ssh_terminfo: all,
                        path: all,
                    };
                    continue;
                }
                _ => {}
            }
            let (name, on) = match part.strip_prefix("no-") {
                Some(name) => (name, false),
                None => (part, true),
            };
            match name {
                "cursor" => features.cursor = on,
                "sudo" => features.sudo = on,
                "title" => features.title = on,
                "ssh-env" => features.ssh_env = on,
                "ssh-terminfo" => features.ssh_terminfo = on,
                "path" => features.path = on,
                _ => {}
            }
        }
        features
    }

    /// The `GHOSTTY_SHELL_FEATURES` value: enabled names sorted case-insensitively, with the
    /// cursor feature carrying its blink state. `None` when everything is off.
    pub fn env_value(&self, cursor_blink: bool) -> Option<String> {
        let mut names = Vec::new();
        if self.cursor {
            names.push(if cursor_blink { "cursor:blink" } else { "cursor:steady" });
        }
        for (on, name) in [
            (self.path, "path"),
            (self.ssh_env, "ssh-env"),
            (self.ssh_terminfo, "ssh-terminfo"),
            (self.sudo, "sudo"),
            (self.title, "title"),
        ] {
            if on {
                names.push(name);
            }
        }
        (!names.is_empty()).then(|| names.join(","))
    }
}

/// The shell integration settings of a session.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct ShellIntegration {
    pub mode: Mode,
    pub features: Features,
    pub cursor_blink: bool,
}

/// What the command line looks like after injection, and for which shell.
#[derive(Debug, PartialEq, Eq)]
pub struct Injected {
    pub shell: Shell,
    pub argv: Vec<String>,
}

/// Prepares `env` and the command line for shell integration. Returns `None`, leaving the
/// environment as it was (apart from the feature list), when the shell is not recognised or
/// the integration files are missing.
pub fn setup(
    mut argv: Vec<String>,
    resources: &Path,
    mode: Mode,
    env: &mut Env,
) -> Option<Injected> {
    let shell = match mode {
        Mode::None => return None,
        Mode::Force(shell) => shell,
        Mode::Detect => detect_shell(argv.first()?)?,
    };
    let argv = match shell {
        Shell::Bash => setup_bash(&argv, resources, env)?,
        Shell::Nushell => setup_nushell(&argv, resources, env)?,
        Shell::Zsh => {
            setup_zsh(resources, env)?;
            std::mem::take(&mut argv)
        }
        Shell::Elvish | Shell::Fish => {
            setup_xdg_data_dirs(resources, env)?;
            std::mem::take(&mut argv)
        }
    };
    Some(Injected { shell, argv })
}

pub fn detect_shell(arg0: &str) -> Option<Shell> {
    let exe = Path::new(arg0).file_name()?.to_str()?;
    match exe {
        "bash" => {
            // Apple's patched bash 3.2 ignores the ENV startup path the injection relies on.
            if cfg!(target_os = "macos") && arg0 == "/bin/bash" {
                return None;
            }
            Some(Shell::Bash)
        }
        "elvish" => Some(Shell::Elvish),
        "fish" => Some(Shell::Fish),
        "nu" => Some(Shell::Nushell),
        "zsh" => Some(Shell::Zsh),
        _ => None,
    }
}

fn integration_dir(resources: &Path) -> Option<PathBuf> {
    let dir = resources.join("shell-integration");
    dir.is_dir().then_some(dir)
}

fn setup_bash(argv: &[String], resources: &Path, env: &mut Env) -> Option<Vec<String>> {
    let mut args = argv.iter();
    let mut out = vec![args.next()?.clone(), "--posix".to_string()];
    let mut inject = String::from("1");
    let mut rcfile = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--posix" => return None,
            "--norc" => inject.push_str(" --norc"),
            "--noprofile" => inject.push_str(" --noprofile"),
            "--rcfile" | "--init-file" => rcfile = args.next().cloned(),
            "-" | "--" => {
                out.push(arg.clone());
                out.extend(args.by_ref().cloned());
            }
            short if short.len() > 1 && short.starts_with('-') && !short.starts_with("--") => {
                if short.contains('c') {
                    return None;
                }
                out.push(arg.clone());
            }
            _ => out.push(arg.clone()),
        }
    }

    let script = resources.join("shell-integration/bash/ghostty.bash");
    if !script.is_file() {
        return None;
    }
    if let Some(previous) = env.get("ENV").cloned() {
        env.insert("GHOSTTY_BASH_ENV".into(), previous);
    }
    env.insert("ENV".into(), script.to_string_lossy().into_owned());
    env.insert("GHOSTTY_BASH_INJECT".into(), inject);
    if let Some(rcfile) = rcfile {
        env.insert("GHOSTTY_BASH_RCFILE".into(), rcfile);
    }
    // POSIX mode defaults HISTFILE to ~/.sh_history; put it back unless the user chose one.
    if !env.contains_key("HISTFILE") {
        if let Some(home) = env.get("HOME").filter(|h| !h.is_empty()) {
            let histfile = format!("{home}/.bash_history");
            env.insert("HISTFILE".into(), histfile);
            env.insert("GHOSTTY_BASH_UNEXPORT_HISTFILE".into(), "1".into());
        }
    }
    Some(out)
}

fn setup_zsh(resources: &Path, env: &mut Env) -> Option<()> {
    let dir = resources.join("shell-integration/zsh");
    if !dir.is_dir() {
        return None;
    }
    if let Some(previous) = env.get("ZDOTDIR").cloned() {
        env.insert("GHOSTTY_ZSH_ZDOTDIR".into(), previous);
    }
    env.insert("ZDOTDIR".into(), dir.to_string_lossy().into_owned());
    Some(())
}

/// Fish, elvish and nushell find their scripts through `XDG_DATA_DIRS`.
fn setup_xdg_data_dirs(resources: &Path, env: &mut Env) -> Option<()> {
    let dir = integration_dir(resources)?.to_string_lossy().into_owned();
    env.insert("GHOSTTY_SHELL_INTEGRATION_XDG_DIR".into(), dir.clone());
    let current = env
        .get("XDG_DATA_DIRS")
        .cloned()
        .unwrap_or_else(|| "/usr/local/share:/usr/share".to_string());
    let value = if current.is_empty() { dir } else { format!("{dir}:{current}") };
    env.insert("XDG_DATA_DIRS".into(), value);
    Some(())
}

fn setup_nushell(argv: &[String], resources: &Path, env: &mut Env) -> Option<Vec<String>> {
    setup_xdg_data_dirs(resources, env)?;
    let mut args = argv.iter();
    let mut out = vec![args.next()?.clone(), "--execute".to_string(), "use ghostty *".to_string()];
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--command" | "--lsp" => return None,
            "-" | "--" => {
                out.push(arg.clone());
                out.extend(args.by_ref().cloned());
            }
            short if short.len() > 1 && short.starts_with('-') && !short.starts_with("--") => {
                if short.contains('c') {
                    return None;
                }
                out.push(arg.clone());
            }
            _ => out.push(arg.clone()),
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resources(shell: Shell) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("shell-integration");
        let sub = match shell {
            Shell::Bash => "bash",
            Shell::Elvish => "elvish/lib",
            Shell::Fish => "fish/vendor_conf.d",
            Shell::Nushell => "nushell/vendor/autoload",
            Shell::Zsh => "zsh",
        };
        std::fs::create_dir_all(root.join(sub)).unwrap();
        if shell == Shell::Bash {
            std::fs::write(root.join("bash/ghostty.bash"), "").unwrap();
        }
        dir
    }

    fn argv(command: &str) -> Vec<String> {
        command.split(' ').map(str::to_string).collect()
    }

    #[test]
    fn detects_shells_by_executable_name() {
        assert_eq!(detect_shell("zsh"), Some(Shell::Zsh));
        assert_eq!(detect_shell("/opt/homebrew/bin/fish"), Some(Shell::Fish));
        assert_eq!(detect_shell("nu"), Some(Shell::Nushell));
        assert_eq!(detect_shell("elvish"), Some(Shell::Elvish));
        assert_eq!(detect_shell("sh"), None);
        assert_eq!(detect_shell("/usr/local/bin/bash"), Some(Shell::Bash));
        assert_eq!(detect_shell("/bin/bash") == Some(Shell::Bash), !cfg!(target_os = "macos"));
    }

    #[test]
    fn features_env_value_is_sorted_and_carries_cursor_blink() {
        let all = Features::parse("true");
        assert_eq!(
            all.env_value(true).as_deref(),
            Some("cursor:blink,path,ssh-env,ssh-terminfo,sudo,title")
        );
        assert_eq!(Features::parse("false").env_value(true), None);
        assert_eq!(Features::default().env_value(false).as_deref(), Some("cursor:steady,path,title"));
        let mixed = Features::parse("false,ssh-env,sudo");
        assert_eq!(mixed.env_value(true).as_deref(), Some("ssh-env,sudo"));
        assert_eq!(Features::parse("no-cursor,sudo").env_value(true).as_deref(), Some("path,sudo,title"));
    }

    #[test]
    fn mode_parses_config_values() {
        assert_eq!(Mode::parse("none"), Some(Mode::None));
        assert_eq!(Mode::parse("detect"), Some(Mode::Detect));
        assert_eq!(Mode::parse("zsh"), Some(Mode::Force(Shell::Zsh)));
        assert_eq!(Mode::parse("tcsh"), None);
    }

    #[test]
    fn zsh_points_zdotdir_at_the_resources_and_keeps_the_old_one() {
        let res = resources(Shell::Zsh);
        let mut env = Env::from([("ZDOTDIR".to_string(), "/home/me".to_string())]);
        let done = setup(argv("zsh -l"), res.path(), Mode::Detect, &mut env).unwrap();
        assert_eq!(done.shell, Shell::Zsh);
        assert_eq!(done.argv, argv("zsh -l"));
        assert_eq!(env["GHOSTTY_ZSH_ZDOTDIR"], "/home/me");
        assert_eq!(env["ZDOTDIR"], res.path().join("shell-integration/zsh").to_string_lossy());
    }

    #[test]
    fn fish_and_elvish_use_xdg_data_dirs() {
        for shell in [Shell::Fish, Shell::Elvish] {
            let res = resources(shell);
            let integration = res.path().join("shell-integration").to_string_lossy().into_owned();
            let mut env = Env::new();
            setup(argv("anything"), res.path(), Mode::Force(shell), &mut env).unwrap();
            assert_eq!(env["GHOSTTY_SHELL_INTEGRATION_XDG_DIR"], integration);
            assert_eq!(env["XDG_DATA_DIRS"], format!("{integration}:/usr/local/share:/usr/share"));

            let mut env = Env::from([("XDG_DATA_DIRS".to_string(), "/opt/share".to_string())]);
            setup(argv("anything"), res.path(), Mode::Force(shell), &mut env).unwrap();
            assert_eq!(env["XDG_DATA_DIRS"], format!("{integration}:/opt/share"));
        }
    }

    #[test]
    fn bash_runs_in_posix_mode_with_injected_flags() {
        let res = resources(Shell::Bash);
        let mut env = Env::from([("ENV".to_string(), "/old.sh".to_string()), ("HOME".to_string(), "/h".to_string())]);
        let done = setup(argv("bash --norc --noprofile -l"), res.path(), Mode::Detect, &mut env).unwrap();
        assert_eq!(done.argv, argv("bash --posix -l"));
        assert_eq!(env["GHOSTTY_BASH_INJECT"], "1 --norc --noprofile");
        assert_eq!(env["GHOSTTY_BASH_ENV"], "/old.sh");
        assert_eq!(env["ENV"], res.path().join("shell-integration/bash/ghostty.bash").to_string_lossy());
        assert_eq!(env["HISTFILE"], "/h/.bash_history");
        assert_eq!(env["GHOSTTY_BASH_UNEXPORT_HISTFILE"], "1");

        let mut env = Env::from([("HISTFILE".to_string(), "mine".to_string())]);
        setup(argv("bash"), res.path(), Mode::Detect, &mut env).unwrap();
        assert_eq!(env["HISTFILE"], "mine");
        assert!(!env.contains_key("GHOSTTY_BASH_UNEXPORT_HISTFILE"));

        let mut env = Env::new();
        let done = setup(argv("bash --rcfile profile.sh"), res.path(), Mode::Detect, &mut env).unwrap();
        assert_eq!(done.argv, argv("bash --posix"));
        assert_eq!(env["GHOSTTY_BASH_RCFILE"], "profile.sh");
    }

    #[test]
    fn bash_skips_unsupported_options_without_touching_the_environment() {
        let res = resources(Shell::Bash);
        for command in ["bash --posix", "bash -c script.sh", "bash -ic script.sh"] {
            let mut env = Env::new();
            assert_eq!(setup(argv(command), res.path(), Mode::Detect, &mut env), None, "{command}");
            assert!(env.is_empty(), "{command}");
        }
    }

    #[test]
    fn nushell_loads_the_module_and_rejects_non_interactive_use() {
        let res = resources(Shell::Nushell);
        let mut env = Env::new();
        let done = setup(argv("nu -l"), res.path(), Mode::Detect, &mut env).unwrap();
        assert_eq!(done.argv, vec!["nu", "--execute", "use ghostty *", "-l"]);
        assert!(env["XDG_DATA_DIRS"].starts_with(&*res.path().join("shell-integration").to_string_lossy()));

        for command in ["nu --command exit", "nu --lsp", "nu -c script.nu"] {
            let mut env = Env::new();
            assert_eq!(setup(argv(command), res.path(), Mode::Detect, &mut env), None, "{command}");
        }
    }

    #[test]
    fn missing_resources_and_unknown_shells_inject_nothing() {
        let mut env = Env::new();
        assert_eq!(setup(argv("zsh"), Path::new("/nonexistent"), Mode::Detect, &mut env), None);
        assert!(env.is_empty());
        let res = resources(Shell::Zsh);
        assert_eq!(setup(argv("sh"), res.path(), Mode::Detect, &mut env), None);
        assert_eq!(setup(argv("zsh"), res.path(), Mode::None, &mut env), None);
        assert!(env.is_empty());
    }
}
