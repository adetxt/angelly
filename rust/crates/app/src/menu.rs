//! AppKit owns macOS menu materials and tracking; other platforms use GPUI menus.

use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::*;

#[cfg(target_os = "macos")]
mod macos;

type Handler = Rc<dyn Fn(&mut Window, &mut App)>;

#[derive(Clone)]
enum Operation {
    Callback(Handler),
    Action(Rc<dyn Action>),
}

impl Operation {
    #[cfg(target_os = "macos")]
    fn run(&self, window: &mut Window, cx: &mut App) {
        match self {
            Self::Callback(handler) => handler(window, cx),
            Self::Action(action) => window.dispatch_action(action.boxed_clone(), cx),
        }
    }
}

#[derive(Clone)]
enum Icon {
    Swatch(Option<Hsla>),
    Image(Arc<Image>),
}

#[derive(Clone)]
enum Entry {
    Command {
        label: SharedString,
        icon: Option<Icon>,
        operation: Operation,
    },
    Submenu {
        label: SharedString,
        menu: Menu,
    },
    Separator,
}

#[derive(Clone, Default)]
pub struct Menu {
    entries: Vec<Entry>,
    focus: Option<FocusHandle>,
}

impl Menu {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn action_context(mut self, focus: FocusHandle) -> Self {
        self.focus = Some(focus);
        self
    }

    pub fn item(self, label: impl Into<SharedString>, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.command(label.into(), None, Operation::Callback(Rc::new(handler)))
    }

    pub fn action(self, label: impl Into<SharedString>, action: Box<dyn Action>) -> Self {
        self.command(label.into(), None, Operation::Action(Rc::from(action)))
    }

    pub fn color_item(
        self,
        label: impl Into<SharedString>,
        color: Option<Hsla>,
        handler: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        self.command(
            label.into(),
            Some(Icon::Swatch(color)),
            Operation::Callback(Rc::new(handler)),
        )
    }

    pub fn image_item(
        self,
        label: impl Into<SharedString>,
        image: Option<Arc<Image>>,
        handler: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        self.command(
            label.into(),
            image.map(Icon::Image),
            Operation::Callback(Rc::new(handler)),
        )
    }

    fn command(mut self, label: SharedString, icon: Option<Icon>, operation: Operation) -> Self {
        self.entries.push(Entry::Command { label, icon, operation });
        self
    }

    pub fn submenu(mut self, label: impl Into<SharedString>, menu: Menu) -> Self {
        self.entries.push(Entry::Submenu {
            label: label.into(),
            menu,
        });
        self
    }

    pub fn separator(mut self) -> Self {
        self.entries.push(Entry::Separator);
        self
    }

    #[cfg(not(target_os = "macos"))]
    fn into_gpui(
        self,
        mut menu: gpui_kit::component::menu::PopupMenu,
        window: &mut Window,
        cx: &mut Context<gpui_kit::component::menu::PopupMenu>,
    ) -> gpui_kit::component::menu::PopupMenu {
        use gpui_kit::component::menu::PopupMenuItem;
        use gpui_kit::component::StyledExt as _;
        if let Some(focus) = self.focus {
            menu = menu.action_context(focus);
        }
        for entry in self.entries {
            menu = match entry {
                Entry::Separator => menu.separator(),
                Entry::Submenu { label, menu: submenu } => menu.submenu(label, window, cx, move |menu, window, cx| {
                    submenu.clone().into_gpui(menu, window, cx)
                }),
                Entry::Command { label, icon, operation } => {
                    let item = match icon {
                        None => PopupMenuItem::new(label),
                        Some(icon) => PopupMenuItem::element(move |_, _| {
                            let mark = match &icon {
                                Icon::Image(image) => img(image.clone()).size(px(16.)).into_any_element(),
                                Icon::Swatch(color) => {
                                    let dot = div().size(px(10.)).rounded_full();
                                    match color {
                                        Some(color) => dot.bg(*color),
                                        None => dot.border_1().border_color(rgb(0x8E8E93)),
                                    }
                                    .into_any_element()
                                }
                            };
                            div()
                                .h_flex()
                                .items_center()
                                .gap(px(8.))
                                .child(mark)
                                .child(label.clone())
                        }),
                    };
                    menu.item(match operation {
                        Operation::Action(action) => item.action(action.boxed_clone()),
                        Operation::Callback(handler) => item.on_click(move |_, window, cx| handler(window, cx)),
                    })
                }
            };
        }
        menu
    }
}

pub trait ContextMenuExt: InteractiveElement + ParentElement + Styled + IntoElement + Sized + 'static {
    fn context_menu(self, build: impl Fn(&mut Window, &mut App) -> Menu + 'static) -> impl IntoElement {
        #[cfg(target_os = "macos")]
        {
            self.on_mouse_down(MouseButton::Right, move |event, window, cx| {
                cx.stop_propagation();
                let menu = build(window, cx);
                macos::show(menu, event.position, window, cx);
            })
        }
        #[cfg(not(target_os = "macos"))]
        {
            gpui_kit::component::menu::ContextMenuExt::context_menu(self, move |menu, window, cx| {
                build(window, cx).into_gpui(menu, window, cx)
            })
        }
    }
}

impl<E: InteractiveElement + ParentElement + Styled + IntoElement + Sized + 'static> ContextMenuExt for E {}

pub fn dropdown_menu(
    button: gpui_kit::component::button::Button,
    build: impl Fn(&mut Window, &mut App) -> Menu + 'static,
) -> impl IntoElement {
    #[cfg(target_os = "macos")]
    {
        use std::cell::Cell;
        let bounds = Rc::new(Cell::new(Bounds::<Pixels>::default()));
        let capture = bounds.clone();
        div()
            .relative()
            .child(
                canvas(move |rect, _, _| capture.set(rect), |_, _, _, _| {})
                    .absolute()
                    .inset_0(),
            )
            .child(button.on_click(move |_, window, cx| {
                let rect = bounds.get();
                let menu = build(window, cx);
                macos::show(menu, point(rect.left(), rect.bottom() + px(4.)), window, cx);
            }))
    }
    #[cfg(not(target_os = "macos"))]
    {
        gpui_kit::component::menu::DropdownMenu::dropdown_menu(button, move |menu, window, cx| {
            build(window, cx).into_gpui(menu, window, cx)
        })
    }
}
