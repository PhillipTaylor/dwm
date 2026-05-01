// Per-output dwm state: tags, layouts, pertag.

use smithay::output::Output;
use smithay::utils::{Logical, Rectangle};

use crate::config::{LAYOUTS, LT_TILE, MFACT, SHOWBAR, TAGS, TOPBAR};

pub struct Monitor {
    pub output: Output,
    pub num: i32,

    // Full output geometry, in compositor logical coordinates.
    pub geometry: Rectangle<i32, Logical>,
    // Window area (geometry minus the bar).
    pub work_area: Rectangle<i32, Logical>,

    pub tagset: [u32; 2],
    pub seltags: usize,
    pub sellt: usize,
    pub lt: [usize; 2],
    pub mfact: f32,
    pub showbar: bool,
    pub topbar: bool,
    pub ltsymbol: String,

    // pertag patch state: per-tag layout/mfact/showbar.
    pub curtag: usize,
    pub prevtag: usize,
    pub lts: Vec<usize>,
    pub mfacts: Vec<f32>,
    pub showbars: Vec<bool>,

    // The currently-focused window's surface, if any. We store an opaque
    // counter rather than a Window reference to keep this type Send+Sync.
    pub sel_focus_order: Option<u64>,
}

impl Monitor {
    pub fn new(output: Output, geometry: Rectangle<i32, Logical>, num: i32) -> Self {
        let n_tags = TAGS.len();
        let mut lts = vec![LT_TILE; n_tags + 1];
        lts[0] = LT_TILE;
        let mfacts = vec![MFACT; n_tags + 1];
        let showbars = vec![SHOWBAR; n_tags + 1];

        let mut m = Self {
            output,
            num,
            geometry,
            work_area: geometry,
            tagset: [1, 1],
            seltags: 0,
            sellt: 0,
            lt: [LT_TILE, LT_TILE],
            mfact: MFACT,
            showbar: SHOWBAR,
            topbar: TOPBAR,
            ltsymbol: LAYOUTS[LT_TILE].symbol.to_string(),
            curtag: 1,
            prevtag: 1,
            lts,
            mfacts,
            showbars,
            sel_focus_order: None,
        };
        m.update_work_area(crate::bar::BAR_HEIGHT);
        m
    }

    /// Recompute the work area, removing the bar from the top or bottom.
    pub fn update_work_area(&mut self, bar_height: i32) {
        let mut wa = self.geometry;
        if self.showbar {
            if self.topbar {
                wa.loc.y += bar_height;
                wa.size.h -= bar_height;
            } else {
                wa.size.h -= bar_height;
            }
        }
        self.work_area = wa;
    }

    /// Y coordinate of the bar (in logical coordinates).
    pub fn bar_y(&self) -> i32 {
        if self.topbar {
            self.geometry.loc.y
        } else {
            self.geometry.loc.y + self.geometry.size.h - crate::bar::BAR_HEIGHT
        }
    }

    /// True if a tag bitmask intersects the currently-visible tagset.
    pub fn is_visible_tagset(&self, tags: u32) -> bool {
        tags & self.tagset[self.seltags] != 0
    }

    /// Switch to the next layout, recording state in pertag arrays.
    pub fn set_layout(&mut self, lt: Option<usize>) {
        if let Some(idx) = lt {
            if idx != self.lt[self.sellt] {
                self.sellt ^= 1;
            }
            self.lt[self.sellt] = idx;
        } else {
            self.sellt ^= 1;
        }
        self.lts[self.curtag] = self.lt[self.sellt];
        self.ltsymbol = LAYOUTS[self.lt[self.sellt]].symbol.to_string();
    }
}
