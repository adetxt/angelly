//! Freedesktop desktop entries: finding the applications that handle folders and building
//! the command line that opens a folder with one.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub const DIRECTORY_MIME: &str = "inode/directory";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopEntry {
    /// The desktop file id, e.g. `org.gnome.Nautilus.desktop`.
    pub id: String,
    pub path: PathBuf,
    pub name: String,
    pub exec: String,
    pub icon: Option<String>,
    pub mime_types: Vec<String>,
    pub terminal: bool,
    pub hidden: bool,
    only_show_in: Vec<String>,
    not_show_in: Vec<String>,
}

impl DesktopEntry {
    /// Parses the `[Desktop Entry]` group. `None` for anything that is not a launchable
    /// application.
    pub fn parse(id: &str, path: &Path, text: &str) -> Option<Self> {
        let mut in_group = false;
        let mut entry = DesktopEntry {
            id: id.to_owned(),
            path: path.to_owned(),
            name: String::new(),
            exec: String::new(),
            icon: None,
            mime_types: Vec::new(),
            terminal: false,
            hidden: false,
            only_show_in: Vec::new(),
            not_show_in: Vec::new(),
        };
        let mut application = false;
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with('[') {
                in_group = line == "[Desktop Entry]";
                continue;
            }
            let Some((key, value)) = line.split_once('=').filter(|_| in_group) else { continue };
            let (key, value) = (key.trim_end(), value.trim_start());
            match key {
                "Type" => application = value == "Application",
                "Name" => entry.name = unescape(value),
                "Exec" => entry.exec = value.to_owned(),
                "Icon" => entry.icon = Some(value.to_owned()),
                "MimeType" => entry.mime_types = list(value),
                "Terminal" => entry.terminal = value == "true",
                "NoDisplay" | "Hidden" => entry.hidden |= value == "true",
                "OnlyShowIn" => entry.only_show_in = list(value),
                "NotShowIn" => entry.not_show_in = list(value),
                _ => {}
            }
        }
        (application && !entry.name.is_empty() && !entry.exec.is_empty()).then_some(entry)
    }

    /// Whether the entry is meant to be shown in a desktop named in `$XDG_CURRENT_DESKTOP`.
    pub fn shown_in(&self, desktops: &[String]) -> bool {
        let listed = |names: &[String]| names.iter().any(|name| desktops.contains(name));
        (self.only_show_in.is_empty() || listed(&self.only_show_in)) && !listed(&self.not_show_in)
    }

    pub fn handles(&self, mime: &str) -> bool {
        self.mime_types.iter().any(|m| m == mime)
    }

    /// The program and arguments that open `target` with this application.
    pub fn command(&self, target: &Path) -> Option<Vec<String>> {
        expand_exec(&self.exec, target, self)
    }
}

fn list(value: &str) -> Vec<String> {
    value.split(';').filter(|item| !item.is_empty()).map(str::to_owned).collect()
}

fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('s') => out.push(' '),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

/// Splits an `Exec` value into arguments and expands its field codes. Double quotes group
/// words and a backslash escapes `"`, `` ` ``, `$` and `\` inside them.
fn expand_exec(exec: &str, target: &Path, entry: &DesktopEntry) -> Option<Vec<String>> {
    let mut args: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut started = false;
    let mut quoted = false;
    let mut has_target = false;
    let mut chars = exec.chars().peekable();
    let path = target.to_string_lossy();
    let url = url::Url::from_directory_path(target).map(String::from).unwrap_or_else(|_| format!("file://{path}"));

    let flush = |current: &mut String, started: &mut bool, args: &mut Vec<String>| {
        if *started {
            args.push(std::mem::take(current));
            *started = false;
        }
    };
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                quoted = !quoted;
                started = true;
            }
            '\\' if quoted => match chars.peek() {
                Some(&next) if matches!(next, '"' | '`' | '$' | '\\') => {
                    current.push(next);
                    chars.next();
                }
                _ => current.push('\\'),
            },
            ' ' | '\t' if !quoted => flush(&mut current, &mut started, &mut args),
            '%' => {
                started = true;
                match chars.next() {
                    Some('%') => current.push('%'),
                    Some('f' | 'F') => {
                        has_target = true;
                        current.push_str(&path);
                    }
                    Some('u' | 'U') => {
                        has_target = true;
                        current.push_str(&url);
                    }
                    Some('c') => current.push_str(&entry.name),
                    Some('k') => current.push_str(&entry.path.to_string_lossy()),
                    Some('i') => {
                        if let Some(icon) = &entry.icon {
                            flush(&mut current, &mut started, &mut args);
                            args.push("--icon".to_owned());
                            current.push_str(icon);
                            started = true;
                        }
                    }
                    _ => {}
                }
            }
            other => {
                current.push(other);
                started = true;
            }
        }
    }
    flush(&mut current, &mut started, &mut args);
    args.retain(|arg| !arg.is_empty());
    if args.is_empty() {
        return None;
    }
    if !has_target {
        args.push(path.into_owned());
    }
    Some(args)
}

