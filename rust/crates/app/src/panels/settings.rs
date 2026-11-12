//! The Settings page: every control writes one line of the config file (or
//! one preference) and the app reloads its theme afterwards.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use angelly_core::config::{ConfigFile, Env};
use angelly_core::persist::HorizontalNewTabButtonPlacement;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Escape;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::StyledExt as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::assets::{app_icon, ICON_VARIANTS};
use super::ui::{self, icon, text, Look, Variant};
use super::check_for_updates;
use super::worktree::middle_truncate;
use super::reload_theme;
use crate::prefs::Prefs;
use super::confirm::Dismissed;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Category {
    Font,
    Appearance,
    Cursor,
    Window,
    Behavior,
    Keyboard,
    Worktrees,
    Updates,
}

impl Category {
    const SECTIONS: [(&'static str, &'static [Category]); 4] = [
        ("Appearance", &[Category::Appearance, Category::Font, Category::Cursor]),
        ("Terminal", &[Category::Behavior, Category::Keyboard]),
        ("Workspace", &[Category::Window, Category::Worktrees]),
        ("Application", &[Category::Updates]),
    ];

    pub const ALL: [Category; 8] = [
        Category::Font,
        Category::Appearance,
        Category::Cursor,
        Category::Window,
        Category::Behavior,
        Category::Keyboard,
        Category::Worktrees,
        Category::Updates,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Category::Font => "Font",
            Category::Appearance => "Appearance",
            Category::Cursor => "Cursor",
            Category::Window => "Window",
            Category::Behavior => "Behavior",
            Category::Keyboard => "Keyboard",
            Category::Worktrees => "Worktrees",
            Category::Updates => "Updates",
        }
    }

    fn icon(self) -> IconName {
        match self {
            Category::Font => IconName::Type,
            Category::Appearance => IconName::Palette,
            Category::Cursor => IconName::MousePointer2,
            Category::Window => IconName::AppWindow,
            Category::Behavior => IconName::SlidersHorizontal,
            Category::Keyboard => IconName::Keyboard,
            Category::Worktrees => IconName::GitBranch,
            Category::Updates => IconName::CircleArrowDown,
        }
    }

    fn subtitle(self) -> &'static str {
        match self {
            Category::Font => "Choose the typeface and text size for your terminals.",
            Category::Appearance => "Customize the colors and appearance of Angelly.",
            Category::Cursor => "Customize the shape and blinking of the terminal cursor.",
            Category::Window => "Set up window layout, tabs, and session restoration.",
            Category::Behavior => "Choose how your terminals handle selection, pasting, and new sessions.",
            Category::Keyboard => "Configure how modifier keys work in the terminal.",
            Category::Worktrees => "Choose where new Git worktrees are created.",
            Category::Updates => "Manage how Angelly checks for new versions.",
        }
    }
}

/// Turns config enum values like `block_hollow` into labels like "Block hollow".
pub fn enum_label(value: &str) -> String {
    match value {
        "true" => "On".to_string(),
        "false" => "Off".to_string(),
        other => {
            let words = other.replace('_', " ");
            let mut chars = words.chars();
            chars.next().map(|first| first.to_uppercase().chain(chars).collect()).unwrap_or_default()
        }
    }
}

type Labeler = fn(&str) -> String;

fn label_icon(value: &str) -> String {
    match value {
        "xray" => "X-Ray".to_string(),
        "attack-on-titan" => "Attack on Titan".to_string(),
        _ => enum_label(value),
    }
}

fn label_restore(value: &str) -> String {
    if value == "default" { "Follow macOS".to_string() } else { enum_label(value) }
}

fn label_confirm(value: &str) -> String {
    match value {
        "true" => "If a process is running".to_string(),
        "false" => "Never".to_string(),
        "always" => "Always".to_string(),
        other => other.to_string(),
    }
}

fn label_tab_position(value: &str) -> String {
    if value == "end" { "At the end" } else { "Next to current tab" }.to_string()
}

fn label_option_alt(value: &str) -> String {
    match value {
        "left" => "Left Option only".to_string(),
        "right" => "Right Option only".to_string(),
        other => enum_label(other),
    }
}

