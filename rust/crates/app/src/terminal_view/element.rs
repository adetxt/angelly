//! The custom gpui element that lays out and paints the cell grid.
//!
//! Painting is split in two: a per-row layout (backgrounds, shaped text runs,
//! custom glyphs and decorations) that only depends on the row's cells and a
//! few view flags, and the paint pass that replays it. Layouts are cached per
//! row and keyed by a hash of everything they depend on, so a frame only
//! reshapes the rows whose content, selection, highlights or cursor changed.

use std::hash::{Hash, Hasher};
use std::rc::Rc;
use std::time::Instant;

use gpui_kit::*;
use terminal::{
    Cell, CellFlags, Colors, CursorStyle, Frame, Padding, Rgb, SearchMatch, Underline as UnderlineKind, Wide,
};

use super::colors;
use super::font::{FontSet, Metrics};
use super::glyphs::{self, Shape};
use super::input::{char_cells, Preedit};
use super::mouse::{self, Link};
use super::scroll;
use super::settings::{BoldColor, Settings};
use super::TerminalView;
use crate::chrome::SidebarDragging;
use crate::theme::hsla_of;

pub struct TerminalElement {
    view: Entity<TerminalView>,
}

impl TerminalElement {
    pub fn new(view: Entity<TerminalView>) -> Self {
        TerminalElement { view }
    }
}

pub struct Prepaint {
    hitbox: Hitbox,
}

impl IntoElement for TerminalElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TerminalElement {
    type RequestLayoutState = ();
    type PrepaintState = Prepaint;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let style = Style { size: size(relative(1.).into(), relative(1.).into()), ..Default::default() };
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Prepaint {
        let started = Instant::now();
        self.view.update(cx, |view, cx| view.prepare(bounds, window, cx));
        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        self.view.update(cx, |view, _| view.stats.prepaint = started.elapsed());
        Prepaint { hitbox }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        prepaint: &mut Prepaint,
        window: &mut Window,
        cx: &mut App,
    ) {
        let started = Instant::now();
        let mut state = self.view.update(cx, |view, cx| view.begin_paint(window, cx));
        let Some(metrics) = state.metrics else {
            self.view.update(cx, |view, _| view.end_paint(state));
            return;
        };
        window.handle_input(&state.focus, ElementInputHandler::new(bounds, self.view.clone()), cx);
        mouse::register(&self.view, &prepaint.hitbox, window);
        window.set_cursor_style(state.mouse_cursor, &prepaint.hitbox);
        // The grid can outlast the view's bounds while a sidebar drag holds the resize.
        window.with_content_mask(Some(ContentMask { bounds }), |window| state.paint(bounds, metrics, window, cx));
        if state.animating {
            window.request_animation_frame();
        }
        let elapsed = started.elapsed();
        self.view.update(cx, |view, _| {
            view.stats.record(elapsed);
            view.end_paint(state);
        });
    }
}

/// Paint timing, reported by the selftest benchmark.
#[derive(Default)]
pub struct PaintStats {
    pub prepaint: std::time::Duration,
    pub frames: u32,
    pub total: std::time::Duration,
    pub worst: std::time::Duration,
    pub rows_built: u64,
}

impl PaintStats {
    fn record(&mut self, paint: std::time::Duration) {
        let frame = paint + self.prepaint;
        self.frames += 1;
        self.total += frame;
        self.worst = self.worst.max(frame);
    }
}

/// Fx-style hasher: the row keys are computed for every row on every frame,
/// so they need to be cheap.
#[derive(Default)]
struct RowHasher(u64);

