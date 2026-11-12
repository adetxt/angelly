//! Small macOS-flavoured controls shared by every panel.

use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::StyledExt as _;
use gpui_kit::component::Theme as ComponentTheme;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::theme::Theme;

/// SwiftUI text styles on macOS.
pub mod text {
    pub const TITLE2: f32 = 17.0;
    pub const HEADLINE: f32 = 13.0;
    pub const BODY: f32 = 13.0;
    pub const CALLOUT: f32 = 12.0;
    pub const SUBHEADLINE: f32 = 11.0;
    pub const CAPTION: f32 = 10.0;
}

/// Line height as a multiple of the font size, matching the system UI font.
pub const LINE_HEIGHT: f32 = 1.25;

pub const FONT: &str = ".SystemUIFont";

/// Colours resolved once per render from the active theme.
#[derive(Clone, Copy)]
pub struct Look {
    pub dark: bool,
    pub background: Hsla,
    pub card: Hsla,
    pub primary: Hsla,
    pub secondary: Hsla,
    pub accent: Hsla,
    pub separator: Hsla,
    pub control: Hsla,
    pub control_hover: Hsla,
    pub field: Hsla,
    pub red: Hsla,
    pub orange: Hsla,
    pub yellow: Hsla,
}

impl Look {
    pub fn new(theme: &Theme) -> Self {
        let dark = theme.dark;
        let background = theme.background();
        let primary = theme.primary;
        let card = if dark {
            background.blend(white().opacity(0.07))
        } else {
            background.blend(black().opacity(0.02))
        };
        Look {
            dark,
            background,
            card,
            primary,
            secondary: theme.secondary(),
            accent: theme.accent,
            separator: primary.opacity(0.1),
            control: primary.opacity(if dark { 0.14 } else { 0.07 }),
            control_hover: primary.opacity(if dark { 0.2 } else { 0.11 }),
            field: primary.opacity(if dark { 0.07 } else { 0.04 }),
            red: if dark { rgb(0xFF453A) } else { rgb(0xFF3B30) }.into(),
            orange: if dark { rgb(0xFF9F0A) } else { rgb(0xFF9500) }.into(),
            yellow: if dark { rgb(0xFFD60A) } else { rgb(0xFFCC00) }.into(),
        }
    }

    pub fn of(cx: &App) -> Look {
        Look::new(cx.global::<Theme>())
    }

    pub fn menu_surface(&self) -> Hsla {
        Hsla::from(rgb(if self.dark { 0x29292E } else { 0xF6F6F8 }))
            .opacity(if self.dark { 0.82 } else { 0.88 })
    }
}

/// Mirrors the terminal theme into the component library so its inputs match.
pub fn sync_component_theme(cx: &mut App) {
    let look = Look::of(cx);
    let mode = if look.dark { gpui_kit::component::ThemeMode::Dark } else { gpui_kit::component::ThemeMode::Light };
    ComponentTheme::change(mode, None, cx);
    ComponentTheme::update(cx, |theme| {
        let colors = &mut theme.colors;
        colors.background = look.background;
        colors.foreground = look.primary;
        colors.border = look.primary.opacity(0.2);
        colors.input = look.primary.opacity(0.2);
        colors.muted = look.field;
        colors.muted_foreground = look.secondary;
        colors.caret = look.primary;
        colors.ring = look.accent;
        colors.selection = look.accent.opacity(0.35);
        colors.popover = look.menu_surface();
        colors.popover_foreground = look.primary;
        colors.accent = look.control;
        colors.accent_foreground = look.primary;
        colors.primary = look.accent;
        colors.primary_foreground = white();
        colors.scrollbar_thumb = look.primary.opacity(0.3);
        theme.font_family = FONT.into();
        theme.radius = px(8.);
    });
}

pub fn icon(name: IconName, size: f32, color: Hsla) -> Svg {
    svg().path(name.path()).size(px(size)).flex_none().text_color(color)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    Regular,
    Prominent,
    Destructive,
}