fn label_updates(value: &str) -> String {
    if value == "download" { "Check and download".to_string() } else { enum_label(value) }
}

fn label_raw(value: &str) -> String {
    value.to_string()
}

const ON_OFF: &[&str] = &["true", "false"];

/// A control in the form.
#[derive(Clone)]
pub enum Row {
    Picker { title: &'static str, subtitle: &'static str, key: &'static str, options: Vec<String>, label: Labeler },
    Text { title: &'static str, subtitle: &'static str, key: &'static str },
    AppIcon,
    QuotaToggle,
    NewTabPlacement,
    WorktreeFolder,
    CheckForUpdates,
}

fn picker(title: &'static str, subtitle: &'static str, key: &'static str, options: &[&str], label: Labeler) -> Row {
    Row::Picker { title, subtitle, key, options: options.iter().map(|s| s.to_string()).collect(), label }
}

/// Rows of a category, with the dynamic option lists filled in.
pub fn rows(category: Category, fonts: &[String], themes: &[String]) -> Vec<Row> {
    match category {
        Category::Font => vec![
            Row::Picker { title: "Font family", subtitle: "The monospace typeface used to display terminal text.", key: "font-family", options: fonts.to_vec(), label: label_raw },
            Row::Text { title: "Font size", subtitle: "The terminal text size, in points.", key: "font-size" },
        ],
        Category::Appearance => vec![
            Row::AppIcon,
            Row::Picker { title: "Theme", subtitle: "The color palette used for terminal text and backgrounds.", key: "theme", options: themes.to_vec(), label: label_raw },
            picker("Window theme", "Choose whether the window follows the system appearance or terminal theme.", "window-theme", &["auto", "system", "light", "dark", "ghostty"], enum_label),
            Row::Text { title: "Background opacity", subtitle: "Window background opacity, from 0 (transparent) to 1 (opaque).", key: "background-opacity" },
            picker("Background blur", "Blur what is behind a transparent window background.", "background-blur", ON_OFF, enum_label),
            Row::QuotaToggle,
        ],
        Category::Cursor => vec![
            picker("Cursor style", "The shape of the cursor at the terminal's insertion point.", "cursor-style", &["block", "bar", "underline", "block_hollow"], enum_label),
            picker("Cursor blink", "Make the cursor blink while the terminal is focused.", "cursor-style-blink", ON_OFF, enum_label),
        ],
        Category::Window => vec![
            Row::Text { title: "Horizontal padding", subtitle: "Space between terminal text and the left and right window edges.", key: "window-padding-x" },
            Row::Text { title: "Vertical padding", subtitle: "Space above and below the terminal text.", key: "window-padding-y" },
            picker("Titlebar style", "Choose how the window titlebar and tabs are displayed.", "macos-titlebar-style", &["native", "transparent", "tabs", "hidden"], enum_label),
            Row::NewTabPlacement,
            picker("Restore windows", "Choose whether to reopen your windows when Angelly starts.", "window-save-state", &["always", "never", "default"], label_restore),
        ],
        Category::Behavior => vec![
            picker("Copy on select", "Automatically copy selected terminal text to the clipboard.", "copy-on-select", ON_OFF, enum_label),
            picker("Confirm before closing", "Choose when to ask before closing a terminal session.", "confirm-close-surface", &["true", "false", "always"], label_confirm),
            picker("New tab position", "Open new tabs beside the current tab or at the end of the list.", "window-new-tab-position", &["current", "end"], label_tab_position),
            picker("Quit after last window closes", "Exit Angelly when there are no open windows.", "quit-after-last-window-closed", ON_OFF, enum_label),
            picker("Hide mouse while typing", "Hide the mouse pointer while entering terminal text.", "mouse-hide-while-typing", ON_OFF, enum_label),
            picker("Paste protection", "Ask for confirmation before pasting potentially unsafe content.", "clipboard-paste-protection", ON_OFF, enum_label),
            picker("New windows keep working directory", "Start new windows in the current terminal's working directory.", "window-inherit-working-directory", ON_OFF, enum_label),
        ],
        Category::Keyboard => vec![picker("Option as Alt", "Choose which Option keys send Alt key sequences to terminal programs.", "macos-option-as-alt", &["true", "false", "left", "right"], label_option_alt)],
        Category::Worktrees => vec![Row::WorktreeFolder],
        Category::Updates => vec![
            picker("Automatic updates", "Choose whether Angelly checks for updates and downloads them automatically.", "auto-update", &["off", "check", "download"], label_updates),
            Row::CheckForUpdates,
        ],
    }
}

/// The choices a picker offers: a value in the file that is not one of the
/// options is kept as its own choice so opening settings never changes it.
pub fn picker_choices(current: Option<&str>, options: &[String]) -> Vec<String> {
    match current {
        Some(value) if !options.iter().any(|o| o == value) => {
            let mut choices = vec![value.to_string()];
            choices.extend(options.iter().cloned());
            choices
        }
        _ => options.to_vec(),
    }
}

/// Theme names from the user's themes directory and the app's bundled themes.
pub fn theme_names(env: &Env) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for dir in [env.user_themes_dir(), env.resource_themes_dir()].into_iter().flatten() {
        if let Ok(entries) = std::fs::read_dir(dir) {
            names.extend(entries.filter_map(Result::ok).map(|e| e.file_name().to_string_lossy().into_owned()));
        }
    }
    names.extend(["Angelly Dark".to_string(), "Angelly Light".to_string()]);
    names.retain(|n| !n.starts_with('.'));
    names.sort_by_key(|n| n.to_lowercase());
    names.dedup();
    names
}

/// The config file's values and the last write error.
pub struct SettingsModel {
    file: ConfigFile,
    pub values: HashMap<String, String>,
    pub write_error: Option<String>,
}

impl SettingsModel {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        let file = ConfigFile::new(path);
        let values = file.values();
        Self { file, values, write_error: None }
    }