/// `$XDG_DATA_HOME` (default `~/.local/share`) followed by `$XDG_DATA_DIRS` (default
/// `/usr/local/share:/usr/share`), each with `applications` appended.
pub fn application_dirs(data_home: Option<&Path>, home: Option<&Path>, data_dirs: Option<&str>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    match (data_home, home) {
        (Some(dir), _) if dir.is_absolute() => dirs.push(dir.to_owned()),
        (_, Some(home)) => dirs.push(home.join(".local/share")),
        _ => {}
    }
    let system = data_dirs.filter(|value| !value.is_empty()).unwrap_or("/usr/local/share:/usr/share");
    dirs.extend(system.split(':').filter(|d| !d.is_empty()).map(PathBuf::from));
    let mut seen = HashSet::new();
    dirs.retain(|dir| seen.insert(dir.clone()));
    dirs.into_iter().map(|dir| dir.join("applications")).collect()
}

/// Every desktop entry below `dirs`. The first occurrence of an id wins, so earlier
/// directories override later ones.
pub fn load_entries(dirs: &[PathBuf]) -> Vec<DesktopEntry> {
    let mut seen = HashSet::new();
    let mut entries = Vec::new();
    for dir in dirs {
        collect(dir, dir, &mut seen, &mut entries);
    }
    entries
}

fn collect(root: &Path, dir: &Path, seen: &mut HashSet<String>, entries: &mut Vec<DesktopEntry>) {
    let Ok(read) = std::fs::read_dir(dir) else { return };
    let mut paths: Vec<PathBuf> = read.flatten().map(|e| e.path()).collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            collect(root, &path, seen, entries);
            continue;
        }
        if path.extension().is_none_or(|e| e != "desktop") {
            continue;
        }
        let Ok(relative) = path.strip_prefix(root) else { continue };
        let id = relative.to_string_lossy().replace(std::path::MAIN_SEPARATOR, "-");
        if !seen.insert(id.clone()) {
            continue;
        }
        if let Some(entry) = std::fs::read_to_string(&path).ok().and_then(|text| DesktopEntry::parse(&id, &path, &text)) {
            entries.push(entry);
        }
    }
}

/// The desktop file id the user chose for `mime` in a `mimeapps.list`.
pub fn default_for(mime: &str, mimeapps: &str) -> Option<String> {
    let mut in_defaults = false;
    for line in mimeapps.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_defaults = line == "[Default Applications]";
            continue;
        }
        if let Some((key, value)) = line.split_once('=').filter(|_| in_defaults) {
            if key.trim() == mime {
                return value.split(';').find(|id| !id.is_empty()).map(str::to_owned);
            }
        }
    }
    None
}

/// The graphical applications that can open a folder: the default first, then the rest by name.
pub fn directory_handlers(entries: Vec<DesktopEntry>, desktops: &[String], default: Option<&str>) -> Vec<DesktopEntry> {
    let mut handlers: Vec<DesktopEntry> = entries
        .into_iter()
        .filter(|entry| entry.handles(DIRECTORY_MIME) && !entry.hidden && !entry.terminal && entry.shown_in(desktops))
        .collect();
    handlers.sort_by_key(|entry| (Some(entry.id.as_str()) != default, entry.name.to_lowercase()));
    handlers
}