/// A push button. Wire `on_click` on the result; disabled buttons ignore hover.
pub fn button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    look: &Look,
    variant: Variant,
    small: bool,
    enabled: bool,
) -> Stateful<Div> {
    let (fill, hover, text_color) = match variant {
        Variant::Prominent => (look.accent, look.accent.blend(white().opacity(0.12)), white()),
        Variant::Regular => (look.control, look.control_hover, look.primary),
        Variant::Destructive => (look.control, look.control_hover, look.red),
    };
    div()
        .id(id)
        .h_flex()
        .items_center()
        .justify_center()
        .flex_none()
        .h(px(if small { 19. } else { 22. }))
        .px(px(if small { 8. } else { 10. }))
        .rounded(px(5.))
        .bg(fill)
        .text_size(px(if small { text::SUBHEADLINE } else { text::BODY }))
        .text_color(text_color)
        .whitespace_nowrap()
        .child(label.into())
        .when(enabled, |d| d.cursor_pointer().hover(move |s| s.bg(hover)))
        .when(!enabled, |d| d.opacity(0.4))
}

/// A link-styled caption button.
pub fn link(id: impl Into<ElementId>, label: impl Into<SharedString>, look: &Look, size: f32) -> Stateful<Div> {
    div()
        .id(id)
        .cursor_pointer()
        .text_size(px(size))
        .text_color(look.accent)
        .child(label.into())
}

/// A floating card: popovers, palette, sheets.
pub fn card(look: &Look, radius: f32) -> Div {
    div()
        .bg(look.card)
        .rounded(px(radius))
        .border_1()
        .border_color(look.primary.opacity(if look.dark { 0.18 } else { 0.14 }))
        .shadow(vec![BoxShadow {
            color: black().opacity(if look.dark { 0.5 } else { 0.25 }),
            offset: point(px(0.), px(8.)),
            blur_radius: px(24.),
            spread_radius: px(0.),
            inset: false,
        }])
        .overflow_hidden()
}

/// The arrow of a popover that sits to the right of its anchor, pointing left.
pub fn arrow_left(look: &Look) -> Svg {
    svg().path("ui/popover-arrow-left.svg").w(px(7.)).h(px(14.)).flex_none().text_color(look.card)
}

/// The arrow of a popover that sits above its anchor, pointing down.
pub fn arrow_down(look: &Look) -> Svg {
    svg().path("ui/popover-arrow-down.svg").w(px(14.)).h(px(7.)).flex_none().text_color(look.card)
}

/// An element id made of a fixed prefix and a runtime key.
pub fn named(prefix: &'static str, key: impl Into<SharedString>) -> ElementId {
    (ElementId::from(prefix), key.into()).into()
}

pub fn divider(look: &Look) -> Div {
    div().h(px(1.)).w_full().flex_none().bg(look.separator)
}

pub fn vdivider(look: &Look) -> Div {
    div().w(px(1.)).h_full().flex_none().bg(look.separator)
}

/// A linear progress bar filled to `fraction` (0...1).
pub fn progress_bar(fraction: f64, look: &Look) -> Div {
    div()
        .w_full()
        .h(px(4.))
        .rounded_full()
        .bg(look.primary.opacity(0.15))
        .child(
            div()
                .h_full()
                .rounded_full()
                .bg(look.primary)
                .w(relative(fraction.clamp(0.0, 1.0) as f32)),
        )
}

/// A macOS-style on/off switch.
pub fn toggle(id: impl Into<ElementId>, on: bool, look: &Look) -> Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .w(px(38.))
        .h(px(22.))
        .rounded_full()
        .cursor_pointer()
        .bg(if on { look.accent } else { look.primary.opacity(0.18) })
        .child(
            div()
                .absolute()
                .top(px(2.))
                .left(px(if on { 18. } else { 2. }))
                .size(px(18.))
                .rounded_full()
                .bg(white())
                .shadow(vec![BoxShadow {
                    color: black().opacity(0.3),
                    offset: point(px(0.), px(1.)),
                    blur_radius: px(2.),
                    spread_radius: px(0.),
            inset: false,
                }]),
        )
}

/// A pop-up button face: current label with the up/down chevrons.
pub fn popup_face(id: impl Into<ElementId>, label: impl Into<SharedString>, look: &Look) -> Stateful<Div> {
    let hover = look.control_hover;
    div()
        .id(id)
        .h_flex()
        .items_center()
        .gap(px(6.))
        .h(px(22.))
        .pl(px(10.))
        .pr(px(6.))
        .rounded(px(5.))
        .bg(look.control)
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .text_size(px(text::BODY))
        .text_color(look.primary)
        .child(div().whitespace_nowrap().child(label.into()))
        .child(icon(IconName::ChevronsUpDown, 11., look.secondary))
}

