use std::time::Duration;

use angelly_core::git::GitPullRequestStatus;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Sizable as _, StyledExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::panels::ui::{self, Look};
use crate::sidebar::icon;
use crate::workspace::{TabId, Workspace};

const WIDTH: f32 = 300.;
const GAP: f32 = 6.;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placement {
    Right,
    Bottom,
}

#[derive(Default)]
pub struct HoverCard {
    trigger: Option<TabId>,
    over_card: bool,
    popup: Option<AnyWindowHandle>,
    task: Option<Task<()>>,
    _keys: Option<Subscription>,
}

impl HoverCard {
    pub fn watch_keys(&mut self, window: &Window, cx: &mut Context<Workspace>) {
        let parent = window.window_handle();
        let workspace = cx.entity().downgrade();
        self._keys = Some(cx.intercept_keystrokes(move |event, window, cx| {
            let Some(owner) = workspace.upgrade() else {
                return;
            };
            let card = &owner.read(cx).chrome.hover_card;
            let here = window.window_handle();
            if (card.popup.is_some() || card.task.is_some())
                && (here == parent || (card.popup == Some(here) && event.keystroke.key == "escape"))
            {
                owner.update(cx, |this, cx| this.chrome.hover_card.dismiss(cx));
            }
        }));
    }

    pub fn dismiss(&mut self, cx: &mut App) {
        self.task = None;
        self.trigger = None;
        self.over_card = false;
        if let Some(popup) = self.popup.take() {
            popup.update(cx, |_, window, _| window.remove_window()).ok();
        }
    }
}

fn popup_bounds(anchor: Bounds<Pixels>, side: Placement, screen: Bounds<Pixels>) -> Bounds<Pixels> {
    let width = px(WIDTH);
    let height = px(96.);
    let origin = match side {
        Placement::Right => point(anchor.right() + px(GAP), anchor.top()),
        Placement::Bottom => point(anchor.left(), anchor.bottom() + px(GAP)),
    };
    let mut bounds = Bounds::new(origin, size(width, height));
    match side {
        Placement::Right => {
            if bounds.right() > screen.right() && anchor.left() - width - px(GAP) >= screen.left() {
                bounds.origin.x = anchor.left() - width - px(GAP);
            }
            bounds.origin.y = bounds
                .origin
                .y
                .min(screen.bottom() - height)
                .max(screen.top());
        }
        Placement::Bottom => {
            if bounds.bottom() > screen.bottom() && anchor.top() - height - px(GAP) >= screen.top()
            {
                bounds.origin.y = anchor.top() - height - px(GAP);
            }
            bounds.origin.x = bounds
                .origin
                .x
                .min(screen.right() - width)
                .max(screen.left());
        }
    }
    bounds
}

impl Workspace {
    pub fn hover_tab(
        &mut self,
        tab: TabId,
        side: Placement,
        anchor: Bounds<Pixels>,
        hovered: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !hovered {
            if self.chrome.hover_card.trigger == Some(tab) {
                self.chrome.hover_card.trigger = None;
                self.hide_tab_card_later(cx);
            }
            return;
        }
        if cx.has_active_drag() {
            return;
        }
        let card = &mut self.chrome.hover_card;
        if card.trigger == Some(tab) {
            return;
        }
        card.dismiss(cx);
        card.trigger = Some(tab);
        card.task = Some(cx.spawn_in(window, async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(400))
                .await;
            cx.update(|window, cx| {
                let Some(owner) = this.upgrade() else { return };
                if owner.read(cx).chrome.hover_card.trigger != Some(tab) || cx.has_active_drag() {
                    return;
                }
                let mut screen_anchor = anchor;
                screen_anchor.origin += window.bounds().origin;
                let screen = window
                    .display(cx)
                    .map(|display| display.bounds())
                    .unwrap_or(window.bounds());
                let bounds = popup_bounds(screen_anchor, side, screen);
                let workspace = owner.downgrade();
                let dark = Look::of(cx).dark;
                let options = WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: None,
                    kind: WindowKind::PopUp,
                    focus: false,
                    is_movable: false,
                    is_resizable: false,
                    is_minimizable: false,
                    display_id: window.display(cx).map(|display| display.id()),
                    window_background: if cfg!(target_os = "macos") {
                        WindowBackgroundAppearance::Transparent
                    } else {
                        WindowBackgroundAppearance::Blurred
                    },
                    ..Default::default()
                };
                match cx.open_window(options, |popup, cx| {
                    crate::platform::popup_surface::install(popup, window, dark);
                    let content = cx.new(|cx| TabPopover {
                        workspace: workspace.clone(),
                        tab,
                        _subscription: workspace
                            .upgrade()
                            .map(|workspace| cx.observe(&workspace, |_, _, cx| cx.notify())),
                    });
                    cx.new(|cx| {
                        gpui_kit::base::Root::new(content, popup, cx).bg(transparent_black())
                    })
                }) {
                    Ok(handle) => owner.update(cx, |this, _| {
                        this.chrome.hover_card.popup = Some(handle.into())
                    }),
                    Err(error) => eprintln!("Couldn't open tab preview: {error}"),
                }
            })
            .ok();
        }));
    }

    fn hide_tab_card_later(&mut self, cx: &mut Context<Self>) {
        self.chrome.hover_card.task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(200))
                .await;
            this.update(cx, |this, cx| {
                if this.chrome.hover_card.trigger.is_none() && !this.chrome.hover_card.over_card {
                    this.chrome.hover_card.dismiss(cx);
                }
            })
            .ok();
        }));
    }
}

