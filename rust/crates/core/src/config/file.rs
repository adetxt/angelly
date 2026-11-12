use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Reads and edits `key = value` lines of a Ghostty config file in place.
///
/// Edits only ever touch the one line for the key being changed, so comments,
/// ordering and every other key in the file are left exactly as written.
#[derive(Debug, Clone)]
pub struct ConfigFile {
    path: PathBuf,
}

/// Repeatable keys where the first line is the primary value and later lines are
/// fallbacks. For every other key the last line wins.
const FIRST_LINE_WINS: &[&str] = &["font-family"];

impl ConfigFile {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The value of every key the file sets. Keys set to an empty value (which resets
    /// them to the default) are omitted.
    pub fn values(&self) -> HashMap<String, String> {
        let lines = self.read_lines();
        let keys: HashSet<String> = lines.iter().filter_map(|l| parse(l)).map(|(k, _)| k).collect();
        let mut result = HashMap::new();
        for key in keys {
            let Some(index) = effective_line(&key, &lines) else { continue };
            if let Some((_, value)) = parse(&lines[index]) {
                if !value.is_empty() {
                    result.insert(key, value);
                }
            }
        }
        result
    }

    /// Sets a key to a value, or removes the line when `value` is `None` so the key
    /// falls back to its default. The effective line is rewritten in place if it
    /// exists, otherwise a new line is appended.
    pub fn set(&self, key: &str, value: Option<&str>) -> std::io::Result<()> {
        let mut lines = self.read_lines();
        let new_line = value.map(|v| format!("{key} = {v}"));

        match (effective_line(key, &lines), new_line) {
            (Some(index), Some(line)) => lines[index] = line,
            (Some(index), None) => {
                lines.remove(index);
            }
            (None, Some(line)) => {
                if lines.last().map(String::as_str) == Some("") {
                    lines.pop();
                }
                lines.push(line);
                lines.push(String::new());
            }
            (None, None) => return Ok(()),
        }

        let target = std::fs::canonicalize(&self.path).unwrap_or_else(|_| self.path.clone());
        write_atomically(&target, lines.join("\n").as_bytes())
    }

    fn read_lines(&self) -> Vec<String> {
        match std::fs::read_to_string(&self.path) {
            Ok(contents) => contents.split('\n').map(str::to_string).collect(),
            Err(_) => Vec::new(),
        }
    }
}

fn write_atomically(target: &Path, data: &[u8]) -> std::io::Result<()> {
    let dir = target.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let name = target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = dir.join(format!(".{name}.{}.tmp", uuid::Uuid::new_v4().simple()));
    let result = (|| {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(data)?;
        file.sync_all()?;
        if let Ok(meta) = std::fs::metadata(target) {
            let _ = std::fs::set_permissions(&tmp, meta.permissions());
        }
        std::fs::rename(&tmp, target)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// The index of the line that determines the key's value, if any.
fn effective_line(key: &str, lines: &[String]) -> Option<usize> {
    let mut result = None;
    for (index, line) in lines.iter().enumerate() {
        let Some((k, value)) = parse(line) else { continue };
        if k != key {
            continue;
        }
        if FIRST_LINE_WINS.contains(&key) {
            if value.is_empty() {
                result = None;
            } else if result.is_none() {
                result = Some(index);
            }
        } else {
            result = Some(index);
        }
    }
    result
}

/// Parses a line the way Ghostty's config loader does: whole-line `#` comments,
/// whitespace around `=`, and optional surrounding quotes.
fn parse(line: &str) -> Option<(String, String)> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let eq = line.find('=')?;
    let key = line[..eq].trim_matches(|c: char| c == ' ' || c == '\t');
    let mut value = line[eq + 1..].trim_matches(|c: char| c == ' ' || c == '\t');
    if value.chars().count() >= 2 && value.starts_with('"') && value.ends_with('"') {
        value = &value[1..value.len() - 1];
    }
    Some((key.to_string(), value.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        dir: tempfile::TempDir,
    }

    impl Fixture {
        fn new() -> Self {
            Self { dir: tempfile::tempdir().unwrap() }
        }

        fn file(&self, contents: Option<&str>) -> ConfigFile {
            let path = self.dir.path().join("config.ghostty");
            if let Some(c) = contents {
                std::fs::write(&path, c).unwrap();
            }
            ConfigFile::new(path)
        }
    }

    fn contents(file: &ConfigFile) -> String {
        std::fs::read_to_string(file.path()).unwrap()
    }

    fn map(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn reads_last_value_ignoring_comments_and_quotes() {
        let f = Fixture::new();
        let file = f.file(Some(
            "# font-size = 99\nfont-size = 12\n  font-size   =   14\ntheme = \"Builtin Dark\"\ncursor-style =",
        ));
        assert_eq!(file.values(), map(&[("font-size", "14"), ("theme", "Builtin Dark")]));
    }

    #[test]
    fn font_family_uses_first_line_after_reset() {
        let f = Fixture::new();
        let file = f.file(Some("font-family = Old\nfont-family =\nfont-family = Primary\nfont-family = Fallback"));
        assert_eq!(file.values().get("font-family").map(String::as_str), Some("Primary"));

        file.set("font-family", Some("New")).unwrap();
        assert_eq!(contents(&file), "font-family = Old\nfont-family =\nfont-family = New\nfont-family = Fallback");
    }

    #[test]
    fn rewrites_effective_line_in_place() {
        let f = Fixture::new();
        let file = f.file(Some("# my config\nfont-size = 12\ntheme = Dark\n\nfont-size = 14\n"));
        file.set("font-size", Some("16")).unwrap();
        assert_eq!(contents(&file), "# my config\nfont-size = 12\ntheme = Dark\n\nfont-size = 16\n");
    }

    #[test]
    fn appends_missing_key_and_removes_on_reset() {
        let f = Fixture::new();
        let file = f.file(Some("theme = Dark"));
        file.set("font-size", Some("13")).unwrap();
        assert_eq!(contents(&file), "theme = Dark\nfont-size = 13\n");
        file.set("theme", None).unwrap();
        assert_eq!(contents(&file), "font-size = 13\n");
    }

    #[test]
    fn creates_missing_file() {
        let f = Fixture::new();
        let file = f.file(None);
        file.set("font-size", Some("13")).unwrap();
        assert_eq!(contents(&file), "font-size = 13\n");
    }

    #[test]
    fn removing_an_absent_key_does_not_create_the_file() {
        let f = Fixture::new();
        let file = f.file(None);
        file.set("font-size", None).unwrap();
        assert!(!file.path().exists());
    }

    #[cfg(unix)]
    #[test]
    fn writes_through_symlink() {
        let f = Fixture::new();
        let target = f.file(Some("theme = Dark\n"));
        let link = f.dir.path().join("link.ghostty");
        std::os::unix::fs::symlink(target.path(), &link).unwrap();

        ConfigFile::new(&link).set("theme", Some("Light")).unwrap();
        assert_eq!(std::fs::read_link(&link).unwrap(), target.path());
        assert_eq!(contents(&target), "theme = Light\n");
    }

    #[test]
    fn preserves_unknown_lines_and_comments() {
        let f = Fixture::new();
        let original = "# header\n\nkeybind = super+d=new_split:right\nunknown-key = 1\n  # indented comment\nfont-size = 12\n";
        let file = f.file(Some(original));
        file.set("font-size", Some("14")).unwrap();
        assert_eq!(contents(&file), original.replace("12", "14"));
    }
}