pub fn new_input(
    window: &mut Window,
    cx: &mut App,
    placeholder: &str,
    value: &str,
) -> Entity<InputState> {
    let placeholder = placeholder.to_string();
    let value = value.to_string();
    cx.new(|cx| InputState::new(window, cx).placeholder(placeholder).default_value(value))
}

/// A bordered single-line text field.
pub fn field(state: &Entity<InputState>) -> Input {
    Input::new(state).appearance(true).bordered(true)
}

/// A circular stroke: a faint full track and, when `fraction` is set, an arc
/// that starts at 12 o'clock and runs clockwise with round caps.
pub fn ring(diameter: f32, line_width: f32, fraction: Option<f64>, track: Hsla, color: Hsla) -> Canvas<()> {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let center = bounds.center();
            let radius = (diameter - line_width) / 2.0;
            let at = |angle: f32| point(center.x + px(radius * angle.cos()), center.y + px(radius * angle.sin()));
            let stroke = |from: f32, sweep: f32| {
                let steps = ((sweep.abs() / (std::f32::consts::PI / 32.0)).ceil() as usize).max(2);
                let mut path = PathBuilder::stroke(px(line_width));
                path.move_to(at(from));
                for step in 1..=steps {
                    path.line_to(at(from + sweep * step as f32 / steps as f32));
                }
                path.build().ok()
            };
            let start = -std::f32::consts::FRAC_PI_2;
            let tau = std::f32::consts::TAU;
            if let Some(path) = stroke(start, tau) {
                window.paint_path(path, track);
            }
            let Some(fraction) = fraction.map(|f| f.clamp(0.0, 1.0) as f32).filter(|f| *f > 0.0) else {
                return;
            };
            if let Some(path) = stroke(start, tau * fraction) {
                window.paint_path(path, color);
            }
            let mut cap = |angle: f32| {
                let c = at(angle);
                let half = px(line_width / 2.0);
                window.paint_quad(quad(
                    Bounds::new(point(c.x - half, c.y - half), size(half * 2.0, half * 2.0)),
                    half,
                    color,
                    0.0,
                    transparent_black(),
                    BorderStyle::default(),
                ));
            };
            if fraction < 1.0 {
                cap(start);
                cap(start + tau * fraction);
            }
        },
    )
    .size(px(diameter))
}

/// A continuously rotating icon.
pub fn spinner(id: impl Into<ElementId>, size_px: f32, color: Hsla, seconds: f32) -> impl IntoElement {
    icon(IconName::RefreshCw, size_px, color).with_animation(
        id,
        Animation::new(std::time::Duration::from_secs_f32(seconds)).repeat(),
        |svg, delta| svg.with_transformation(Transformation::rotate(percentage(delta))),
    )
}

/// The container of a pop-up menu.
pub fn menu(look: &Look) -> Div {
    card(look, 8.).bg(look.menu_surface()).occlude().p(px(4.)).v_flex().text_size(px(text::BODY)).text_color(look.primary)
}

/// One row of a pop-up menu; the selected value carries a checkmark.
pub fn menu_item(id: impl Into<ElementId>, label: impl Into<SharedString>, selected: bool, look: &Look) -> Stateful<Div> {
    let accent = look.accent;
    div()
        .id(id)
        .h_flex()
        .items_center()
        .gap(px(4.))
        .h(px(22.))
        .px(px(6.))
        .rounded(px(4.))
        .cursor_pointer()
        .whitespace_nowrap()
        .hover(move |s| s.bg(accent).text_color(white()))
        .child(div().w(px(12.)).flex_none().when(selected, |d| d.child(icon(IconName::Check, 10., look.primary))))
        .child(label.into())
}

/// A caption-sized chevron that rotates when expanded.
pub fn disclosure(expanded: bool, look: &Look) -> Svg {
    let mut chevron = icon(IconName::ChevronRight, 10., look.primary);
    if expanded {
        chevron = chevron.with_transformation(Transformation::rotate(percentage(0.25)));
    }
    chevron
}