impl Hasher for RowHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for chunk in bytes.chunks(8) {
            let mut buf = [0u8; 8];
            buf[..chunk.len()].copy_from_slice(chunk);
            self.write_u64(u64::from_le_bytes(buf));
        }
    }

    fn write_u64(&mut self, v: u64) {
        self.0 = (self.0.rotate_left(5) ^ v).wrapping_mul(0x517c_c1b7_2722_0a95);
    }

    fn write_u32(&mut self, v: u32) {
        self.write_u64(u64::from(v));
    }

    fn write_u16(&mut self, v: u16) {
        self.write_u64(u64::from(v));
    }

    fn write_u8(&mut self, v: u8) {
        self.write_u64(u64::from(v));
    }

    fn write_usize(&mut self, v: usize) {
        self.write_u64(v as u64);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SpanKind {
    Selection,
    Match,
    CurrentMatch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Span {
    pub start: u16,
    pub end: u16,
    pub kind: SpanKind,
}

enum DecoKind {
    Underline(UnderlineKind),
    Strike,
    Overline,
}

struct Deco {
    start: u16,
    end: u16,
    kind: DecoKind,
    color: Hsla,
}

struct GlyphItem {
    col: u16,
    shapes: Vec<Shape>,
    color: Hsla,
}

#[derive(Default)]
struct RowItems {
    bgs: Vec<(u16, u16, Hsla)>,
    texts: Vec<(u16, ShapedLine)>,
    glyphs: Vec<GlyphItem>,
    decos: Vec<Deco>,
}

struct RowEntry {
    key: u64,
    items: RowItems,
}

/// Cached row layouts, owned by the view and taken out while painting.
#[derive(Default)]
pub struct Painter {
    rows: Vec<Option<RowEntry>>,
}

/// Everything the paint pass reads, captured once per frame.
pub struct PaintState {
    pub frame: Frame,
    painter: Painter,
    pub focus: FocusHandle,
    pub metrics: Option<Metrics>,
    pub mouse_cursor: gpui_kit::CursorStyle,
    pub animating: bool,
    settings: Rc<Settings>,
    fonts: Rc<FontSet>,
    origin: Point<Pixels>,
    focused: bool,
    font_size: f32,
    blink_on: bool,
    epoch: u64,
    preedit: Option<Preedit>,
    link: Option<Link>,
    matches: Vec<SearchMatch>,
    current_match: Option<SearchMatch>,
    scroller_opacity: f32,
    scroller_hover: bool,
    dimmed: bool,
    pub rows_built: u64,
}

/// Visual state of the cursor for one frame.
#[derive(Clone, Copy)]
struct CursorPaint {
    col: u16,
    row: u16,
    width: u16,
    style: CursorStyle,
    color: Hsla,
    text: Rgb,
}

impl TerminalView {
    /// Measures fonts, resolves the grid for `bounds` and refreshes the frame.
    pub(crate) fn prepare(&mut self, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let metrics = match self.metrics {
            Some(m) if m.scale == window.scale_factor() => m,
            _ => {
                let m = Metrics::measure(window, &self.fonts, self.font_size, &self.settings);
                self.metrics = Some(m);
                self.style_epoch += 1;
                m
            }
        };
        let pad = self.settings.padding;
        let avail_w = f32::from(bounds.size.width) - pad.left - pad.right;
        let avail_h = f32::from(bounds.size.height) - pad.top - pad.bottom;
        let (cw, ch) = (f32::from(metrics.cell_width), f32::from(metrics.cell_height));
        let cols = ((avail_w / cw).floor().max(1.0)) as u16;
        let rows = ((avail_h / ch).floor().max(1.0)) as u16;
        let snap = |v: f32| (v * metrics.scale).floor() / metrics.scale;
        let (extra_x, extra_y) = if self.settings.balance_padding {
            (
                snap(((avail_w - f32::from(cols) * cw) / 2.0).max(0.0)),
                snap(((avail_h - f32::from(rows) * ch) / 2.0).max(0.0)),
            )
        } else {
            (0.0, 0.0)
        };
        self.layout.bounds = bounds;
        self.layout.origin = point(bounds.origin.x + px(pad.left + extra_x), bounds.origin.y + px(pad.top + extra_y));
        // A new view still needs its first grid; later changes wait for the sidebar drag to end.
        let held = self.layout.grid != (0, 0) && SidebarDragging::get(cx);
        if self.layout.grid != (cols, rows) && !held {
            self.apply_grid(cols, rows, cx);
        }
        let mut term = self.session.term();
        let scale = metrics.scale;
        term.set_padding(Padding {
            left: ((pad.left + extra_x) * scale) as u32,
            top: ((pad.top + extra_y) * scale) as u32,
            right: ((pad.right + extra_x) * scale) as u32,
            bottom: ((pad.bottom + extra_y) * scale) as u32,
        });
        term.update_frame(&mut self.frame);
        if self.search.is_some() {
            if let Some(status) = term.search_poll() {
                if let Some(search) = &mut self.search {
                    search.total = status.total;
                    search.selected = status.selected;
                }
                if !status.complete {
                    window.request_animation_frame();
                }
            }
        }
    }

    fn begin_paint(&mut self, _: &mut Window, _: &mut Context<Self>) -> PaintState {
        let (matches, current_match) = if self.search.is_some() {
            let term = self.session.term();
            (term.search_viewport_matches(), term.search_selected_match())
        } else {
            (Vec::new(), None)
        };
        let (scroller_opacity, scroll_animating) =
            if self.settings.scrollbar { self.scroller.opacity(Instant::now()) } else { (0.0, false) };
        let link = self.hover_link.clone();
        let mouse_cursor = if self.mouse.over_scrollbar {
            gpui_kit::CursorStyle::Arrow
        } else if link.is_some() {
            gpui_kit::CursorStyle::PointingHand
        } else if self.mouse.mods.alt {
            gpui_kit::CursorStyle::Crosshair
        } else {
            gpui_kit::CursorStyle::IBeam
        };
        let progress_animating = matches!(self.progress, Some(p) if p.state == terminal::ProgressState::Indeterminate);
        PaintState {
            frame: std::mem::take(&mut self.frame),
            painter: std::mem::take(&mut self.painter),
            focus: self.focus.clone(),
            metrics: self.metrics,
            mouse_cursor,
            animating: scroll_animating || progress_animating,
            settings: self.settings.clone(),
            fonts: self.fonts.clone(),
            origin: self.layout.origin,
            focused: self.is_focused(),
            font_size: self.font_size,
            blink_on: self.blink_on,
            epoch: self.style_epoch,
            preedit: self.preedit.clone(),
            link,
            matches,
            current_match,
            scroller_opacity,
            scroller_hover: self.mouse.over_scrollbar || self.scroller.dragging,
            dimmed: self.dimmed,
            rows_built: 0,
        }
    }

    fn end_paint(&mut self, state: PaintState) {
        self.stats.rows_built += state.rows_built;
        self.frame = state.frame;
        self.painter = state.painter;
    }
}

fn spans_for_rows(frame: &Frame, matches: &[SearchMatch], current: Option<SearchMatch>) -> Vec<Vec<Span>> {
    let rows = frame.rows.len();
    let last_col = frame.cols.saturating_sub(1);
    let mut spans: Vec<Vec<Span>> = vec![Vec::new(); rows];
    for (y, row) in frame.rows.iter().enumerate() {
        if let Some((a, b)) = row.selection {
            spans[y].push(Span { start: a, end: b, kind: SpanKind::Selection });
        }
    }
    for m in matches {
        let kind = if Some(*m) == current { SpanKind::CurrentMatch } else { SpanKind::Match };
        for r in m.start.1..=m.end.1 {
            let Some(row) = spans.get_mut(usize::from(r)) else { continue };
            let start = if r == m.start.1 { m.start.0 } else { 0 };
            let end = if r == m.end.1 { m.end.0 } else { last_col };
            row.push(Span { start, end, kind });
        }
    }
    spans
}

/// Colors that do not depend on the cell.
struct Palette {
    fg: Rgb,
    bg: Rgb,
    selection_fg: Option<Rgb>,
    selection_bg: Option<Rgb>,
    match_fg: Rgb,
    match_bg: Rgb,
    current_fg: Rgb,
    current_bg: Rgb,
}

impl Palette {
    fn new(colors: &Colors) -> Palette {
        Palette {
            fg: colors.foreground,
            bg: colors.background,
            selection_fg: colors.selection_foreground,
            selection_bg: colors.selection_background,
            match_fg: Rgb::new(0, 0, 0),
            match_bg: Rgb::new(0xFF, 0xE0, 0x82),
            current_fg: Rgb::new(0, 0, 0),
            current_bg: Rgb::new(0xF2, 0xA5, 0x7E),
        }
    }
}

/// The painted colors of one cell: foreground and an explicit background when
/// it differs from the default.
fn cell_colors(
    cell: &Cell,
    col: u16,
    spans: &[Span],
    palette: &Palette,
    colors: &Colors,
    settings: &Settings,
    cursor_text: Option<(u16, Rgb)>,
) -> (Rgb, Option<Rgb>) {
    let (mut fg, mut bg) = cell.colors(palette.fg, palette.bg);
    let bold = cell.flags.contains(CellFlags::BOLD);
    if bold && cell.fg.is_some() && !cell.flags.contains(CellFlags::INVERSE) {
        match settings.bold {
            BoldColor::Bright => fg = colors::bright_variant(fg, &colors.palette),
            BoldColor::Color(c) => fg = c,
            BoldColor::AsIs => {}
        }
    }
    if cell.flags.contains(CellFlags::FAINT) {
        fg = colors::mix(bg, fg, settings.faint_opacity);
    }
    let span = [SpanKind::CurrentMatch, SpanKind::Selection, SpanKind::Match]
        .into_iter()
        .find(|k| spans.iter().any(|s| s.kind == *k && s.start <= col && col <= s.end));
    let mut explicit = false;
    match span {
        Some(SpanKind::Selection) => {
            let (sf, sb) = (palette.selection_fg.unwrap_or(bg), palette.selection_bg.unwrap_or(fg));
            fg = sf;
            bg = sb;
            explicit = true;
        }
        Some(SpanKind::Match) => {
            fg = palette.match_fg;
            bg = palette.match_bg;
            explicit = true;
        }
        Some(SpanKind::CurrentMatch) => {
            fg = palette.current_fg;
            bg = palette.current_bg;
            explicit = true;
        }
        None => {
            if settings.minimum_contrast > 1.0 {
                fg = colors::ensure_contrast(fg, bg, settings.minimum_contrast);
            }
        }
    }
    if cell.flags.contains(CellFlags::INVISIBLE) {
        fg = bg;
    }
    if let Some((c, text)) = cursor_text {
        if c == col {
            fg = text;
        }
    }
    (fg, (explicit || bg != palette.bg).then_some(bg))
}

#[derive(Clone, Copy)]
struct RowInputs<'a> {
    palette: &'a Palette,
    colors: &'a Colors,
    settings: &'a Settings,
    fonts: &'a FontSet,
    metrics: Metrics,
    font_size: Pixels,
    cursor_text: Option<(u16, Rgb)>,
}

fn row_key(row: &terminal::Row, spans: &[Span], inputs: &RowInputs, epoch: u64) -> u64 {
    let mut h = RowHasher::default();
    epoch.hash(&mut h);
    inputs.palette.fg.hash(&mut h);
    inputs.palette.bg.hash(&mut h);
    spans.hash(&mut h);
    inputs.cursor_text.hash(&mut h);
    for c in &row.cells {
        c.ch.hash(&mut h);
        c.extra.hash(&mut h);
        c.fg.hash(&mut h);
        c.bg.hash(&mut h);
        c.underline_color.hash(&mut h);
        c.flags.hash(&mut h);
        c.underline.hash(&mut h);
        c.wide.hash(&mut h);
    }
    h.finish()
}

struct RunStyle {
    fg: Rgb,
    bold: bool,
    italic: bool,
}

struct Run {
    start: u16,
    text: String,
    style: RunStyle,
}

fn build_row(row: &terminal::Row, spans: &[Span], inputs: &RowInputs, window: &Window) -> RowItems {
    let mut items = RowItems::default();
    let cells = &row.cells;
    let scale = inputs.metrics.scale;
    let (cw, ch) = (f32::from(inputs.metrics.cell_width), f32::from(inputs.metrics.cell_height));
    let _ = ch;

    let styled: Vec<(Rgb, Option<Rgb>)> = cells
        .iter()
        .enumerate()
        .map(|(x, c)| {
            cell_colors(c, x as u16, spans, inputs.palette, inputs.colors, inputs.settings, inputs.cursor_text)
        })
        .collect();

    // Backgrounds: merge neighbouring cells of one color.
    let mut x = 0usize;
    while x < cells.len() {
        let Some(bg) = styled[x].1 else {
            x += 1;
            continue;
        };
        let start = x;
        while x < cells.len() && styled[x].1 == Some(bg) {
            x += 1;
        }
        items.bgs.push((start as u16, x as u16, hsla_of(bg)));
    }

    // Decorations: underline, strikethrough, overline spans of equal color.
    let push_deco = |items: &mut RowItems, start: usize, end: usize, kind: DecoKind, color: Rgb| {
        items.decos.push(Deco { start: start as u16, end: end as u16, kind, color: hsla_of(color) });
    };
    let mut x = 0usize;
    while x < cells.len() {
        let c = &cells[x];
        if c.wide == Wide::SpacerTail {
            x += 1;
            continue;
        }
        let color_of = |c: &Cell, fg: Rgb| c.underline_color.unwrap_or(fg);
        if c.underline != UnderlineKind::None {
            let (kind, color) = (c.underline, color_of(c, styled[x].0));
            let start = x;
            while x < cells.len() && cells[x].underline == kind && color_of(&cells[x], styled[x].0) == color {
                x += 1;
            }
            push_deco(&mut items, start, x, DecoKind::Underline(kind), color);
            continue;
        }
        x += 1;
    }
    for (flag, kind) in [(CellFlags::STRIKETHROUGH, 0), (CellFlags::OVERLINE, 1)] {
        let mut x = 0usize;
        while x < cells.len() {
            if !cells[x].flags.contains(flag) {
                x += 1;
                continue;
            }
            let (start, color) = (x, styled[x].0);
            while x < cells.len() && cells[x].flags.contains(flag) && styled[x].0 == color {
                x += 1;
            }
            let kind = if kind == 0 { DecoKind::Strike } else { DecoKind::Overline };
            push_deco(&mut items, start, x, kind, color);
        }
    }

    // Text: runs of narrow cells sharing a style, wide cells on their own.
    let text_system = window.text_system();
    let shape = |text: String, style: &RunStyle, force: f32| -> ShapedLine {
        let font = inputs.fonts.face(style.bold, style.italic).clone();
        let run = TextRun {
            len: text.len(),
            font,
            color: hsla_of(style.fg),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        text_system.shape_line(text.into(), inputs.font_size, &[run], Some(px(force)))
    };
    let flush = |items: &mut RowItems, run: Option<Run>| {
        let Some(mut run) = run else { return };
        let trimmed = run.text.trim_end_matches(' ').len();
        run.text.truncate(trimmed);
        if run.text.is_empty() {
            return;
        }
        let line = shape(run.text, &run.style, cw);
        items.texts.push((run.start, line));
    };

    let mut run: Option<Run> = None;
    for (x, cell) in cells.iter().enumerate() {
        let (fg, _) = styled[x];
        let skip = cell.is_empty()
            || cell.wide == Wide::SpacerTail
            || cell.wide == Wide::SpacerHead
            || cell.flags.contains(CellFlags::INVISIBLE);
        if skip {
            if cell.is_empty() || cell.wide != Wide::SpacerTail {
                flush(&mut items, run.take());
            }
            continue;
        }
        if !cell.flags.contains(CellFlags::INVISIBLE) && cell.extra.is_none() && glyphs::is_custom(cell.ch) {
            flush(&mut items, run.take());
            if let Some(shapes) = glyphs::shapes(cell.ch, cw, f32::from(inputs.metrics.cell_height), scale) {
                items.glyphs.push(GlyphItem { col: x as u16, shapes, color: hsla_of(fg) });
            }
            continue;
        }
        let style =
            RunStyle { fg, bold: cell.flags.contains(CellFlags::BOLD), italic: cell.flags.contains(CellFlags::ITALIC) };
        if cell.wide == Wide::Wide {
            flush(&mut items, run.take());
            let mut text = String::new();
            cell.push_text(&mut text);
            items.texts.push((x as u16, shape(text, &style, cw * 2.0)));
            continue;
        }
        let is_space = cell.ch == ' ' && cell.extra.is_none();
        if let Some(r) = &mut run {
            let same = r.style.fg == style.fg && r.style.bold == style.bold && r.style.italic == style.italic;
            if same || is_space {
                cell.push_text(&mut r.text);
                continue;
            }
            flush(&mut items, run.take());
        }
        if is_space {
            continue;
        }
        let mut text = String::new();
        cell.push_text(&mut text);
        run = Some(Run { start: x as u16, text, style });
    }
    flush(&mut items, run.take());
    items
}

fn quad_bounds(x: f32, y: f32, w: f32, h: f32) -> Bounds<Pixels> {
    Bounds { origin: point(px(x), px(y)), size: size(px(w), px(h)) }
}

impl PaintState {
    fn cursor(&self, spans_hint: &Palette) -> Option<CursorPaint> {
        let c = self.frame.cursor;
        let (col, row) = c.pos.filter(|_| c.visible)?;
        if self.preedit.is_some() {
            return None;
        }
        let blinking_off = self.focused && c.blinking && !self.blink_on;
        if blinking_off {
            return None;
        }
        let style = if self.focused { c.style } else { CursorStyle::BlockHollow };
        let color = hsla_of(self.frame.colors.cursor.unwrap_or(spans_hint.fg)).opacity(self.settings.cursor_opacity);
        let text = self.frame.colors.cursor_text.unwrap_or(spans_hint.bg);
        let (col, width) = if c.wide_tail { (col.saturating_sub(1), 2) } else { (col, 1) };
        Some(CursorPaint { col, row, width, style, color, text })
    }

    pub fn paint(&mut self, bounds: Bounds<Pixels>, metrics: Metrics, window: &mut Window, cx: &mut App) {
        let palette = Palette::new(&self.frame.colors);
        let settings = self.settings.clone();
        let (cw, ch) = (f32::from(metrics.cell_width), f32::from(metrics.cell_height));
        let origin = self.origin;
        let (ox, oy) = (f32::from(origin.x), f32::from(origin.y));
        let cols = self.frame.cols;

        let mut bg = hsla_of(palette.bg);
        bg.a = settings.background_opacity;
        window.paint_quad(fill(bounds, bg));

        let spans = spans_for_rows(&self.frame, &self.matches, self.current_match);
        let cursor = self.cursor(&palette);
        let block = cursor.filter(|c| c.style == CursorStyle::Block);
        let inputs = RowInputs {
            palette: &palette,
            colors: &self.frame.colors,
            settings: &settings,
            fonts: &self.fonts,
            metrics,
            font_size: px(self.font_size),
            cursor_text: block.map(|c| (c.col, c.text)),
        };

        let nrows = self.frame.rows.len();
        if self.painter.rows.len() != nrows {
            self.painter.rows.clear();
            self.painter.rows.resize_with(nrows, || None);
        }
        for y in 0..nrows {
            let row = &self.frame.rows[y];
            let row_inputs = RowInputs {
                cursor_text: inputs.cursor_text.filter(|_| block.is_some_and(|c| usize::from(c.row) == y)),
                ..inputs
            };
            let key = row_key(row, &spans[y], &row_inputs, self.epoch);
            let fresh = self.painter.rows[y].as_ref().is_some_and(|e| e.key == key);
            if !fresh {
                let items = build_row(row, &spans[y], &row_inputs, window);
                self.painter.rows[y] = Some(RowEntry { key, items });
                self.rows_built += 1;
            }
        }

        let cell_x = |col: u16| ox + cw * f32::from(col);
        let cell_y = |row: usize| oy + ch * row as f32;

        for (y, entry) in self.painter.rows.iter().enumerate() {
            let Some(entry) = entry else { continue };
            for (start, end, color) in &entry.items.bgs {
                let x0 = cell_x(*start);
                window.paint_quad(fill(quad_bounds(x0, cell_y(y), cw * f32::from(end - start), ch), *color));
            }
        }

        if let Some(c) = cursor {
            self.paint_cursor(c, origin, metrics, window);
        }

        for (y, entry) in self.painter.rows.iter().enumerate() {
            let Some(entry) = entry else { continue };
            let top = cell_y(y);
            for (col, line) in &entry.items.texts {
                let _ = line.paint(
                    point(px(cell_x(*col)), px(top + f32::from(metrics.text_dy))),
                    metrics.cell_height,
                    TextAlign::Left,
                    None,
                    window,
                    cx,
                );
            }
            for g in &entry.items.glyphs {
                paint_shapes(window, &g.shapes, point(px(cell_x(g.col)), px(top)), g.color);
            }
            for d in &entry.items.decos {
                paint_deco(window, d, cell_x(d.start), top, cw, metrics);
            }
        }

        if let Some(link) = &self.link {
            let color = hsla_of(palette.fg);
            for span in &link.spans {
                let y = cell_y(usize::from(span.row)) + f32::from(metrics.underline_y);
                let x0 = cell_x(span.start);
                window.paint_quad(fill(
                    quad_bounds(x0, y, cw * f32::from(span.end + 1 - span.start), f32::from(metrics.underline_thickness)),
                    color,
                ));
            }
        }

        if let Some(preedit) = &self.preedit {
            self.paint_preedit(preedit, metrics, &palette, window, cx);
        }

        self.paint_scrollbar(bounds, metrics, window);

        if self.dimmed && !self.focused {
            let fill_color = self.settings.unfocused_fill.unwrap_or(palette.bg);
            let mut color = hsla_of(fill_color);
            color.a = 1.0 - self.settings.unfocused_opacity;
            window.paint_quad(fill(bounds, color));
        }
        let _ = cols;
    }

    fn paint_cursor(&self, c: CursorPaint, origin: Point<Pixels>, m: Metrics, window: &mut Window) {
        let (cw, ch) = (f32::from(m.cell_width), f32::from(m.cell_height));
        let x = f32::from(origin.x) + cw * f32::from(c.col);
        let y = f32::from(origin.y) + ch * f32::from(c.row);
        let w = cw * f32::from(c.width);
        let thick = f32::from(m.cursor_thickness).max(1.0);
        // The cursor height is centred in the cell; the block keeps the whole cell.
        let height = f32::from(m.cursor_height).min(ch);
        let top = y + (ch - height) / 2.0;
        let quad = match c.style {
            CursorStyle::Block => fill(quad_bounds(x, y, w, ch), c.color),
            CursorStyle::Bar => fill(quad_bounds(x, top, thick, height), c.color),
            CursorStyle::Underline => fill(quad_bounds(x, top + height - thick, w, thick), c.color),
            CursorStyle::BlockHollow => outline(quad_bounds(x, top, w, height), c.color, BorderStyle::Solid),
        };
        window.paint_quad(quad);
    }

    fn paint_preedit(&self, preedit: &Preedit, m: Metrics, palette: &Palette, window: &mut Window, cx: &mut App) {
        let Some((col, row)) = self.frame.cursor.pos else { return };
        let (cw, ch) = (f32::from(m.cell_width), f32::from(m.cell_height));
        let x = f32::from(self.origin.x) + cw * f32::from(col);
        let y = f32::from(self.origin.y) + ch * f32::from(row);
        let width_cells = preedit.cells();
        window.paint_quad(fill(quad_bounds(x, y, cw * width_cells as f32, ch), hsla_of(palette.bg)));
        let color = hsla_of(palette.fg);
        let mut cell_x = x;
        for c in preedit.text.chars() {
            let w = char_cells(c);
            let mut text = String::new();
            text.push(c);
            let run = TextRun {
                len: text.len(),
                font: self.fonts.regular.clone(),
                color,
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let line =
                window.text_system().shape_line(text.into(), px(self.font_size), &[run], Some(px(cw * w as f32)));
            let _ = line.paint(point(px(cell_x), px(y)), m.cell_height, TextAlign::Left, None, window, cx);
            cell_x += cw * w as f32;
        }
        let thick = f32::from(m.thickness);
        window.paint_quad(fill(quad_bounds(x, y + f32::from(m.underline_y), cw * width_cells as f32, thick), color));
        if let Some((a, b)) = preedit.selected_cells() {
            window.paint_quad(fill(
                quad_bounds(x + cw * a as f32, y + f32::from(m.underline_y), cw * (b - a) as f32, thick * 2.0),
                color,
            ));
        }
        let caret = preedit.caret_cells();
        window.paint_quad(fill(quad_bounds(x + cw * caret as f32, y, f32::from(m.thickness).max(1.0), ch), color));
    }

    fn paint_scrollbar(&self, bounds: Bounds<Pixels>, m: Metrics, window: &mut Window) {
        if self.scroller_opacity <= 0.0 {
            return;
        }
        let bar = self.frame.scrollbar;
        let track = f32::from(bounds.size.height);
        let Some((top, height)) = scroll::thumb(track, bar.total, bar.offset, bar.len, SCROLLBAR_MIN_THUMB) else {
            return;
        };
        let width = if self.scroller_hover { SCROLLBAR_HOVER_WIDTH } else { SCROLLBAR_WIDTH };
        let x = f32::from(bounds.right()) - width - SCROLLBAR_INSET;
        let mut color = hsla_of(self.frame.colors.foreground);
        color.a = if self.scroller_hover { 0.55 } else { 0.4 } * self.scroller_opacity;
        if self.scroller_hover {
            let mut track_color = hsla_of(self.frame.colors.foreground);
            track_color.a = 0.08 * self.scroller_opacity;
            window.paint_quad(fill(
                quad_bounds(x - 2.0, f32::from(bounds.origin.y), width + 4.0 + SCROLLBAR_INSET, track),
                track_color,
            ));
        }
        let thumb = quad_bounds(x, f32::from(bounds.origin.y) + top, width, height);
        window.paint_quad(quad(thumb, px(width / 2.0), color, 0.0, transparent_black(), BorderStyle::default()));
        let _ = m;
    }
}

pub const SCROLLBAR_WIDTH: f32 = 7.0;
pub const SCROLLBAR_HOVER_WIDTH: f32 = 10.0;
pub const SCROLLBAR_INSET: f32 = 2.0;
pub const SCROLLBAR_MIN_THUMB: f32 = 24.0;
/// Width of the pointer zone that grabs the scrollbar.
pub const SCROLLBAR_HIT_WIDTH: f32 = 16.0;

fn paint_shapes(window: &mut Window, shapes: &[Shape], origin: Point<Pixels>, color: Hsla) {
    let at = |x: f32, y: f32| point(origin.x + px(x), origin.y + px(y));
    for shape in shapes {
        match shape {
            Shape::Rect { x0, y0, x1, y1, alpha } => {
                let mut c = color;
                c.a *= alpha;
                window.paint_quad(fill(Bounds { origin: at(*x0, *y0), size: size(px(x1 - x0), px(y1 - y0)) }, c));
            }
            Shape::Fill(points) => {
                let mut b = PathBuilder::fill();
                for (i, (x, y)) in points.iter().enumerate() {
                    if i == 0 {
                        b.move_to(at(*x, *y));
                    } else {
                        b.line_to(at(*x, *y));
                    }
                }
                b.close();
                if let Ok(path) = b.build() {
                    window.paint_path(path, color);
                }
            }
            Shape::Stroke { points, width } => {
                let mut b = PathBuilder::stroke(px(*width));
                for (i, (x, y)) in points.iter().enumerate() {
                    if i == 0 {
                        b.move_to(at(*x, *y));
                    } else {
                        b.line_to(at(*x, *y));
                    }
                }
                if let Ok(path) = b.build() {
                    window.paint_path(path, color);
                }
            }
        }
    }
}

fn paint_deco(window: &mut Window, d: &Deco, x: f32, top: f32, cw: f32, m: Metrics) {
    let cells = f32::from(d.end - d.start);
    let width = cw * cells;
    match &d.kind {
        DecoKind::Strike => {
            let t = f32::from(m.strike_thickness);
            window.paint_quad(fill(quad_bounds(x, top + f32::from(m.strike_y), width, t), d.color));
        }
        DecoKind::Overline => {
            let t = f32::from(m.overline_thickness);
            window.paint_quad(fill(quad_bounds(x, top + f32::from(m.overline_y), width, t), d.color));
        }
        DecoKind::Underline(kind) => {
            let t = f32::from(m.underline_thickness);
            let y = top + f32::from(m.underline_y);
            match kind {
                UnderlineKind::None => {}
                UnderlineKind::Single => window.paint_quad(fill(quad_bounds(x, y, width, t), d.color)),
                UnderlineKind::Double => {
                    window.paint_quad(fill(quad_bounds(x, y - t, width, t), d.color));
                    window.paint_quad(fill(quad_bounds(x, y + t, width, t), d.color));
                }
                UnderlineKind::Dotted => {
                    let step = t * 2.0;
                    let count = (width / step).floor().max(1.0) as i32;
                    let slack = (width - step * count as f32) / 2.0;
                    for i in 0..count {
                        window.paint_quad(fill(quad_bounds(x + slack + step * i as f32, y, t, t), d.color));
                    }
                }
                UnderlineKind::Dashed => {
                    for i in 0..(d.end - d.start) {
                        let cell = x + cw * f32::from(i);
                        let dash = cw / 3.0;
                        window.paint_quad(fill(quad_bounds(cell + dash / 2.0, y, dash, t), d.color));
                        window.paint_quad(fill(quad_bounds(cell + dash * 1.5 + dash / 2.0, y, dash, t), d.color));
                    }
                }
                UnderlineKind::Curly => {
                    window.paint_underline(
                        point(px(x), px(y - t)),
                        px(width),
                        &UnderlineStyle { thickness: px(t), color: Some(d.color), wavy: true },
                    );
                }
            }
        }
    }
}
