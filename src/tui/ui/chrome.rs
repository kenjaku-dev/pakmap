//! Chrome (overhaul phase 2): the fixed frame — the tab strip with its
//! active-tab indicator, the status/origin strip, and the footer help line —
//! plus the too-small floor warning. Text, lines and order are the v0.10
//! ones byte-for-byte; what changed is that these are now *named*
//! components: the active tab is a colored strip state (registry
//! `tab_selected`), the footer is a single help line ordered by importance,
//! and the status strip owns the "what am I looking at" metadata.
//!
//! D1: every style value resolves through the palette (`super::style`).

use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, Paragraph, Tabs, Wrap},
};

use crate::tui::app::{App, StagedLegs};
use crate::tui::style::ThemeStyles;

/// The status strip's spinner alphabet (phase 4): the classic braille walk,
/// one phase step per 240 ms against the `StagedLegs.started` anchor — a
/// pure function of elapsed time, so no frame counter exists anywhere; the
/// in-flight redraw cadence (about 6.7 fps during live legs) advances it.
pub(crate) const SPINNER_FRAMES: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

/// Phase step of the braille walk in ms: `spinner_char` divides elapsed by
/// 240; the fresh-anchor unit test relies on the first frame being selected
/// inside that window (documented here rather than duplicated as a const).
#[cfg(test)]
#[allow(dead_code)]
const SPINNER_MS: u128 = 240;

/// The spinner glyph at a wall-clock moment (testable pure pick).
fn spinner_char(started: std::time::Instant) -> char {
    let step = started.elapsed().as_millis() / 240;
    SPINNER_FRAMES[(step % SPINNER_FRAMES.len() as u128) as usize]
}

/// The in-flight leg strip text (phase 4): base copy + the planned legs in
/// platform order — arrived legs show their count, pending ones the
/// spinner glyph; un-planned legs render nothing. Pure: `draw_status`
/// calls it every frame so counters/spinner stay live between leg arrivals.
fn legs_status_text(legs: &StagedLegs, spinner: char) -> String {
    let labels = ["repo", "aur", "flatpak"];
    let parts: Vec<String> = legs
        .planned
        .iter()
        .zip(legs.legs.iter())
        .zip(labels.iter())
        .filter_map(|((planned, leg), label)| {
            if !planned {
                return None;
            }
            Some(if leg.arrived {
                format!("{label} {}", leg.count)
            } else {
                format!("{label} {spinner}")
            })
        })
        .collect();
    format!("Searching for '{}' — {}…", legs.query, parts.join(" · "))
}

/// The responsive-floor warning: the frame cannot draw its regions below
/// `[tui] floor_width`×`floor_height` (v0.10 copy, verbatim).
pub fn draw_floor_warning(f: &mut Frame, area: Rect, pal: &ThemeStyles, fw: u16, fh: u16) {
    let msg = Paragraph::new(format!("Terminal too small — need ≥{}×{}", fw, fh))
        .style(pal.border_error)
        .alignment(ratatui::layout::Alignment::Center)
        .wrap(Wrap { trim: true })
        .block(Block::bordered().title(" pakmap "));
    f.render_widget(msg, area);
}

pub fn draw_tabs(f: &mut Frame, app: &App, area: Rect) {
    let pal = app.palette();
    let titles: Vec<Line> = if area.width < 60 {
        // Narrow ASCII fallback — no emoji, short labels
        vec![
            Line::from("[1]Search"),
            Line::from("[2]Installed"),
            Line::from("[3]Updates"),
        ]
    } else {
        vec![
            Line::from("🔍 Search (install)"),
            Line::from("🗑 Installed (remove)"),
            Line::from("⬆ Updates"),
        ]
    };
    let tabs = Tabs::new(titles)
        .select(app.mode.tab_index())
        .style(pal.tab_normal)
        .highlight_style(pal.tab_selected)
        .divider(if area.width < 60 { "|" } else { " │ " });
    // Hint on the right is rendered via status/help; keep tabs single-line, no border (clutter audit).
    f.render_widget(tabs, area);
}

