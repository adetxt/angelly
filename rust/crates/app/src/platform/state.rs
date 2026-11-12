//! Small persistent switches owned by the platform layer: secure input, the chosen
//! app icon and the Shortcuts permission. They live in their own JSON file next to
//! the preferences so the two writers never clobber each other.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use angelly_core::config::Env;
use angelly_core::persist::data_dir;
use serde_json::{Map, Value};

const FILE_NAME: &str = "platform-state.json";

pub const SECURE_INPUT: &str = "SecureInput";
pub const SHORTCUTS_PERMISSION: &str = "ShortcutsPermission";

pub fn path() -> Option<PathBuf> {
    data_dir(&Env::from_process()).map(|dir| dir.join(FILE_NAME))
}

fn load(path: &Path) -> Map<String, Value> {
    match fs::read(path).ok().and_then(|data| serde_json::from_slice(&data).ok()) {
        Some(Value::Object(map)) => map,
        _ => Map::new(),
    }
}

fn save(path: &Path, map: &Map<String, Value>) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
    fs::create_dir_all(parent)?;
    let tmp = path.with_extension("json.tmp");
    let mut file = fs::File::create(&tmp)?;
    file.write_all(&serde_json::to_vec_pretty(map)?)?;
    file.sync_all()?;
    fs::rename(tmp, path)
}

pub fn get_bool(key: &str) -> Option<bool> {
    load(&path()?).get(key)?.as_bool()
}

pub fn set(key: &str, value: impl Into<Value>) {
    let Some(path) = path() else { return };
    let mut map = load(&path);
    map.insert(key.to_owned(), value.into());
    let _ = save(&path, &map);
}