    pub fn path(&self) -> &Path {
        self.file.path()
    }

    pub fn reload(&mut self) {
        self.values = self.file.values();
    }

    /// Sets a key, or resets it to the default with `None`. Returns whether the
    /// file changed.
    pub fn set(&mut self, key: &str, value: Option<&str>) -> bool {
        if self.values.get(key).map(String::as_str) == value {
            return false;
        }
        match self.file.set(key, value) {
            Ok(()) => self.write_error = None,
            Err(error) => {
                self.write_error = Some(format!("Couldn't save settings: {error}"));
                return false;
            }
        }
        self.reload();
        true
    }
}

/// The config file Settings edits: `$ANGELLY_CONFIG` when set, otherwise the
/// preferred location.
pub fn config_path(env: &Env) -> Option<PathBuf> {
    std::env::var_os("ANGELLY_CONFIG").map(PathBuf::from).or_else(|| env.preferred_config_path())
}

fn monospace_families(cx: &App) -> Vec<String> {
    let system = cx.text_system();
    let mut families: Vec<String> = system
        .all_font_names()
        .into_iter()
        .filter(|name| {
            let id = system.resolve_font(&font(name.clone()));
            let advance = |c: char| system.advance(id, px(16.), c).map(|s| f32::from(s.width)).unwrap_or(0.0);
            advance('i') > 0.0 && (advance('i') - advance('M')).abs() < 0.01
        })
        .collect();
    families.sort_by_key(|n| n.to_lowercase());
    families.dedup();
    families
}

pub struct SettingsView {
    category: Category,
    model: SettingsModel,
    fonts: Vec<String>,
    themes: Vec<String>,
    inputs: HashMap<&'static str, Entity<InputState>>,
    open_picker: Option<&'static str>,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<Dismissed> for SettingsView {}

impl SettingsView {
    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
    }

