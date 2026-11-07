//! The TUI layout/truncation vocabulary (overhaul phase 2, plan §3).
//!
//! Every `Rect` the TUI draws is computed here as a **pure function** of
//! `(area, config/state)` — no `Frame`, no widget construction, no styling —
//! so the resize contract is owned by the stress-matrix unit tests at the
//! bottom of this file instead of hope. Truncation lives here too: `fit`
//! (pad-to-column cells) and `tail` (search/detail input tracking) are the
//! single char-boundary-safe truncation points module-wide.
//!
//! D1 (overhaul phase 1): this file moves geometry and carries no ratatui
//! style construction — every `Style` value it accepts is resolved upstream
//! through `src/tui/style.rs`.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    text::Line,
    widgets::{Block, BorderType, Borders},
};

use crate::tui::style::ThemeStyles;

// ---- frame chrome geometry -------------------------------------------------

/// The responsive floor in `(w, h)`: config-provided, never below the
/// 40×10 sanity margin (v0.10 semantics, verbatim).
pub fn floor_required(cfg: &crate::config::TuiConfig) -> (u16, u16) {
    (cfg.floor_width.max(40), cfg.floor_height.max(10))
}

/// `true` when `area` satisfies the floor (the frame renders); otherwise the
/// caller draws the too-small warning via `crate::tui::ui::chrome`.
#[must_use]
pub fn floor_ok(area: Rect, (fw, fh): (u16, u16)) -> bool {
    area.width >= fw && area.height >= fh
}

/// The app frame regions: optional tab strip + search + results + status +
/// help, top to bottom, exactly the v0.10 constraint set.
#[derive(Debug, Clone, Copy)]
pub struct Regions {
    /// `None` when the tab strip is disabled (`[tui] show_tabs = false`).
    pub tabs: Option<Rect>,
    pub search: Rect,
    pub results: Rect,
    pub status: Rect,
    pub help: Rect,
    /// v0.10.1 phase 3: the inline results-filter line (one row under the
    /// search box, `Some` only while the line is open)
    /// — results shrink by exactly this row.
    pub filter: Option<Rect>,
}

/// The frame layout as a pure function of its inputs (search bar height and
/// results minimum are the `[tui]` config knobs, pre-clamped by the caller;
/// `show_filter` adds the one-row lens line under the search box).
pub fn regions(
    area: Rect,
    show_tabs: bool,
    search_h: u16,
    results_min: u16,
    show_filter: bool,
) -> Regions {
    if !show_tabs {
        let mut constraints = vec![
            Constraint::Length(search_h),
            Constraint::Min(results_min),
            Constraint::Length(1), // status
            Constraint::Length(1), // help
        ];
        if show_filter {
            constraints.insert(1, Constraint::Length(1)); // filter line
        }
        let s = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(area);
        return if show_filter {
            Regions {
                tabs: None,
                search: s[0],
                filter: Some(s[1]),
                results: s[2],
                status: s[3],
                help: s[4],
            }
        } else {
            Regions {
                tabs: None,
                search: s[0],
                filter: None,
                results: s[1],
                status: s[2],
                help: s[3],
            }
        };
    }
    let mut constraints = vec![
        Constraint::Length(1), // tabs
        Constraint::Length(search_h),
        Constraint::Min(results_min),
        Constraint::Length(1), // status
        Constraint::Length(1), // help
    ];
    if show_filter {
        constraints.insert(2, Constraint::Length(1)); // filter line under the search box
    }
    let s = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);
    Regions {
        tabs: Some(s[0]),
        search: s[1],
        filter: show_filter.then(|| s[2]),
        results: if show_filter { s[3] } else { s[2] },
        status: if show_filter { s[4] } else { s[3] },
        help: if show_filter { s[5] } else { s[4] },
    }
}

// ---- the preview-pane split (phase 4) --------------------------------------