#[cfg(test)]
mod tests {
    use super::*;

    const NAUTILUS: &str = "[Desktop Entry]\nType=Application\nName=Files\nName[de]=Dateien\nExec=nautilus --new-window %U\nIcon=org.gnome.Nautilus\nMimeType=inode/directory;application/x-gnome-saved-search;\nOnlyShowIn=GNOME;\n\n[Desktop Action new-window]\nName=New Window\nExec=nautilus --other\n";

    fn entry(text: &str) -> DesktopEntry {
        DesktopEntry::parse("a.desktop", Path::new("/usr/share/applications/a.desktop"), text).unwrap()
    }

    #[test]
    fn parses_the_main_group_only() {
        let e = entry(NAUTILUS);
        assert_eq!(e.name, "Files");
        assert_eq!(e.exec, "nautilus --new-window %U");
        assert!(e.handles(DIRECTORY_MIME));
        assert!(e.shown_in(&["GNOME".into()]));
        assert!(!e.shown_in(&["KDE".into()]));
        assert!(DesktopEntry::parse("x", Path::new("x"), "[Desktop Entry]\nType=Link\nName=x\nExec=y\n").is_none());
    }

    #[test]
    fn expands_field_codes_and_appends_a_missing_target() {
        let target = Path::new("/home/a/my project");
        assert_eq!(
            entry(NAUTILUS).command(target).unwrap(),
            ["nautilus", "--new-window", "file:///home/a/my%20project/"]
        );
        let plain = entry("[Desktop Entry]\nType=Application\nName=E\nExec=code\n");
        assert_eq!(plain.command(target).unwrap(), ["code", "/home/a/my project"]);
        let quoted = entry("[Desktop Entry]\nType=Application\nName=E\nExec=\"/opt/My App/run\" --x \"a b\" %f %%\n");
        assert_eq!(
            quoted.command(target).unwrap(),
            ["/opt/My App/run", "--x", "a b", "/home/a/my project", "%"]
        );
    }

    #[test]
    fn data_dirs_follow_the_xdg_defaults() {
        let dirs = application_dirs(None, Some(Path::new("/home/a")), None);
        assert_eq!(
            dirs,
            [
                PathBuf::from("/home/a/.local/share/applications"),
                PathBuf::from("/usr/local/share/applications"),
                PathBuf::from("/usr/share/applications"),
            ]
        );
    }

    #[test]
    fn handlers_put_the_default_first_and_skip_terminal_apps() {
        let make = |id: &str, name: &str, extra: &str| {
            DesktopEntry::parse(id, Path::new(id), &format!("[Desktop Entry]\nType=Application\nName={name}\nExec=x\nMimeType=inode/directory;\n{extra}")).unwrap()
        };
        let entries = vec![make("b.desktop", "Beta", ""), make("a.desktop", "Alpha", ""), make("t.desktop", "Term", "Terminal=true\n"), make("h.desktop", "Hidden", "NoDisplay=true\n")];
        let names: Vec<_> = directory_handlers(entries.clone(), &[], Some("b.desktop")).into_iter().map(|e| e.name).collect();
        assert_eq!(names, ["Beta", "Alpha"]);
        let mimeapps = "[Added Associations]\ninode/directory=x.desktop;\n[Default Applications]\ninode/directory=b.desktop;c.desktop;\n";
        assert_eq!(default_for(DIRECTORY_MIME, mimeapps).as_deref(), Some("b.desktop"));
    }

    #[test]
    fn earlier_directories_override_later_ones() {
        let (first, second) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let write = |dir: &Path, name: &str| {
            std::fs::write(dir.join("x.desktop"), format!("[Desktop Entry]\nType=Application\nName={name}\nExec=x\n")).unwrap()
        };
        write(first.path(), "First");
        write(second.path(), "Second");
        let entries = load_entries(&[first.path().to_owned(), second.path().to_owned()]);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "First");
    }
}
