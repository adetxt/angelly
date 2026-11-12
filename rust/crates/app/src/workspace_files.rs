//! File > "Save Workspace..." and "Open Workspace...": a window's tabs written to and read
//! from a `.angellyworkspace` file, which holds the JSON of one closed-window snapshot.
//! Terminals reopen as new sessions.

use std::fs;
use std::path::{Path, PathBuf};

use angelly_core::persist::{ClosedWindowRecoveryContext, ClosedWindowSnapshot};
use gpui_kit::*;

use crate::app_host::windows;
use crate::session;
use crate::workspace::Workspace;

const DEFAULT_NAME: &str = "Workspace.angellyworkspace";

pub fn save(cx: &mut App) {
    let Some(win) = windows(cx).into_iter().next() else { return };
    let Some(snapshot) = win.workspace.read(cx).snapshot(cx) else { return };
    let Ok(bytes) = serde_json::to_vec(&snapshot) else { return };
    let chosen = cx.prompt_for_new_path(&start_directory(), Some(DEFAULT_NAME));
    cx.spawn(async move |_| {
        if let Ok(Ok(Some(path))) = chosen.await {
            let _ = fs::write(path, bytes);
        }
    })
    .detach();
}

pub fn open(cx: &mut App) {
    let chosen = cx.prompt_for_paths(PathPromptOptions {
        files: true,
        directories: false,
        multiple: false,
        prompt: Some("Open".into()),
    });
    cx.spawn(async move |cx| {
        let Ok(Ok(Some(paths))) = chosen.await else { return };
        let Some(snapshot) = paths.first().and_then(|path| read(path)) else { return };
        let _ = cx.update(|cx| {
            let _ = session::open_window(None, cx, move |window, cx| {
                let mut context = ClosedWindowRecoveryContext::new();
                cx.new(|cx| Workspace::from_snapshot(&snapshot, Some(&mut context), window, cx))
            });
        });
    })
    .detach();
}

fn read(path: &Path) -> Option<ClosedWindowSnapshot> {
    let snapshot: ClosedWindowSnapshot = serde_json::from_slice(&fs::read(path).ok()?).ok()?;
    snapshot.validate().ok()?;
    Some(snapshot)
}

fn start_directory() -> PathBuf {
    std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from)
}