/// Desktop-width threshold for the optional preview pane: below it the `i`
/// info modal stays the only detail surface (`tui.preview_pane` + narrow
/// terminal = the plan's modal fallback), at/above it the list shares its
/// region with the fact-sheet pane.
pub const PREVIEW_MIN_W: u16 = 100;

/// Split the results region into `(list, preview?)` — horizontally at the
/// desktop threshold, list keeping the left share (62/38 split; the list
/// must stay the primary surface). `enabled=false` or a too-narrow region
/// collapses to `(area, None)`. Pure merge of geometry with config: the
/// caller passes `tui.preview_pane`.
pub fn split_preview(area: Rect, enabled: bool) -> (Rect, Option<Rect>) {
    if !enabled || area.width < PREVIEW_MIN_W {
        return (area, None);
    }
    const PERCENT: u16 = 62;
    let split = ((area.width as u32 * PERCENT as u32) / 100).min(u16::MAX as u32) as u16;
    let split = split.clamp(1, area.width.saturating_sub(1)); // both halves live
    let list = Rect {
        width: split,
        ..area
    };
    let preview = Rect {
        x: area.x + split,
        width: area.width.saturating_sub(split),
        ..area
    };
    (list, Some(preview))
}

/// A centered popup rect as a percentage pair of `r` (was `centered_rect`):
/// cache-free, no Layout alloc — manual centered math. Clamped to the area
/// so oversized percents (>100) can never produce an out-of-bounds rect.
/// u32 math: width * percent can overflow u16 on wide terminals (debug
/// builds panic on overflow).
pub fn popup_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_width = ((r.width as u32 * percent_x as u32) / 100).min(r.width as u32) as u16;
    let popup_height = ((r.height as u32 * percent_y as u32) / 100).min(r.height as u32) as u16;
    let popup_x = r.x + (r.width.saturating_sub(popup_width)) / 2;
    let popup_y = r.y + (r.height.saturating_sub(popup_height)) / 2;
    Rect::new(popup_x, popup_y, popup_width, popup_height)
}

// ---- the borders/spacing vocabulary ----------------------------------------

/// Every *framed* section in the TUI uses exactly one border depth: ALL
/// borders with the config border type (`[tui] border`, default rounded) —
/// this is the single construction point (overhaul plan §3: "one borders/
/// spacing vocabulary module-wide").
pub fn framed<'a>(title: impl Into<Line<'a>>, border_type: BorderType) -> Block<'a> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(border_type)
        .title(title)
}

/// The centered-overlay block vocabulary: ALL borders + the config border
/// type + the popup background from the registry + the dialog's border
/// emphasis (a plain entry or one of the fidelity-divergent helpers). No
/// overlay may assemble its own Block outside this function.
pub fn overlay_block<'a>(
    pal: &ThemeStyles,
    border_type: BorderType,
    title: impl Into<Line<'a>>,
    border_style: ratatui::style::Style,
) -> Block<'a> {
    framed(title, border_type)
        .border_style(border_style)
        .style(pal.popup_bg)
}

// ---- the truncation vocabulary ---------------------------------------------

/// Char-boundary-safe fixed-width fit: truncate to `w` display columns, then
/// pad with spaces to exactly `w` (the search-field truncation discipline,
/// adapted for table-like cells — settings modal rows and lanes).
pub fn fit(s: &str, w: usize) -> String {
    use unicode_width::UnicodeWidthChar;
    let mut out = String::new();
    let mut acc = 0;
    for ch in s.chars() {
        let cw = ch.width().unwrap_or(0);
        if acc + cw > w {
            break;
        }
        acc += cw;
        out.push(ch);
    }
    while acc < w {
        out.push(' ');
        acc += 1;
    }
    out
}

