use std::ffi::c_void;

use vt_sys as sys;

use crate::{
    ffi::*,
    term::{Scrollbar, Term},
    types::{CellFlags, Colors, CursorStyle, Rgb, Underline, Wide},
};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Cell {
    /// First codepoint of the grapheme cluster; `'\0'` for an empty cell.
    pub ch: char,
    /// Remaining codepoints of the cluster (combining marks, ZWJ sequences).
    pub extra: Option<Box<[char]>>,
    /// Resolved foreground, or `None` for the terminal default.
    pub fg: Option<Rgb>,
    /// Resolved background, or `None` for the terminal default.
    pub bg: Option<Rgb>,
    pub underline_color: Option<Rgb>,
    pub flags: CellFlags,
    pub underline: Underline,
    pub wide: Wide,
}

impl Cell {
    pub fn is_empty(&self) -> bool {
        self.ch == '\0'
    }

    /// Appends the cell's text (nothing for an empty cell).
    pub fn push_text(&self, out: &mut String) {
        if self.ch != '\0' {
            out.push(self.ch);
            if let Some(extra) = &self.extra {
                out.extend(extra.iter());
            }
        }
    }

    /// Foreground and background to paint with the terminal defaults filled
    /// in and `INVERSE` applied. `BOLD`/`FAINT`/`INVISIBLE` are left to the
    /// renderer.
    pub fn colors(&self, default_fg: Rgb, default_bg: Rgb) -> (Rgb, Rgb) {
        let fg = self.fg.unwrap_or(default_fg);
        let bg = self.bg.unwrap_or(default_bg);
        if self.flags.contains(CellFlags::INVERSE) {
            (bg, fg)
        } else {
            (fg, bg)
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Row {
    pub cells: Vec<Cell>,
    /// Set when this row's contents changed in the last update.
    pub dirty: bool,
    /// Inclusive column range selected on this row.
    pub selection: Option<(u16, u16)>,
    /// The line continues on the next row: this row soft-wrapped.
    pub wrapped: bool,
}

impl Row {
    pub fn text(&self) -> String {
        let mut s = String::new();
        for c in &self.cells {
            if c.is_empty() {
                s.push(' ');
            } else {
                c.push_text(&mut s);
            }
        }
        s.truncate(s.trim_end().len());
        s
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cursor {
    /// Viewport position; `None` when the cursor is scrolled out of view.
    pub pos: Option<(u16, u16)>,
    pub visible: bool,
    pub style: CursorStyle,
    pub blinking: bool,
    /// The cursor sits on the right half of a wide character.
    pub wide_tail: bool,
    pub password_input: bool,
}

impl Default for Cursor {
    fn default() -> Self {
        Cursor {
            pos: None,
            visible: true,
            style: CursorStyle::Block,
            blinking: false,
            wide_tail: false,
            password_input: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Dirty {
    #[default]
    Clean,
    Partial,
    Full,
}

/// Owned copy of the visible screen, refreshed incrementally by
/// [`Term::update_frame`]. Keep one per view and reuse it so only dirty rows
/// are re-extracted.
#[derive(Clone, Debug)]
pub struct Frame {
    pub rows: Vec<Row>,
    pub cols: u16,
    pub colors: Colors,
    pub cursor: Cursor,
    pub scrollbar: Scrollbar,
    pub alt_screen: bool,
    /// What changed in the last update.
    pub dirty: Dirty,
}

impl Default for Frame {
    fn default() -> Self {
        Frame {
            rows: Vec::new(),
            cols: 0,
            colors: Colors::new(Rgb::new(0xdd, 0xdd, 0xdd), Rgb::new(0x11, 0x11, 0x11)),
            cursor: Cursor::default(),
            scrollbar: Scrollbar::default(),
            alt_screen: false,
            dirty: Dirty::Full,
        }
    }
}

impl Frame {
    pub fn new() -> Frame {
        Frame::default()
    }

    /// Visible text, one line per row, trailing blanks trimmed.
    pub fn text(&self) -> String {
        self.rows
            .iter()
            .map(Row::text)
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl Term {
    /// Snapshots the visible screen into `frame`, re-reading only rows the
    /// terminal marked dirty. Returns whether anything changed.
    ///
    /// While a program holds synchronized output (mode 2026) the previously
    /// captured frame keeps being presented; a hold longer than one second is
    /// released.
    pub fn update_frame(&mut self, frame: &mut Frame) -> bool {
        use sys::GhosttyRenderStateData as D;

        if !self.expire_render_hold() {
            unsafe {
                check(
                    sys::ghostty_render_state_update(self.state, self.raw),
                    "render_state_update",
                )
            };
        }

        let scrollbar = self.scrollbar();
        let alt_screen = self.is_alt_screen();
        let dirty = match state_get::<sys::GhosttyRenderStateDirty::Type>(
            self.state,
            D::GHOSTTY_RENDER_STATE_DATA_DIRTY,
        ) {
            Some(sys::GhosttyRenderStateDirty::GHOSTTY_RENDER_STATE_DIRTY_FULL) => Dirty::Full,
            Some(sys::GhosttyRenderStateDirty::GHOSTTY_RENDER_STATE_DIRTY_PARTIAL) => {
                Dirty::Partial
            }
            _ => Dirty::Clean,
        };

        let size = self.size;
        let resized = frame.cols != size.cols || frame.rows.len() != size.rows as usize;
        if resized {
            frame.cols = size.cols;
            frame.rows.resize_with(size.rows as usize, Row::default);
        }
        let side_changed = frame.scrollbar != scrollbar || frame.alt_screen != alt_screen;
        frame.scrollbar = scrollbar;
        frame.alt_screen = alt_screen;

        // The cursor can change (style, blink, position) without the render
        // state being marked dirty, so it is read on every update.
        let mut cursor = sized::<sys::GhosttyRenderStateCursor>(|c, n| c.size = n);
        let ok = unsafe {
            sys::ghostty_render_state_get(
                self.state,
                D::GHOSTTY_RENDER_STATE_DATA_CURSOR as _,
                &mut cursor as *mut _ as *mut c_void,
            )
        };
        let before = frame.cursor;
        if ok == OK {
            use sys::GhosttyRenderStateCursorVisualStyle as V;
            frame.cursor = Cursor {
                pos: cursor
                    .viewport_has_value
                    .then_some((cursor.viewport_x, cursor.viewport_y)),
                visible: cursor.visible,
                style: match cursor.visual_style {
                    V::GHOSTTY_RENDER_STATE_CURSOR_VISUAL_STYLE_BAR => CursorStyle::Bar,
                    V::GHOSTTY_RENDER_STATE_CURSOR_VISUAL_STYLE_UNDERLINE => CursorStyle::Underline,
                    V::GHOSTTY_RENDER_STATE_CURSOR_VISUAL_STYLE_BLOCK_HOLLOW => {
                        CursorStyle::BlockHollow
                    }
                    _ => CursorStyle::Block,
                },
                blinking: cursor.blinking,
                wide_tail: cursor.wide_tail,
                password_input: cursor.password_input,
            };
        }

        if dirty == Dirty::Clean && !resized {
            frame.dirty = Dirty::Clean;
            for row in &mut frame.rows {
                row.dirty = false;
            }
            return side_changed || frame.cursor != before;
        }

        let mut colors = sized::<sys::GhosttyRenderStateColors>(|c, n| c.size = n);
        unsafe {
            sys::ghostty_render_state_get(
                self.state,
                D::GHOSTTY_RENDER_STATE_DATA_COLORS as _,
                &mut colors as *mut _ as *mut c_void,
            );
        }
        frame.colors.foreground = colors.foreground.into();
        frame.colors.background = colors.background.into();
        frame.colors.cursor_text = self.cursor_text;
        frame.colors.selection_foreground = self.selection_foreground;
        frame.colors.selection_background = self.selection_background;
        for (dst, src) in frame.colors.palette.iter_mut().zip(colors.palette.iter()) {
            *dst = (*src).into();
        }
        frame.colors.cursor = colors.cursor_has_value.then(|| colors.cursor.into());

        let full = dirty == Dirty::Full || resized;
        unsafe {
            use sys::GhosttyRenderStateRowData as R;
            check(
                sys::ghostty_render_state_get(
                    self.state,
                    D::GHOSTTY_RENDER_STATE_DATA_ROW_ITERATOR as _,
                    &mut self.row_iter as *mut _ as *mut c_void,
                ),
                "get row iterator",
            );
            let mut y = 0usize;
            while sys::ghostty_render_state_row_iterator_next(self.row_iter) && y < frame.rows.len()
            {
                let row = &mut frame.rows[y];
                let mut row_dirty = false;
                sys::ghostty_render_state_row_get(
                    self.row_iter,
                    R::GHOSTTY_RENDER_STATE_ROW_DATA_DIRTY as _,
                    &mut row_dirty as *mut _ as *mut c_void,
                );
                row.dirty = full || row_dirty;

                let mut sel = sized::<sys::GhosttyRenderStateRowSelection>(|s, n| s.size = n);
                let r = sys::ghostty_render_state_row_get(
                    self.row_iter,
                    R::GHOSTTY_RENDER_STATE_ROW_DATA_SELECTION as _,
                    &mut sel as *mut _ as *mut c_void,
                );
                row.selection = (r == OK).then_some((sel.start_x, sel.end_x));

                let mut raw: sys::GhosttyRow = 0;
                sys::ghostty_render_state_row_get(
                    self.row_iter,
                    R::GHOSTTY_RENDER_STATE_ROW_DATA_RAW as _,
                    &mut raw as *mut _ as *mut c_void,
                );
                let mut wrapped = false;
                sys::ghostty_row_get(
                    raw,
                    sys::GhosttyRowData::GHOSTTY_ROW_DATA_WRAP as _,
                    &mut wrapped as *mut _ as *mut c_void,
                );
                row.wrapped = wrapped;

                if row.dirty {
                    sys::ghostty_render_state_row_get(
                        self.row_iter,
                        R::GHOSTTY_RENDER_STATE_ROW_DATA_CELLS as _,
                        &mut self.row_cells as *mut _ as *mut c_void,
                    );
                    read_cells(self.row_cells, row, frame.cols, &colors.palette);
                }
                y += 1;
            }
        }
        frame.dirty = dirty;
        unsafe {
            check(
                sys::ghostty_render_state_clean(self.state),
                "render_state_clean",
            )
        };
        true
    }
}

unsafe fn read_cells(
    cells: sys::GhosttyRenderStateRowCells,
    row: &mut Row,
    cols: u16,
    palette: &[sys::GhosttyColorRgb; 256],
) {
    use sys::GhosttyRenderStateRowCellsData as C;
    row.cells.clear();
    row.cells.reserve(cols as usize);
    let mut buf = [0u32; 16];
    let mut big: Vec<u32> = Vec::new();

    while sys::ghostty_render_state_row_cells_next(cells) {
        let mut cell = Cell::default();

        let mut len: u32 = 0;
        sys::ghostty_render_state_row_cells_get(
            cells,
            C::GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_GRAPHEMES_LEN as _,
            &mut len as *mut _ as *mut c_void,
        );
        if len > 0 {
            let cps: &[u32] = if len as usize <= buf.len() {
                sys::ghostty_render_state_row_cells_get(
                    cells,
                    C::GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_GRAPHEMES_BUF as _,
                    buf.as_mut_ptr() as *mut c_void,
                );
                &buf[..len as usize]
            } else {
                big.resize(len as usize, 0);
                sys::ghostty_render_state_row_cells_get(
                    cells,
                    C::GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_GRAPHEMES_BUF as _,
                    big.as_mut_ptr() as *mut c_void,
                );
                &big
            };
            let to_char = |c: u32| char::from_u32(c).unwrap_or(char::REPLACEMENT_CHARACTER);
            cell.ch = to_char(cps[0]);
            if cps.len() > 1 {
                cell.extra = Some(cps[1..].iter().map(|&c| to_char(c)).collect());
            }
        }

        let color = |kind: C::Type| {
            let mut c = sys::GhosttyColorRgb::default();
            let r = sys::ghostty_render_state_row_cells_get(
                cells,
                kind as _,
                &mut c as *mut _ as *mut c_void,
            );
            (r == OK).then(|| Rgb::from(c))
        };
        cell.fg = color(C::GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_FG_COLOR);
        cell.bg = color(C::GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_BG_COLOR);

        let mut styled = false;
        sys::ghostty_render_state_row_cells_get(
            cells,
            C::GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_HAS_STYLING as _,
            &mut styled as *mut _ as *mut c_void,
        );
        if styled {
            let mut style = sized::<sys::GhosttyStyle>(|s, n| s.size = n);
            let r = sys::ghostty_render_state_row_cells_get(
                cells,
                C::GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_STYLE as _,
                &mut style as *mut _ as *mut c_void,
            );
            if r == OK {
                let mut f = CellFlags::default();
                for (on, flag) in [
                    (style.bold, CellFlags::BOLD),
                    (style.italic, CellFlags::ITALIC),
                    (style.faint, CellFlags::FAINT),
                    (style.blink, CellFlags::BLINK),
                    (style.inverse, CellFlags::INVERSE),
                    (style.invisible, CellFlags::INVISIBLE),
                    (style.strikethrough, CellFlags::STRIKETHROUGH),
                    (style.overline, CellFlags::OVERLINE),
                ] {
                    if on {
                        f |= flag;
                    }
                }
                cell.flags = f;
                cell.underline = Underline::from_raw(style.underline as _);
                cell.underline_color = match style.underline_color.tag {
                    sys::GhosttyStyleColorTag::GHOSTTY_STYLE_COLOR_RGB => {
                        Some(style.underline_color.value.rgb.into())
                    }
                    sys::GhosttyStyleColorTag::GHOSTTY_STYLE_COLOR_PALETTE => {
                        Some(palette[style.underline_color.value.palette as usize].into())
                    }
                    _ => None,
                };
            }
        }

        let mut raw: sys::GhosttyCell = 0;
        sys::ghostty_render_state_row_cells_get(
            cells,
            C::GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_RAW as _,
            &mut raw as *mut _ as *mut c_void,
        );
        let mut wide = sys::GhosttyCellWide::GHOSTTY_CELL_WIDE_NARROW;
        sys::ghostty_cell_get(
            raw,
            sys::GhosttyCellData::GHOSTTY_CELL_DATA_WIDE as _,
            &mut wide as *mut _ as *mut c_void,
        );
        cell.wide = match wide {
            sys::GhosttyCellWide::GHOSTTY_CELL_WIDE_WIDE => Wide::Wide,
            sys::GhosttyCellWide::GHOSTTY_CELL_WIDE_SPACER_TAIL => Wide::SpacerTail,
            sys::GhosttyCellWide::GHOSTTY_CELL_WIDE_SPACER_HEAD => Wide::SpacerHead,
            _ => Wide::Narrow,
        };
        let mut link = false;
        sys::ghostty_cell_get(
            raw,
            sys::GhosttyCellData::GHOSTTY_CELL_DATA_HAS_HYPERLINK as _,
            &mut link as *mut _ as *mut c_void,
        );
        if link {
            cell.flags |= CellFlags::HYPERLINK;
        }

        row.cells.push(cell);
    }
}