    fn back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let edited = self.inputs.iter().find_map(|(&key, input)| {
            let input = input.read(cx);
            input.focus_handle(cx).is_focused(window).then(|| (key, input.value().trim().to_string()))
        });
        if let Some((key, value)) = edited {
            self.set(key, (!value.is_empty()).then_some(value.as_str()), cx);
            if self.model.write_error.is_some() { return; }
        }
        self.focus(window, cx);
        cx.emit(Dismissed);
    }

    fn escape(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.open_picker.take().is_some() {
            cx.notify();
        } else {
            self.back(window, cx);
        }
    }

    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let env = Env::from_process();
        let path = config_path(&env).unwrap_or_else(|| PathBuf::from("config.ghostty"));
        let focus = cx.focus_handle();
        let subscription = cx.observe_window_activation(window, |this, window, cx| {
            if window.is_window_active() {
                this.model.reload();
                this.sync_inputs(window, cx);
                cx.notify();
            }
        });
        Self {
            category: Category::Font,
            model: SettingsModel::new(path),
            fonts: monospace_families(cx),
            themes: theme_names(&env),
            inputs: HashMap::new(),
            open_picker: None,
            focus,
            _subscriptions: vec![subscription],
        }
    }

    pub fn select_category(&mut self, category: Category, window: &mut Window, cx: &mut Context<Self>) {
        self.category = category;
        self.open_picker = None;
        self.ensure_inputs(window, cx);
        cx.notify();
    }

    fn ensure_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for row in rows(self.category, &self.fonts, &self.themes) {
            let Row::Text { key, .. } = row else { continue };
            if self.inputs.contains_key(key) {
                continue;
            }
            let value = self.model.values.get(key).cloned().unwrap_or_default();
            let input = ui::new_input(window, cx, "Default", &value);
            self._subscriptions.push(cx.subscribe_in(&input, window, move |this, input, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                    let draft = input.read(cx).value().trim().to_string();
                    this.set(key, (!draft.is_empty()).then_some(draft.as_str()), cx);
                }
            }));
            self.inputs.insert(key, input);
        }
    }

    /// Refreshes text fields that are not being edited from the file.
    fn sync_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for (key, input) in &self.inputs {
            if input.read(cx).focus_handle(cx).is_focused(window) {
                continue;
            }
            let value = self.model.values.get(*key).cloned().unwrap_or_default();
            if input.read(cx).value().as_ref() != value {
                input.update(cx, |state, cx| state.set_value(value, window, cx));
            }
        }
    }

    pub fn set(&mut self, key: &str, value: Option<&str>, cx: &mut Context<Self>) {
        if self.model.set(key, value) {
            reload_theme(cx);
        }
        cx.notify();
    }

    fn open_config_file(&self, cx: &mut Context<Self>) {
        let path = self.model.path().to_path_buf();
        if !path.exists() {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(&path, "");
        }
        cx.open_with_system(&path);
    }

    fn picker_row(
        &self,
        title: &'static str,
        subtitle: &'static str,
        key: &'static str,
        options: &[String],
        label: Labeler,
        look: &Look,
        cx: &mut Context<Self>,
    ) -> Div {
        let current = self.model.values.get(key).cloned();
        let face_label = current.as_deref().map(label).unwrap_or_else(|| "Default".to_string());
        let mut holder = div().relative().child(ui::popup_face(ui::named("picker", key), face_label, look).on_click(cx.listener(
            move |this, _, _, cx| {
                this.open_picker = if this.open_picker == Some(key) { None } else { Some(key) };
                cx.notify();
            },
        )));
        if self.open_picker == Some(key) {
            let default_item = ui::menu_item(ui::named("choice-default", key), "Default", current.is_none(), look).on_click(
                cx.listener(move |this, _, _, cx| {
                    this.open_picker = None;
                    this.set(key, None, cx);
                }),
            );
            let mut items = Vec::new();
            for (index, choice) in picker_choices(current.as_deref(), options).into_iter().enumerate() {
                let selected = current.as_deref() == Some(choice.as_str());
                let shown = label(&choice);
                items.push(ui::menu_item(("choice", index), shown, selected, look).on_click(cx.listener(move |this, _, _, cx| {
                    this.open_picker = None;
                    this.set(key, Some(choice.as_str()), cx);
                })));
            }
            holder = holder.child(deferred(
                anchored()
                    .anchor(Anchor::TopRight)
                    .offset(point(px(180.), px(24.)))
                    .snap_to_window_with_margin(px(8.))
                    .child(
                        ui::menu(look)
                            .id(ui::named("menu", key))
                            .max_h(px(260.))
                            .overflow_y_scroll()
                            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                                this.open_picker = None;
                                cx.notify();
                            }))
                            .child(default_item)
                            .child(div().h(px(1.)).mx(px(6.)).my(px(3.)).bg(look.separator))
                            .children(items),
                    ),
            ));
        }
        form_row(title, subtitle, look, holder)
    }

    fn row(&self, row: &Row, look: &Look, cx: &mut Context<Self>) -> Div {
        match row {
            Row::Picker { title, subtitle, key, options, label } => self.picker_row(title, subtitle, key, options, *label, look, cx),
            Row::Text { title, subtitle, key } => {
                let field = self.inputs.get(key).map_or_else(div, |input| div().w(px(180.)).child(ui::field(input)));
                form_row(title, subtitle, look, field)
            }
            Row::AppIcon => {
                let value = self.model.values.get("macos-icon").cloned().unwrap_or_else(|| "official".to_string());
                let options: Vec<String> = ICON_VARIANTS.iter().map(|s| s.to_string()).collect();
                let preview = app_icon(&value).map(|image| img(image).size(px(64.)).flex_none());
                div()
                    .h_flex()
                    .items_center()
                    .gap(px(12.))
                    .px(px(12.))
                    .py(px(8.))
                    .children(preview)
                    .child(
                        self.picker_row("App icon", "Choose the icon shown in the Dock and app switcher.", "macos-icon", &options, label_icon, look, cx).flex_1(),
                    )
            }
            Row::QuotaToggle => {
                let on = Prefs::read(cx).shows_quota();
                form_row(
                    "Show AI quota tracker",
                    "Display Claude and Codex usage in the workspace sidebar.",
                    look,
                    ui::toggle("quota-toggle", on, look).on_click(cx.listener(move |_, _, _, cx| {
                        Prefs::update(cx, |prefs| prefs.set_shows_ai_quota_tracker(!on));
                        cx.refresh_windows();
                    })),
                )
            }
            Row::NewTabPlacement => {
                let current = Prefs::read(cx).new_tab_placement();
                let key = "new-tab-placement";
                let mut holder = div().relative().child(
                    ui::popup_face(ui::named("picker", key), current.label(), look).on_click(cx.listener(move |this, _, _, cx| {
                        this.open_picker = if this.open_picker == Some(key) { None } else { Some(key) };
                        cx.notify();
                    })),
                );
                if self.open_picker == Some(key) {
                    let items = HorizontalNewTabButtonPlacement::ALL.into_iter().map(|placement| {
                        ui::menu_item(ui::named("placement", placement.raw_value()), placement.label(), placement == current, look)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.open_picker = None;
                                Prefs::update(cx, |prefs| prefs.set_horizontal_new_tab_button_placement(placement));
                                cx.refresh_windows();
                            }))
                    });
                    holder = holder.child(deferred(
                        anchored()
                            .anchor(Anchor::TopRight)
                            .offset(point(px(180.), px(24.)))
                            .child(
                                ui::menu(look).on_mouse_down_out(cx.listener(|this, _, _, cx| {
                                    this.open_picker = None;
                                    cx.notify();
                                })).children(items),
                            ),
                    ));
                }
                form_row("New button in horizontal tabs", "Choose where the new-tab button appears in horizontal layouts.", look, holder)
            }
            Row::WorktreeFolder => self.worktree_folder(look, cx),
            Row::CheckForUpdates => div().h_flex().px(px(12.)).py(px(6.)).child(
                ui::button("check-updates", "Check for Updates Now", look, Variant::Regular, false, true)
                    .on_click(|_, _, cx| check_for_updates(cx)),
            ),
        }
    }

    fn worktree_folder(&self, look: &Look, cx: &mut Context<Self>) -> Div {
        let prefs = Prefs::read(cx);
        let root = prefs.worktree_root();
        let is_default = prefs.worktree_root_is_default();
        let shown = root.display().to_string();
        div()
            .v_flex()
            .child(form_row(
                "Parent folder",
                "New worktrees are grouped by repository inside this folder.",
                look,
                div()
                    .h_flex()
                    .items_center()
                    .gap(px(8.))
                    .child(div().text_size(px(text::BODY)).child(middle_truncate(&shown, 34)))
                    .child(ui::button("worktree-choose", "Choose…", look, Variant::Regular, false, true).on_click(
                        cx.listener(|_, _, _, cx| {
                            let receiver = cx.prompt_for_paths(PathPromptOptions {
                                files: false,
                                directories: true,
                                multiple: false,
                                prompt: Some("Choose where Angelly creates worktrees.".into()),
                            });
                            cx.spawn(async move |this, cx| {
                                let Ok(Ok(Some(paths))) = receiver.await else { return };
                                let Some(folder) = paths.into_iter().next() else { return };
                                let _ = this.update(cx, |_, cx| {
                                    Prefs::update(cx, |prefs| prefs.set_worktree_root(&folder));
                                    cx.notify();
                                });
                            })
                            .detach();
                        }),
                    ))
                    .child(
                        ui::button("worktree-reset", "Reset", look, Variant::Regular, false, !is_default).when(
                            !is_default,
                            |b| {
                                b.on_click(cx.listener(|_, _, _, cx| {
                                    Prefs::update(cx, |prefs| prefs.reset_worktree_root());
                                    cx.notify();
                                }))
                            },
                        ),
                    ),
            ))
            .child(
                div()
                    .px(px(12.))
                    .pb(px(8.))
                    .text_size(px(text::CAPTION))
                    .text_color(look.secondary)
                    .child("Changing this setting does not move existing worktrees."),
            )
    }
}

