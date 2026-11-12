//! The titlebar: sidebar toggle and new-tab button, the working directory with its git
//! state, and the "Open With" button.

use std::path::PathBuf;

use angelly_core::git::{GitTabStatus, Indicator};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use crate::menu::{dropdown_menu, Menu};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Icon, IconName, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::component::StyledExt as _;
use gpui_kit::*;
use gpui_kit::base::ElementExt as _;

use crate::middle_text::MiddleText;
use crate::sidebar::{icon, Palette};
use crate::tab_colors::SystemColors;
use crate::theme::Theme;
use crate::window_chrome::Spec;
use crate::workspace::{Workspace, TITLEBAR_HEIGHT};

const CONTEXT_MIN_X: f32 = 120.0;
const APPLICATION_WIDTH: f32 = 52.0;
const APPLICATION_HEIGHT: f32 = 24.0;

impl Workspace {
    /// The directory the titlebar shows: the git checkout root, else the working directory.
    pub fn titlebar_directory(&self) -> Option<PathBuf> {
        let id = self.tabs.get(self.selected)?.id;
        self.chrome
            .git
            .status_for(id)
            .map(|status| status.directory.clone())
            .or_else(|| self.chrome.git.directory_for(id).map(PathBuf::from))
    }

    fn titlebar_status(&self) -> Option<GitTabStatus> {
        self.chrome.git.status_for(self.tabs.get(self.selected)?.id).cloned()
    }
}

pub fn render(
    workspace: &mut Workspace,
    spec: &Spec,
    sidebar_shown: bool,
    title_strip: Option<impl IntoElement>,
    reset_zoom: Option<impl IntoElement>,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    workspace.reload_open_with(cx);
    let palette = Palette::of(cx.global::<Theme>());
    let context_x = if sidebar_shown { workspace.sidebar_width + 12.0 } else { CONTEXT_MIN_X };
    let placement = crate::prefs::Prefs::read(cx).new_tab_placement();
    let new_tab_in_titlebar = placement.shows_in_titlebar(sidebar_shown, true);
    let available = f32::from(window.viewport_size().width) - 64.0 - context_x;

    let toggle = div()
        .id("sidebar-toggle")
        .role(Role::Button)
        .aria_label(if sidebar_shown { "Hide Tab Sidebar" } else { "Show Tab Sidebar" })
        .aria_keyshortcuts("Control+Command+S")
        .flex()
        .items_center()
        .justify_center()
        .size(px(20.))
        .mx(px(7.))
        .flex_none()
        .occlude()
        .child(icon("icons/panel-left.svg", 15., palette.secondary))
        .tooltip({
            let shown = sidebar_shown;
            move |window, cx| {
                Tooltip::new(if shown { "Hide Tab Sidebar" } else { "Show Tab Sidebar" }).build(window, cx)
            }
        })
        .on_click(cx.listener(|this, _, _, cx| this.set_sidebar_visible(!this.sidebar_visible, cx)));

    let toggle_shown = !spec.tabs_in_titlebar();
    let leading = div()
        .h_flex()
        .items_center()
        .h_full()
        .flex_none()
        .child(div().w(px(spec.leading_inset())).h_full().flex_none())
        .when(toggle_shown, |d| d.child(toggle))
        .when(new_tab_in_titlebar && toggle_shown, |d| d.child(div().mr(px(10.)).occlude().child(new_tab_button(workspace, cx))));

    let caption = crate::platform::window_controls::width(window);
    let context = context_view(workspace, palette, cx);
    let application = (available >= 24.0 && !spec.tabs_in_titlebar() && workspace.titlebar_directory().is_some())
        .then(|| open_with_button(workspace, palette, cx));

    div()
        .absolute()
        .top_0()
        .left_0()
        .right_0()
        .h(px(TITLEBAR_HEIGHT))
        .h_flex()
        .items_center()
        .child(leading)
        .child(crate::platform::window_controls::drag_area(
            div().flex_1().h_full().window_control_area(WindowControlArea::Drag),
            window,
        ))
        .when_some(title_strip, |bar, strip| {
            bar.child(div().absolute().top_0().left(px(spec.leading_inset())).right(px(12. + caption)).h_full().child(strip))
        })
        .when(available >= 24.0 && !spec.tabs_in_titlebar(), |bar| {
            bar.child(
                div()
                    .absolute()
                    .left(px(context_x))
                    .top_0()
                    .h_full()
                    .w(px(available.max(0.0)))
                    .h_flex()
                    .items_center()
                    .overflow_hidden()
                    .child(context),
            )
        })
        .child(
            div()
                .absolute()
                .right(px(12. + caption))
                .top(px((TITLEBAR_HEIGHT - APPLICATION_HEIGHT) / 2.0))
                .h_flex()
                .gap(px(8.))
                .items_center()
                .children(reset_zoom)
                .children(application),
        )
        .children(crate::platform::window_controls::render(window, palette, TITLEBAR_HEIGHT).map(|buttons| {
            div().absolute().top_0().right_0().h(px(TITLEBAR_HEIGHT)).child(buttons)
        }))
}