pub fn draw_status(f: &mut Frame, app: &App, area: Rect) {
    let pal = app.palette();
    // Phase 4: while staged legs are in flight the strip composes the live
    // per-leg line (spinner = wall-clock), so counters stay honest between
    // message arrivals; every other state keeps the v0.10 cached string.
    let text = match &app.legs {
        Some(legs) => legs_status_text(legs, spinner_char(legs.started)),
        None => app.status.clone(),
    };
    let style = if app.is_loading {
        pal.status_loading
    } else {
        pal.status_idle
    };
    let line = Line::from(vec![Span::styled(text, style)]);
    let p = Paragraph::new(line);
    f.render_widget(p, area);
}

pub fn draw_help(f: &mut Frame, app: &App, area: Rect) {
    let pal = app.palette();
    // Overhaul phase 3 (plan §3): the legend is GENERATED from the keymap
    // table — `tui::keys::footer_legend` composes the v0.10 strings from
    // `FOOTER_TOKENS`; a drift test pins them byte-exact in `keys.rs`.
    let help = crate::tui::keys::footer_legend(app.mode, app.focus);
    let p = Paragraph::new(help).style(pal.help).block(Block::default());
    f.render_widget(p, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::app::{SearchStage, StagedLegs};

    fn legs_fixture(planned: [bool; 3]) -> StagedLegs {
        let mut legs = StagedLegs::new("firefox".into(), planned);
        legs.apply_leg(SearchStage::Repo, 7);
        legs.apply_leg(SearchStage::Aur, 12);
        legs
    }

    /// Acceptance (phase 4): the strip reports every planned leg — arrived
    /// with counts, pending with the spinner glyph — in platform order, and
    /// omits un-planned ones entirely.
    #[test]
    fn legs_status_text_reports_planned_legs_only() {
        let all = legs_fixture([true, true, true]);
        assert_eq!(
            legs_status_text(&all, '⠇'),
            "Searching for 'firefox' — repo 7 · aur 12 · flatpak ⠇…"
        );
        let fresh = StagedLegs::new("vim".into(), [true, true, true]);
        assert_eq!(
            legs_status_text(&fresh, '⠋'),
            "Searching for 'vim' — repo ⠋ · aur ⠋ · flatpak ⠋…"
        );
        let repo_only = StagedLegs::new("x".into(), [true, false, false]);
        assert_eq!(
            legs_status_text(&repo_only, '⠸'),
            "Searching for 'x' — repo ⠸…"
        );
        // Degenerate: no planned legs still renders a sane strip.
        let none = StagedLegs::new("x".into(), [false, false, false]);
        assert_eq!(legs_status_text(&none, '⠋'), "Searching for 'x' — …");
    }

    /// The spinner picks a braille frame; a fresh `started` anchor can only
    /// resolve to the first frame (elapsed < 240 ms), so the pick is
    /// deterministic inside the test window.
    #[test]
    fn spinner_char_starts_at_first_frame() {
        let fresh = std::time::Instant::now();
        assert_eq!(spinner_char(fresh), SPINNER_FRAMES[0]);
    }

    /// Slot mapping sanity: a leg mutation lands in the right bracket
    /// (field-wise — `started` anchors wall-clock, not a comparable facet).
    #[test]
    fn apply_leg_targets_the_right_slot() {
        let mut legs = StagedLegs::new("q".into(), [true, true, true]);
        legs.apply_leg(SearchStage::Flatpak, 3);
        assert!(legs.legs[2].arrived);
        assert_eq!(legs.legs[2].count, 3);
        assert!(!legs.legs[0].arrived);
        assert!(!legs.legs[1].arrived);
        assert_eq!(legs.query, "q");
        assert_eq!(legs.planned, [true, true, true]);
    }
}