fn form_row(title: &str, subtitle: &str, look: &Look, control: impl IntoElement) -> Div {
    div()
        .h_flex()
        .items_center()
        .justify_between()
        .gap(px(24.))
        .min_h(px(88.))
        .py(px(22.))
        .text_size(px(text::BODY))
        .text_color(look.primary)
        .child(div().v_flex().flex_1().min_w_0().gap(px(6.))
            .child(title.to_string())
            .child(div().text_color(look.secondary).child(subtitle.to_string())))
        .child(div().flex_none().child(control))
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.ensure_inputs(window, cx);
        let look = Look::of(cx);

        let mut sidebar = div().id("settings-categories").v_flex().w(px(220.)).flex_none().h_full()
            .overflow_y_scroll().px(px(12.)).pb(px(24.)).bg(look.card);
        for (section, categories) in Category::SECTIONS {
            let items = categories.iter().copied().map(|category| {
                let selected = category == self.category;
                Button::new(ui::named("category", category.title()))
                    .ghost()
                    .w_full()
                    .h(px(32.))
                    .accessibility_label(category.title())
                    .child(div().w_full().h_flex().items_center().gap(px(8.))
                        .child(icon(category.icon(), 14., if selected { look.accent } else { look.secondary }))
                        .child(category.title()))
                    .when(selected, |d| d.bg(look.control).text_color(look.primary))
                    .text_color(look.primary)
                    .on_click(cx.listener(move |this, _, window, cx| this.select_category(category, window, cx)))
            });
            sidebar = sidebar.child(div().v_flex().gap(px(4.)).mt(px(24.))
                .child(div().px(px(8.)).mb(px(4.)).text_size(px(text::CALLOUT)).text_color(look.secondary).child(section))
                .children(items));
        }

        let form_rows = rows(self.category, &self.fonts, &self.themes);
        let count = form_rows.len();
        let mut group = div().v_flex();
        for (index, row) in form_rows.iter().enumerate() {
            group = group.child(self.row(row, &look, cx));
            if index + 1 < count {
                group = group.child(div().h(px(1.)).bg(look.separator));
            }
        }

        let footer = div()
            .v_flex()
            .gap(px(6.))
            .py(px(16.))
            .w_full()
            .max_w(px(840.))
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .justify_between()
                    .gap(px(8.))
                    .child(
                        div()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_size(px(text::BODY))
                            .text_color(look.secondary)
                            .child(middle_truncate(&self.model.path().display().to_string(), 56)),
                    )
                    .child(
                        ui::button("open-config", "Open Config File…", &look, Variant::Regular, false, true)
                            .on_click(cx.listener(|this, _, _, cx| this.open_config_file(cx))),
                    ),
            )
            .children(
                self.model
                    .write_error
                    .clone()
                    .map(|error| div().text_size(px(text::BODY)).text_color(look.red).child(error)),
            )
            .child(
                div()
                    .text_size(px(text::CAPTION))
                    .text_color(look.secondary)
                    .child("Other options can be set in the config file. Some changes apply only to new windows."),
            );

        let spec = crate::window_chrome::Spec::of(cx);
        let header_height = crate::workspace::TITLEBAR_HEIGHT;
        let header = div()
            .h_flex()
            .h(px(header_height))
            .flex_none()
            .items_center()
            .bg(look.card)
            .child(div().w(px(if spec.titlebar_height() > 0.0 { spec.leading_inset() } else { 8. })).flex_none())
            .child(Button::new("settings-back").ghost().label("Back").icon(IconName::ArrowLeft)
                .on_click(cx.listener(|this, _, window, cx| this.back(window, cx))))
            .child(crate::platform::window_controls::drag_area(
                div().flex_1().h_full().window_control_area(WindowControlArea::Drag), window))
            .children(crate::platform::window_controls::render(window, crate::sidebar::Palette::of(cx.global::<crate::theme::Theme>()), header_height));

        div()
            .id("settings")
            .key_context("Settings")
            .track_focus(&self.focus)
            .size_full()
            .v_flex()
            .bg(look.background)
            .text_color(look.primary)
            .font_family(ui::FONT).line_height(relative(ui::LINE_HEIGHT))
            .on_action(cx.listener(|this, _: &Escape, window, cx| this.escape(window, cx)))
            .on_action(cx.listener(|this, _: &crate::workspace::CloseTab, window, cx| this.back(window, cx)))
            .on_action(cx.listener(|this, _: &crate::splits::ClosePane, window, cx| this.back(window, cx)))
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    this.escape(window, cx);
                    cx.stop_propagation();
                }
            }))
            .child(header)
            .child(div().h_flex().flex_1().min_h_0()
            .child(sidebar)
            .child(ui::vdivider(&look))
            .child(
                div()
                    .v_flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(
                        div()
                            .id("settings-form")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .px(px(32.))
                            .py(px(32.))
                            .child(div().v_flex().w_full().max_w(px(840.))
                                .child(div().mb(px(8.)).text_size(px(24.)).child(self.category.title()))
                                .child(div().mb(px(20.)).text_size(px(text::BODY)).text_color(look.secondary).child(self.category.subtitle()))
                                .child(group)),
                    )
                    .child(ui::divider(&look))
                    .child(div().px(px(32.)).child(footer)),
            ))
    }
}