/// The "+" button and its menu: a new tab, and the worktree panels anchored below it.
pub fn new_tab_button(workspace: &Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    let weak = cx.entity().downgrade();
    let anchor = workspace.chrome.new_tab_bounds.clone();
    let capture = anchor.clone();
    let button = Button::new("new-tab")
        .ghost()
        .xsmall()
        .icon(Icon::new(IconName::Plus).size_3p5())
        .w(px(24.))
        .h(px(24.))
        .tooltip("New Tab or Worktree");
    let button = dropdown_menu(button, move |_, _| {
            let below = {
                let bounds = anchor.get();
                point(bounds.left(), bounds.bottom() + px(4.))
            };
            let (tab, create, manage) = (weak.clone(), weak.clone(), weak.clone());
            Menu::new().item("New Tab", move |window, cx| {
                tab.update(cx, |this, cx| this.new_tab_from_selected(window, cx)).ok();
            })
            .separator()
            .item("New Worktree…", move |window, cx| {
                create.update(cx, |this, cx| this.show_new_worktree(Some(below), window, cx)).ok();
            })
            .item("Manage Worktrees…", move |window, cx| {
                manage.update(cx, |this, cx| this.show_manage_worktrees(window, cx)).ok();
            })
        });
    div()
        .relative()
        .child(canvas(move |bounds, _, _| capture.set(bounds), |_, _, _, _| {}).absolute().inset_0())
        .child(button)
}