/// Tail-truncate one line to `w` display columns, width-aware and
/// char-boundary safe (`"..."` + the visible tail — the v0.10 search-field
/// walk, verbatim): used by the search input paragraph and the settings
/// detail cursor mirror. Contract the callers rely on: `s` is returned
/// untouched when it fits, or when `w <= 3`.
pub fn tail(s: &str, w: usize) -> String {
    use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
    const ELLIPSIS: usize = 3; // "..."
    if s.width() <= w || w <= ELLIPSIS {
        return s.to_string();
    }
    let target = w.saturating_sub(ELLIPSIS);
    let mut seen = 0usize;
    let mut start_idx = s.len();
    // Walk from end, collect width until target — char-boundary safe
    for (idx, ch) in s.char_indices().rev() {
        let cw = ch.width().unwrap_or(0);
        if seen + cw > target {
            break;
        }
        seen += cw;
        start_idx = idx;
    }
    // start_idx is already at char boundary from char_indices
    format!("...{}", &s[start_idx..])
}

#[cfg(test)]
mod tests {
    use super::*;
    use unicode_width::UnicodeWidthStr;

    fn in_b(rect: Rect, area: Rect, name: &str, w: u16, h: u16) {
        assert!(
            rect.x + rect.width <= area.width && rect.y + rect.height <= area.height,
            "{name} {rect:?} escapes area {w}×{h}"
        );
    }

    fn spans_overlap(a: Rect, b: Rect) -> bool {
        a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height
    }

    /// Acceptance (plan §3): the resize contract is a stress table — min
    /// floor, 2× floor, terminal-width cuts at 40/60/100/200 columns, plus
    /// heights at the floor and double: every region stays inside the frame
    /// and no two regions overlap, with and without the tab strip.
    #[test]
    fn regions_stress_matrix_in_bounds_and_disjoint() {
        let cols = [10u16, 20, 40, 60, 100, 200];
        let rows = [10u16, 12, 24, 48, 100];
        for &w in &cols {
            for &h in &rows {
                for show_tabs in [true, false] {
                    let area = Rect::new(0, 0, w, h);
                    let r = regions(area, show_tabs, 3, 4, w % 2 == 0);
                    for (name, rect) in [
                        ("search", r.search),
                        ("results", r.results),
                        ("status", r.status),
                        ("help", r.help),
                    ] {
                        in_b(rect, area, name, w, h);
                    }
                    if let Some(tabs) = r.tabs {
                        in_b(tabs, area, "tabs", w, h);
                    }
                    let pairs = [
                        (r.search, r.results, "search×results"),
                        (r.results, r.status, "results×status"),
                        (r.status, r.help, "status×help"),
                    ];
                    for (a, b, label) in pairs {
                        assert!(!spans_overlap(a, b), "overlap {label} at {w}×{h}");
                    }
                    if let Some(tabs) = r.tabs {
                        assert!(!spans_overlap(tabs, r.search), "tabs×search {w}×{h}");
                    }
                }
            }
        }
    }

    /// Acceptance (plan §3): no layout panic window from 10×10 to the
    /// maximum sane size — the pure functions are swept over the grid (and
    /// the zero-area degenerate corner).
    #[test]
    fn layout_vocabulary_never_panics_across_the_sweep() {
        for w in 0..=120 {
            for h in 0..=48 {
                let area = Rect::new(0, 0, w, h);
                let r = regions(area, w % 2 == 0, (w % 7) + 1, (h % 5) + 1, w % 3 == 0);
                let _ = (r.search, r.results, r.status, r.help, r.tabs);
                let p = popup_rect((w % 100) + 1, (h % 100) + 1, area);
                assert!(p.width <= w && p.height <= h, "popup escape {w}×{h}");
            }
        }
        let zero = Rect::new(0, 0, 0, 0);
        let r = regions(zero, true, 3, 4, false);
        assert_eq!((r.search.width, r.help.height), (0, 0));
        assert_eq!(popup_rect(60, 30, zero).width, 0);
    }