#[cfg(test)]
mod tests {
    use std::prelude::v1::test;

    use super::*;
    use angelly_core::config::Platform;

    fn scratch(contents: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.ghostty");
        std::fs::write(&path, contents).unwrap();
        (dir, path)
    }

    #[test]
    fn enum_labels() {
        assert_eq!(enum_label("true"), "On");
        assert_eq!(enum_label("false"), "Off");
        assert_eq!(enum_label("block_hollow"), "Block hollow");
        assert_eq!(enum_label("ghostty"), "Ghostty");
        assert_eq!(enum_label(""), "");
        assert_eq!(label_icon("xray"), "X-Ray");
        assert_eq!(label_icon("attack-on-titan"), "Attack on Titan");
        assert_eq!(label_restore("default"), "Follow macOS");
        assert_eq!(label_confirm("true"), "If a process is running");
        assert_eq!(label_tab_position("end"), "At the end");
        assert_eq!(label_tab_position("current"), "Next to current tab");
        assert_eq!(label_option_alt("left"), "Left Option only");
        assert_eq!(label_updates("download"), "Check and download");
    }

    #[test]
    fn unknown_values_stay_selectable() {
        let options = vec!["a".to_string(), "b".to_string()];
        assert_eq!(picker_choices(Some("b"), &options), ["a", "b"]);
        assert_eq!(picker_choices(Some("z"), &options), ["z", "a", "b"]);
        assert_eq!(picker_choices(None, &options), ["a", "b"]);
    }