fn context_view(workspace: &Workspace, palette: Palette, cx: &Context<Workspace>) -> impl IntoElement {
    let status = workspace.titlebar_status();
    let directory = workspace.titlebar_directory();
    let name = directory
        .as_ref()
        .map(|d| d.file_name().map_or_else(|| d.to_string_lossy().into_owned(), |n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "Terminal".to_string());
    let tooltip = status
        .as_ref()
        .map(GitTabStatus::detail)
        .or_else(|| directory.as_ref().map(|d| d.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "Working directory unavailable".to_string());
    let dark = palette.dark;
    let tab = workspace.tabs.get(workspace.selected).map(|tab| tab.id);
    let popover_bounds = std::rc::Rc::new(std::cell::Cell::new(Bounds::<Pixels>::default()));
    let capture = popover_bounds.clone();

    let avatar = status.as_ref().and_then(|s| s.owner_avatar_url.as_ref()).map(|url| {
        match workspace.chrome.git.avatar(url) {
            Some(image) => img(image).size(px(16.)).flex_none().rounded(px(3.)).into_any_element(),
            None => icon("icons/folder.svg", 14., palette.secondary).into_any_element(),
        }
    });
    let indicator = status.as_ref().map(|status| {
        let (path, color) = match status.indicator() {
            Indicator::Standard => ("git/branch.svg", palette.secondary),
            Indicator::Open => ("git/pull-request.svg", SystemColors::green(dark)),
            Indicator::Draft => ("git/draft.svg", palette.primary),
            Indicator::Merged => ("git/merge.svg", SystemColors::purple(dark)),
            Indicator::Conflict => ("git/conflict.svg", SystemColors::red(dark)),
        };
        icon(path, 16., color)
    });
    let branch = status.as_ref().map(|s| format!("[{}]", s.titlebar_branch()));
    let changes = status
        .as_ref()
        .and_then(|s| s.line_changes)
        .filter(|c| c.additions != 0 || c.deletions != 0);

    div()
        .id("titlebar-context")
        .aria_description(tooltip)
        .h_flex()
        .items_center()
        .gap(px(8.))
        .min_w_0()
        .text_size(px(12.))
        .text_color(palette.primary)
        .whitespace_nowrap()
        .children(avatar)
        .child(div().min_w_0().child(MiddleText::new(name, 12., FontWeight::NORMAL, palette.primary)))
        .children(indicator)
        .children(branch.map(|b| div().text_color(palette.secondary).overflow_hidden().text_ellipsis().child(b)))
        .children(changes.map(|c| {
            div()
                .h_flex()
                .gap(px(6.))
                .flex_none()
                .child(div().text_color(SystemColors::green(dark)).child(format!("+{}", c.additions)))
                .child(div().text_color(SystemColors::red(dark)).child(format!("-{}", c.deletions)))
        }))
        .when(status.as_ref().is_some_and(|s| s.line_changes.is_none()), |context| {
            context.child(div().flex_none().text_color(palette.secondary).child("Changes unavailable"))
        })
        .on_prepaint(move |bounds, _, _| capture.set(bounds))
        .when_some(tab, |context, id| context.on_hover(cx.listener(move |this, hovered: &bool, window, cx| {
            this.hover_tab(id, crate::tab_popover::Placement::Bottom, popover_bounds.get(), *hovered, window, cx);
        })))
}

fn open_with_button(workspace: &Workspace, palette: Palette, cx: &mut Context<Workspace>) -> impl IntoElement {
    let (id, path) = crate::prefs::Prefs::read(cx).preferences().open_with_application();
    let selected = workspace.chrome.open_with.selected(id, path).cloned();
    let apps = workspace.chrome.open_with.apps.clone();
    let enabled = workspace.titlebar_directory().is_some();
    let weak = cx.entity().downgrade();
    let name = selected.as_ref().map_or("Application".to_string(), |app| app.name.clone());
    let application_icon = match selected.as_ref().and_then(|app| app.icon.clone()) {
        Some(image) => img(image).size(px(16.)).into_any_element(),
        None => icon("icons/square.svg", 14., palette.secondary).into_any_element(),
    };

    let primary = div()
        .id("open-with")
        .role(Role::Button)
        .aria_label(format!("Open in {name}"))
        .flex()
        .items_center()
        .justify_center()
        .w(px(28.))
        .h_full()
        .child(application_icon)
        .tooltip(move |window, cx| Tooltip::new(format!("Open in {name}")).build(window, cx))
        .when_some(selected.filter(|_| enabled), |d, app| {
            d.on_click(cx.listener(move |this, _, window, cx| this.open_in(&app, false, window, cx)))
        });

    let menu = Button::new("open-with-menu")
        .ghost()
        .w(px(22.))
        .h(px(APPLICATION_HEIGHT))
        .icon(Icon::new(IconName::ChevronDown).size_2p5())
        .tooltip("Open With");
    let menu = dropdown_menu(menu, move |_, _| {
            let menu = apps.iter().fold(Menu::new(), |menu, app| {
                let (workspace, target) = (weak.clone(), app.clone());
                menu.image_item(
                    app.name.clone(), app.icon.clone(), move |window, cx| {
                        workspace.update(cx, |this, cx| this.open_in(&target, true, window, cx)).ok();
                    },
                )
            });
            let (other, copy) = (weak.clone(), weak.clone());
            menu.separator()
                .item("Other Application…", move |window, cx| {
                    other.update(cx, |this, cx| this.choose_application(window, cx)).ok();
                })
                .separator()
                .item("Copy Path", move |_, cx| {
                    copy.update(cx, |this, cx| this.copy_directory_path(cx)).ok();
                })
        });

    div()
        .h_flex()
        .items_center()
        .w(px(APPLICATION_WIDTH))
        .h(px(APPLICATION_HEIGHT))
        .rounded(px(5.))
        .border_1()
        .border_color(palette.secondary.opacity(0.4))
        .occlude()
        .child(primary)
        .child(div().w(px(1.)).h_full().flex_none().bg(palette.secondary.opacity(0.4)))
        .child(menu)
}