struct TabPopover {
    workspace: WeakEntity<Workspace>,
    tab: TabId,
    _subscription: Option<Subscription>,
}

impl Render for TabPopover {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(workspace) = self.workspace.upgrade() else {
            return div().into_any_element();
        };
        let owner = workspace.read(cx);
        let Some(tab) = owner.tabs.iter().find(|tab| tab.id == self.tab) else {
            return div().into_any_element();
        };
        let title = owner.tab_title(tab, cx);
        let status = owner.chrome.git.status_for(self.tab);
        let look = Look::of(cx);
        let green = rgb(if look.dark { 0x65d98b } else { 0x147d37 });
        let red = rgb(if look.dark { 0xff8b86 } else { 0xb42318 });
        let heading = status
            .map(|s| s.titlebar_branch())
            .unwrap_or_else(|| title.clone());
        let has_title = status.is_some() && title != heading;
        let has_footer = status.is_some_and(|s| {
            s.pull_request != GitPullRequestStatus::NotApplicable
                || s.has_conflicts
                || s.line_changes.is_none()
        }) || owner.chrome.git.error_for(self.tab).is_some();
        let height = 38. + if has_title { 22. } else { 0. } + if has_footer { 28. } else { 0. };
        if window.viewport_size().height != px(height) {
            window.resize(size(px(WIDTH), px(height)));
        }
        let enter = self.workspace.clone();
        let mut card = div()
            .id("tab-preview")
            .role(Role::Group)
            .aria_label(format!(
                "Tab details: {}",
                status.map(|s| s.detail()).unwrap_or_else(|| title.clone())
            ))
            .v_flex()
            .size_full()
            .p(px(10.))
            .gap(px(6.))
            .font_family(ui::FONT)
            .text_size(px(ui::text::CALLOUT))
            .line_height(relative(ui::LINE_HEIGHT))
            .text_color(look.primary)
            .when(!cfg!(target_os = "macos"), |card| {
                card.bg(look.menu_surface())
                    .rounded(px(10.))
                    .border_1()
                    .border_color(look.separator)
            })
            .on_hover(move |hovered, _, cx| {
                enter
                    .update(cx, |owner, cx| {
                        owner.chrome.hover_card.over_card = *hovered;
                        if *hovered {
                            owner.chrome.hover_card.task = None;
                        } else {
                            owner.hide_tab_card_later(cx);
                        }
                    })
                    .ok();
            });
        let mut header = div()
            .h_flex()
            .items_center()
            .gap(px(6.))
            .when(status.is_some(), |header| {
                header.child(icon("git/branch.svg", 13., look.primary))
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .font_weight(FontWeight::MEDIUM)
                    .child(heading),
            );
        if let Some(changes) = status
            .and_then(|s| s.line_changes)
            .filter(|c| c.additions > 0 || c.deletions > 0)
        {
            header = header.child(
                div()
                    .h_flex()
                    .flex_none()
                    .gap(px(5.))
                    .child(
                        div()
                            .text_color(green)
                            .child(format!("+{}", changes.additions)),
                    )
                    .child(
                        div()
                            .text_color(red)
                            .child(format!("-{}", changes.deletions)),
                    ),
            );
        }
        card = card.child(header);
        if has_title {
            card = card.child(
                div()
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .child(title),
            );
        }
        if has_footer {
            let mut footer = div().h_flex().items_center().gap(px(6.));
            if let Some(status) = status {
                footer = match &status.pull_request {
                    GitPullRequestStatus::PullRequest { number, state, url } => {
                        let url = url.to_string();
                        footer.child(
                            Button::new("open-tab-pr")
                                .ghost()
                                .xsmall()
                                .label(format!("PR #{number} · {}", state.label()))
                                .on_click(move |_, _, cx| cx.open_url(&url)),
                        )
                    }
                    request => footer.child(match request {
                        GitPullRequestStatus::Loading => "Checking PR…",
                        GitPullRequestStatus::NoPullRequest => "No PR",
                        GitPullRequestStatus::Unavailable => "GitHub unavailable",
                        _ => "",
                    }),
                };
                if status.has_conflicts {
                    footer = footer.child(div().text_color(red).child("Merge conflicts"));
                } else if status.line_changes.is_none() {
                    footer = footer.child("Changes unavailable");
                }
                if status.pull_request_stale {
                    footer = footer.child("Last known status");
                }
            }
            if owner.chrome.git.error_for(self.tab).is_some() {
                footer = footer.child("Git refresh failed");
            }
            card = card.child(footer);
        }
        card.into_any_element()
    }
}