    #[test]
    fn every_category_has_controls() {
        for category in Category::ALL {
            assert!(!rows(category, &["Menlo".into()], &["Angelly Dark".into()]).is_empty(), "{category:?}");
        }
        let keys: Vec<&str> = Category::ALL
            .into_iter()
            .flat_map(|c| rows(c, &[], &[]))
            .filter_map(|row| match row {
                Row::Picker { key, .. } | Row::Text { key, .. } => Some(key),
                _ => None,
            })
            .collect();
        for key in [
            "font-family",
            "font-size",
            "theme",
            "window-theme",
            "background-opacity",
            "background-blur",
            "cursor-style",
            "cursor-style-blink",
            "window-padding-x",
            "window-padding-y",
            "macos-titlebar-style",
            "window-save-state",
            "copy-on-select",
            "confirm-close-surface",
            "window-new-tab-position",
            "quit-after-last-window-closed",
            "mouse-hide-while-typing",
            "clipboard-paste-protection",
            "window-inherit-working-directory",
            "macos-option-as-alt",
            "auto-update",
        ] {
            assert!(keys.contains(&key), "missing {key}");
        }
    }

    #[test]
    fn setting_a_value_rewrites_only_its_line() {
        let (_dir, path) = scratch("# my config\nfont-size = 12\ntheme = Nord\n");
        let mut model = SettingsModel::new(&path);
        assert_eq!(model.values.get("font-size").map(String::as_str), Some("12"));
        assert!(model.set("font-size", Some("14")));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "# my config\nfont-size = 14\ntheme = Nord\n");
        assert_eq!(model.values["font-size"], "14");
    }

    #[test]
    fn default_removes_the_key_and_new_keys_are_appended() {
        let (_dir, path) = scratch("# keep\ntheme = Nord\n");
        let mut model = SettingsModel::new(&path);
        assert!(model.set("theme", None));
        assert!(model.set("cursor-style", Some("bar")));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "# keep\ncursor-style = bar\n");
        assert!(!model.values.contains_key("theme"));
    }

    #[test]
    fn unchanged_values_do_not_touch_the_file() {
        let (_dir, path) = scratch("theme = Nord\n");
        let mut model = SettingsModel::new(&path);
        assert!(!model.set("theme", Some("Nord")));
        assert!(!model.set("font-size", None));
    }

    #[test]
    fn failed_writes_are_reported() {
        let dir = tempfile::tempdir().unwrap();
        let blocker = dir.path().join("file");
        std::fs::write(&blocker, "").unwrap();
        let mut model = SettingsModel::new(blocker.join("config"));
        assert!(!model.set("theme", Some("Nord")));
        assert!(model.write_error.as_deref().is_some_and(|e| e.starts_with("Couldn't save settings")));
    }

    #[test]
    fn theme_names_merge_user_and_bundled_directories() {
        let home = tempfile::tempdir().unwrap();
        let resources = tempfile::tempdir().unwrap();
        let user = home.path().join(".config/ghostty/themes");
        std::fs::create_dir_all(&user).unwrap();
        std::fs::write(user.join("zebra"), "").unwrap();
        std::fs::write(user.join(".hidden"), "").unwrap();
        std::fs::create_dir_all(resources.path().join("themes")).unwrap();
        std::fs::write(resources.path().join("themes").join("Apple"), "").unwrap();
        let env = Env {
            platform: Platform::MacOs,
            home: Some(home.path().to_path_buf()),
            xdg_config_home: None,
            resources_dir: Some(resources.path().to_path_buf()),
        };
        assert_eq!(theme_names(&env), ["Angelly Dark", "Angelly Light", "Apple", "zebra"]);
    }
}