    /// 2× floor and just-below-floor cuts (the plan's resize table edges):
    /// the floor predicate is exact at both sides of every edge.
    #[test]
    fn floor_edges_render_regions_safely() {
        let (fw, fh) = floor_required(&crate::config::Config::default().tui);
        assert!(fw >= 40 && fh >= 10);
        for (w, h) in [(fw, fh), (fw * 2, fh * 2), (fw - 1, fh - 1)] {
            let area = Rect::new(0, 0, w, h);
            let r = regions(area, true, 3, 4, h % 2 == 0);
            assert!(r.tabs.is_some(), "regions defined below too");
            assert_eq!(
                floor_ok(area, (fw, fh)),
                area.width >= fw && area.height >= fh,
                "floor predicate wrong at {w}×{h}"
            );
        }
    }

    /// `popup_rect` (was `centered_rect`) stays in bounds: oversized percents
    /// clamp, degenerate areas never underflow.
    #[test]
    fn popup_rect_stays_in_bounds() {
        let area = Rect::new(0, 0, 80, 24);
        let p = popup_rect(60, 30, area);
        assert!(p.x + p.width <= area.width);
        assert!(p.y + p.height <= area.height);
        let big = popup_rect(200, 200, area);
        assert_eq!(
            (big.x, big.y, big.width, big.height),
            (0u16, 0u16, 80u16, 24u16)
        );
        let tiny = popup_rect(60, 30, Rect::new(0, 0, 0, 0));
        assert_eq!((tiny.width, tiny.height), (0u16, 0u16));
    }

    /// `split_preview` (phase 4): the disabled/narrow paths return the whole
    /// region unchanged; the enabled desktop path splits so both halves are
    /// non-empty, cover the full width, stay inside the frame — and the
    /// list half keeps the left majority share.
    #[test]
    fn preview_split_is_bounded_and_thresholded() {
        let wide = Rect::new(0, 0, 120, 20);
        let (list, preview) = split_preview(wide, true);
        let p = preview.expect("desktop width splits");
        assert!(list.width >= 60, "list keeps the majority share: {list:?}");
        assert_eq!(list.x, 0);
        assert_eq!(p.x, list.width);
        assert_eq!(list.width + p.width, 120, "halves cover the region");
        assert_eq!(p.height, 20, "full-height right pane");
        // Threshold: exactly-at splits, one below does not.
        let at = Rect::new(0, 0, PREVIEW_MIN_W, 20);
        assert!(split_preview(at, true).1.is_some());
        let below = Rect::new(0, 0, PREVIEW_MIN_W - 1, 20);
        assert!(split_preview(below, true).1.is_none());
        // Disabled / degenerate: untouched pass-through.
        assert!(split_preview(wide, false).1.is_none());
        let (whole, none) = split_preview(Rect::new(0, 0, 0, 0), true);
        assert!(none.is_none() && whole.width == 0);
        // Sweep: no split can panic or mis-place across sizes.
        for w in 0..=140u16 {
            let area = Rect::new(0, 0, w, 10);
            if let Some(p) = split_preview(area, true).1 {
                assert!(p.x + p.width <= w && p.width >= 1, "escape at {w}");
            }
        }
    }

    /// `tail` is char-boundary safe, keeps the visible end, and is a no-op
    /// when the input fits (or width is degenerate); `fit` pads to the exact
    /// column count and truncates rather than wraps.
    #[test]
    fn truncation_vocabulary_is_char_safe() {
        const WIDE: &str = "日本語テスト";
        assert_eq!(tail("abcdef", 6), "abcdef");
        assert_eq!(tail("", 8), "");
        assert_eq!(tail("ab", 4), "ab");
        assert_eq!(tail("abcdef", 5), "...ef"); // target = 2, verbatim walk
        for w in 0..=14 {
            let out = tail(WIDE, w);
            if WIDE.width() > w && w > 3 {
                assert!(
                    out.starts_with("...") && out.width() <= w,
                    "tail at width {w} truncated wrongly: {out:?}"
                );
            } else {
                assert_eq!(out, WIDE, "tail must be a no-op for w<=3 at width {w}");
            }
        }
        for w in 0..=9 {
            assert_eq!(fit("αβγ🎯", w).width(), w, "fit must pad to exactly {w}");
        }
        assert_eq!(fit("abcde", 4), "abcd"); // truncated, never wrapped
    }
}
