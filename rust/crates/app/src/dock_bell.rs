//! Dock feedback for bells: the badge counts the terminals that rang while out of sight and
//! a bell bounces the icon once when the app is in the background and `bell-features`
//! includes `attention`.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use gpui_kit::*;

use crate::app_host::windows;
use crate::platform::app_icon;
use crate::terminal_view::settings::parse_bell_features;
use crate::terminal_view::{TerminalEvent, TerminalView};
use crate::theme::Theme;

const POLL: Duration = Duration::from_millis(500);

/// The count last shown, so the Dock tile is only touched when it changes.
static SHOWN: AtomicUsize = AtomicUsize::new(0);

pub fn install(cx: &mut App) {
    cx.observe_new(|_: &mut TerminalView, _, cx: &mut Context<TerminalView>| {
        let view = cx.entity();
        cx.subscribe(&view, |_, _, event: &TerminalEvent, cx| {
            if *event == TerminalEvent::Bell {
                // The ringing view is leased while this runs, and the badge reads every view.
                cx.defer(rang);
            }
        })
        .detach();
    })
    .detach();
    // Bells are cleared by interaction, which raises no event, so the badge follows by polling.
    cx.spawn(async move |cx| loop {
        cx.background_executor().timer(POLL).await;
        cx.update(refresh);
    })
    .detach();
}

fn rang(cx: &mut App) {
    let features = parse_bell_features(cx.global::<Theme>().config.get("bell-features"));
    if features.attention && cx.active_window().is_none() {
        app_icon::request_attention();
    }
    refresh(cx);
}

fn refresh(cx: &mut App) {
    let ringing = windows(cx)
        .iter()
        .flat_map(|win| win.workspace.read(cx).tabs.iter().flat_map(|tab| tab.splits.panes()))
        .filter(|pane| pane.read(cx).has_bell())
        .count();
    if SHOWN.swap(ringing, Ordering::Relaxed) != ringing {
        app_icon::set_badge(ringing);
    }
}