/// `ANGELLY_SELFTEST_TABS=bar,hover-check` verifies native popup geometry and dismissal.
pub fn selftest(window: &mut Window, cx: &mut Context<Workspace>) {
    cx.spawn_in(window, async move |this, cx| {
        cx.background_executor().timer(Duration::from_secs(1)).await;
        cx.update(|window, cx| {
            window.dispatch_event(
                PlatformInput::MouseMove(MouseMoveEvent {
                    position: point(px(100.), px(48.)),
                    pressed_button: None,
                    modifiers: Modifiers::default(),
                }),
                cx,
            );
        })
        .ok();
        cx.background_executor().timer(Duration::from_secs(1)).await;
        cx.update(|window, cx| {
            let owner = this.upgrade().unwrap();
            let workspace = owner.read(cx);
            let popup = workspace
                .chrome
                .hover_card
                .popup
                .expect("hover did not open a card");
            let vertical = workspace.sidebar_visible;
            let sidebar_width = workspace.sidebar_width;
            let parent = window.bounds();
            let bounds = popup.update(cx, |_, popup, _| popup.bounds()).unwrap();
            if vertical {
                assert!(
                    bounds.left() >= parent.left() + px(sidebar_width - 8.),
                    "card overlaps vertical tabs: {bounds:?}"
                );
            } else {
                assert!(
                    bounds.top()
                        >= parent.top()
                            + px(crate::workspace::TITLEBAR_HEIGHT + crate::tab_bar::BAR_HEIGHT),
                    "card overlaps horizontal tabs: {bounds:?}"
                );
            }
            assert!(
                bounds.size.height <= px(96.),
                "card is not compact: {bounds:?}"
            );
            window.dispatch_event(
                PlatformInput::MouseMove(MouseMoveEvent {
                    position: point(px(600.), px(300.)),
                    pressed_button: None,
                    modifiers: Modifiers::default(),
                }),
                cx,
            );
            popup
                .update(cx, |_, window, cx| {
                    window.dispatch_event(
                        PlatformInput::MouseMove(MouseMoveEvent {
                            position: point(px(10.), px(10.)),
                            pressed_button: None,
                            modifiers: Modifiers::default(),
                        }),
                        cx,
                    );
                })
                .unwrap();
        })
        .ok();
        cx.background_executor()
            .timer(Duration::from_millis(300))
            .await;
        cx.update(|window, cx| {
            assert!(
                this.upgrade()
                    .unwrap()
                    .read(cx)
                    .chrome
                    .hover_card
                    .popup
                    .is_some(),
                "entering the card dismissed it"
            );
            window.dispatch_event(
                PlatformInput::KeyDown(KeyDownEvent {
                    keystroke: Keystroke::parse("escape").unwrap(),
                    is_held: false,
                    prefer_character_input: false,
                }),
                cx,
            );
            assert!(
                this.upgrade()
                    .unwrap()
                    .read(cx)
                    .chrome
                    .hover_card
                    .popup
                    .is_none(),
                "Escape did not dismiss the card"
            );
            eprintln!(
                "[selftest] tab hover: placement, compact size, pointer entry and Escape passed"
            );
            cx.quit();
        })
        .ok();
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use super::{popup_bounds, Placement};
    use gpui_kit::{point, px, size, Bounds};
    use std::prelude::v1::test;

    #[test]
    fn previews_stay_outside_vertical_and_horizontal_tabs() {
        let screen = Bounds::new(point(px(0.), px(0.)), size(px(1440.), px(900.)));
        let tab = Bounds::new(point(px(16.), px(48.)), size(px(200.), px(32.)));
        let right = popup_bounds(tab, Placement::Right, screen);
        assert!(right.left() > tab.right());
        assert_eq!(right.top(), tab.top());
        let bottom = popup_bounds(tab, Placement::Bottom, screen);
        assert!(bottom.top() > tab.bottom());
        assert_eq!(bottom.left(), tab.left());
    }
}
