use std::ffi::c_void;

use vt_sys as sys;

use crate::{ffi::*, term::Term};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SearchStatus {
    pub total: usize,
    /// Index of the selected match, 0 being the newest.
    pub selected: Option<usize>,
    /// The search has caught up with the terminal contents.
    pub complete: bool,
}

/// A match in viewport cell coordinates, both ends inclusive. May span lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SearchMatch {
    pub start: (u16, u16),
    pub end: (u16, u16),
}

const MAX_TICKS_PER_POLL: usize = 64;

impl Term {
    /// Starts (or restarts) a case-insensitive search over the screen and
    /// scrollback. An empty needle clears it. Call [`search_poll`](Self::search_poll)
    /// to make progress.
    pub fn search_set(&mut self, needle: &str) {
        if self.search.is_none() {
            if needle.is_empty() {
                return;
            }
            let mut s = std::ptr::null_mut();
            let r = unsafe { sys::ghostty_search_new(std::ptr::null(), &mut s, self.raw) };
            if r != OK {
                return;
            }
            self.search = Some(s);
        }
        let s = self.search.unwrap();
        let value = sys::GhosttyString {
            ptr: needle.as_ptr(),
            len: needle.len(),
        };
        unsafe {
            sys::ghostty_search_set(
                s,
                sys::GhosttySearchOption::GHOSTTY_SEARCH_OPT_NEEDLE as _,
                if needle.is_empty() {
                    std::ptr::null()
                } else {
                    &value as *const _ as *const c_void
                },
            );
        }
        if needle.is_empty() {
            self.search_close();
        }
    }

    pub fn search_close(&mut self) {
        if let Some(s) = self.search.take() {
            unsafe { sys::ghostty_search_free(s) };
        }
    }

    pub fn is_searching(&self) -> bool {
        self.search.is_some()
    }

    /// Feeds the search with the latest terminal contents and does a bounded
    /// amount of matching. Call it every frame while the find bar is open.
    pub fn search_poll(&mut self) -> Option<SearchStatus> {
        let s = self.search?;
        unsafe {
            sys::ghostty_search_feed(s);
            let mut status = sys::GhosttySearchStatus::GHOSTTY_SEARCH_STATUS_RUNNING;
            for _ in 0..MAX_TICKS_PER_POLL {
                if sys::ghostty_search_tick(s, &mut status) != OK
                    || status != sys::GhosttySearchStatus::GHOSTTY_SEARCH_STATUS_RUNNING
                {
                    break;
                }
            }
            let mut total = 0usize;
            sys::ghostty_search_get(
                s,
                sys::GhosttySearchData::GHOSTTY_SEARCH_DATA_TOTAL_MATCHES as _,
                &mut total as *mut _ as *mut c_void,
            );
            let mut idx = 0usize;
            let has_sel = sys::ghostty_search_get(
                s,
                sys::GhosttySearchData::GHOSTTY_SEARCH_DATA_SELECTED_INDEX as _,
                &mut idx as *mut _ as *mut c_void,
            ) == OK;
            Some(SearchStatus {
                total,
                selected: has_sel.then_some(idx),
                complete: status == sys::GhosttySearchStatus::GHOSTTY_SEARCH_STATUS_COMPLETE,
            })
        }
    }

    /// Selects the next match toward older output, scrolling it into view.
    pub fn search_next(&mut self) -> bool {
        self.search_step(sys::GhosttySearchOption::GHOSTTY_SEARCH_OPT_SELECT_NEXT)
    }

    /// Selects the previous match toward newer output, scrolling it into view.
    pub fn search_prev(&mut self) -> bool {
        self.search_step(sys::GhosttySearchOption::GHOSTTY_SEARCH_OPT_SELECT_PREV)
    }

    fn search_step(&mut self, opt: sys::GhosttySearchOption::Type) -> bool {
        let Some(s) = self.search else { return false };
        unsafe { sys::ghostty_search_set(s, opt as _, std::ptr::null()) == OK }
    }

    /// Matches currently visible, for drawing highlights.
    pub fn search_viewport_matches(&self) -> Vec<SearchMatch> {
        let Some(s) = self.search else {
            return Vec::new();
        };
        let Some(sels) = (unsafe { read_selections(s) }) else {
            return Vec::new();
        };
        sels.iter()
            .filter_map(|sel| self.match_in_viewport(sel))
            .collect()
    }

    /// The selected match, if it is visible.
    pub fn search_selected_match(&self) -> Option<SearchMatch> {
        let s = self.search?;
        let mut sel = sized::<sys::GhosttySelection>(|s, n| s.size = n);
        let r = unsafe {
            sys::ghostty_search_get(
                s,
                sys::GhosttySearchData::GHOSTTY_SEARCH_DATA_SELECTED_MATCH as _,
                &mut sel as *mut _ as *mut c_void,
            )
        };
        if r != OK {
            return None;
        }
        self.match_in_viewport(&sel)
    }

    fn match_in_viewport(&self, sel: &sys::GhosttySelection) -> Option<SearchMatch> {
        let point = |r: &sys::GhosttyGridRef| {
            let mut c = sys::GhosttyPointCoordinate::default();
            let res = unsafe {
                sys::ghostty_terminal_point_from_grid_ref(
                    self.raw,
                    r,
                    sys::GhosttyPointTag::GHOSTTY_POINT_TAG_VIEWPORT as _,
                    &mut c,
                )
            };
            (res == OK && c.y < self.size.rows as u32).then_some((c.x, c.y as u16))
        };
        Some(SearchMatch {
            start: point(&sel.start)?,
            end: point(&sel.end)?,
        })
    }
}

unsafe fn read_selections(s: sys::GhosttySearch) -> Option<Vec<sys::GhosttySelection>> {
    let key = sys::GhosttySearchData::GHOSTTY_SEARCH_DATA_VIEWPORT_MATCHES as _;
    let mut buf = sys::GhosttySelectionBuffer {
        ptr: std::ptr::null_mut(),
        cap: 0,
        len: 0,
    };
    let r = sys::ghostty_search_get(s, key, &mut buf as *mut _ as *mut c_void);
    if r == OK && buf.len == 0 {
        return Some(Vec::new());
    }
    if r != sys::GhosttyResult::GHOSTTY_OUT_OF_SPACE {
        return None;
    }
    let mut out = vec![sized::<sys::GhosttySelection>(|s, n| s.size = n); buf.len];
    let mut buf = sys::GhosttySelectionBuffer {
        ptr: out.as_mut_ptr(),
        cap: out.len(),
        len: 0,
    };
    if sys::ghostty_search_get(s, key, &mut buf as *mut _ as *mut c_void) != OK {
        return None;
    }
    out.truncate(buf.len);
    Some(out)
}
