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

use crate::tui::app::{App, Mode, StagedLegs};
use crate::tui::style::ThemeStyles;
use unicode_width::UnicodeWidthStr;

/// The status strip's spinner alphabet (phase 4): the classic braille walk,
/// one phase step per 240 ms against the `StagedLegs.started` anchor — a
/// pure function of elapsed time, so no frame counter exists anywhere; the
/// in-flight redraw cadence (about 6.7 fps during live legs) advances it.
pub(crate) const SPINNER_FRAMES: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

/// 0.12 (TUI phase 1): the plain-ASCII spinner walk used when `tui.icons`
/// resolves to ascii — same cadence, no braille.
pub(crate) const ASCII_SPINNER_FRAMES: &[char] = &['|', '/', '-', '\\'];

/// 0.12 (TUI phase 1): pure glyph-set resolver. `ascii`/`unicode` force a set;
/// `auto` falls back to ASCII when the session looks ASCII-only (NO_COLOR via
/// `no_color`, TERM=linux console, LC_ALL=C/POSIX). Env values are passed in so
/// the decision is unit-testable without touching the process environment.
pub(crate) fn ascii_icons(icons: &str, no_color: bool, term: &str, lc_all: &str) -> bool {
    match icons.trim().to_lowercase().as_str() {
        "ascii" => true,
        "unicode" => false,
        _ => no_color || term == "linux" || lc_all == "C" || lc_all == "POSIX",
    }
}

fn use_ascii(app: &App) -> bool {
    ascii_icons(
        &app.config.tui.icons,
        app.no_color,
        &std::env::var("TERM").unwrap_or_default(),
        &std::env::var("LC_ALL").unwrap_or_default(),
    )
}

/// 0.12 (TUI phase 2): the spinner alphabet is honest to `tui.tick_chars` —
/// a non-empty override drives the TUI spinner too (it used to be CLI-only).
/// Otherwise the glyph set decides: ASCII walk vs the braille walk.
fn spinner_frames(app: &App) -> Vec<char> {
    if let Some(t) = app.config.tui.tick_chars.as_deref() {
        let chars: Vec<char> = t.chars().filter(|c| !c.is_whitespace()).collect();
        if !chars.is_empty() {
            return chars;
        }
    }
    if use_ascii(app) {
        ASCII_SPINNER_FRAMES.to_vec()
    } else {
        SPINNER_FRAMES.to_vec()
    }
}

/// Phase step of the braille walk in ms: `spinner_char` divides elapsed by
/// 240; the fresh-anchor unit test relies on the first frame being selected
/// inside that window (documented here rather than duplicated as a const).
#[cfg(test)]
#[allow(dead_code)]
const SPINNER_MS: u128 = 240;

/// The spinner glyph at a wall-clock moment (testable pure pick).
fn spinner_char(started: std::time::Instant, frames: &[char]) -> char {
    let step = started.elapsed().as_millis() / 240;
    frames[(step % frames.len() as u128) as usize]
}

/// The in-flight leg strip text (phase 4): base copy + the planned legs in
/// platform order — arrived legs show their count, pending ones the
/// spinner glyph; un-planned legs render nothing. Pure: `draw_status`
/// calls it every frame so counters/spinner stay live between leg arrivals.
fn legs_status_text(legs: &StagedLegs, spinner: char, mode: Mode) -> String {
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
                // 0.12 (TUI phase 2): a failed leg is a failure, not a zero —
                // the strip marks it ✗ (the reason rides the status line).
                if leg.error.is_some() {
                    format!("{label} ✗")
                } else {
                    format!("{label} {}", leg.count)
                }
            } else {
                format!("{label} {spinner}")
            })
        })
        .collect();
    // v0.10.1 phase 1: mode-honest copy — Search asks ("Searching for
    // 'q'"), Updates checks ("Checking for updates").
    let (verb, query) = if mode == Mode::Updates {
        ("Checking for updates", String::new())
    } else {
        ("Searching", format!("for '{}' ", legs.query))
    };
    format!("{verb} {query}— {}…", parts.join(" · "))
}

/// The responsive-floor warning: the frame cannot draw its regions below
/// `[tui] floor_width`×`floor_height`. 0.12 (TUI phase 3): the copy now says
/// what happens next instead of just refusing — enlarge the terminal (or
/// lower the floor keys) and the full frame returns.
pub fn draw_floor_warning(f: &mut Frame, area: Rect, pal: &ThemeStyles, fw: u16, fh: u16) {
    let msg = Paragraph::new(format!(
        "Terminal too small — need ≥{fw}×{fh}; resize or lower tui.floor_width/height to render"
    ))
    .style(pal.border_error)
    .alignment(ratatui::layout::Alignment::Center)
    .wrap(Wrap { trim: true })
    .block(Block::bordered().title(" pakmap "));
    f.render_widget(msg, area);
}

pub fn draw_tabs(f: &mut Frame, app: &App, area: Rect) {
    let pal = app.palette();
    // 0.12 (TUI phase 1): the glyph set is a config decision (`tui.icons`),
    // not a width heuristic — the old `<60` branch was dead at the default
    // floor. ASCII keeps tabs/spinner readable on consoles and NO_COLOR.
    let ascii = use_ascii(app);
    let titles: Vec<Line> = if ascii {
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
        .divider(if ascii { "|" } else { " │ " });
    // Hint on the right is rendered via status/help; keep tabs single-line, no border (clutter audit).
    f.render_widget(tabs, area);
}

pub fn draw_status(f: &mut Frame, app: &App, area: Rect) {
    let pal = app.palette();
    // Phase 4: while staged legs are in flight the strip composes the live
    // per-leg line (spinner = wall-clock), so counters stay honest between
    // message arrivals; every other state keeps the v0.10 cached string.
    let text = match &app.legs {
        Some(legs) => legs_status_text(
            legs,
            spinner_char(legs.started, &spinner_frames(app)),
            app.mode,
        ),
        None => app.status.clone(),
    };
    let style = match app.status_kind {
        crate::tui::app::StatusKind::Success => pal.status_success,
        crate::tui::app::StatusKind::Warn => pal.status_warn,
        crate::tui::app::StatusKind::Error => pal.status_error,
        crate::tui::app::StatusKind::Info if app.is_loading => pal.status_loading,
        crate::tui::app::StatusKind::Info => pal.status_idle,
    };
    let line = Line::from(vec![Span::styled(text, style)]);
    let p = Paragraph::new(line);
    f.render_widget(p, area);
}

/// v0.10.1 phase 3: the inline results-filter line (one borderless row
/// under the search box while the lens line is open — mirrors the status
/// line's no-block style; height-1 rows cannot carry borders). The cursor
/// is the line's own (the search box cursor only draws while the line is
/// closed: focus stays on the list when `F` opens this).
pub fn draw_filter_line(f: &mut Frame, app: &App, area: Rect) {
    let Some(input) = app.filter_draft.as_ref() else {
        return;
    };
    let pal = app.palette();
    let value = input.value();
    let prefix = " filter: ";
    // 0.12 (TUI phase 4): mirror `draw_search` — tail-truncate to the row and
    // place the native cursor at the VISUAL cursor (correct in the middle of
    // CJK/long text). The old fake `▏` at the end lied about the position.
    let available = area.width.saturating_sub(prefix.width() as u16 + 1) as usize;
    let display = super::layout::tail(value, available);
    let disp_w = display.width();
    let hidden = value.width().saturating_sub(disp_w);
    let vis = input.visual_cursor().saturating_sub(hidden).min(disp_w);
    let line = Line::from(vec![
        Span::styled(prefix, pal.text_dim),
        Span::styled(display, pal.text),
    ]);
    f.render_widget(Paragraph::new(line), area);
    let x = area
        .x
        .saturating_add(prefix.width() as u16)
        .saturating_add(vis as u16)
        .min(area.x.saturating_add(area.width.saturating_sub(1)));
    f.set_cursor_position((x, area.y));
}

pub fn draw_help(f: &mut Frame, app: &App, area: Rect) {
    let pal = app.palette();
    // Overhaul phase 3 (plan §3): the legend is GENERATED from the keymap
    // table — `tui::keys::footer_legend` composes the v0.10 strings from
    // `FOOTER_TOKENS`; a drift test pins them byte-exact in `keys.rs`.
    let help = crate::tui::keys::footer_legend_for_width(app.mode, app.focus, area.width as usize);
    let p = Paragraph::new(help).style(pal.help).block(Block::default());
    f.render_widget(p, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::app::{Mode, SearchStage, StagedLegs};

    fn legs_fixture(planned: [bool; 3]) -> StagedLegs {
        let mut legs = StagedLegs::new("firefox".into(), planned);
        legs.apply_leg(SearchStage::Repo, 7);
        legs.apply_leg(SearchStage::Aur, 12);
        legs
    }

    /// Acceptance (phase 4): the strip reports every planned leg — arrived
    /// with counts, pending with the spinner glyph — in platform order, and
    /// omits un-planned ones entirely. Copy is mode-honest (v0.10.1
    /// phase 1): Search asks, Updates checks.
    #[test]
    fn legs_status_text_reports_planned_legs_only() {
        let all = legs_fixture([true, true, true]);
        assert_eq!(
            legs_status_text(&all, '⠇', Mode::Search),
            "Searching for 'firefox' — repo 7 · aur 12 · flatpak ⠇…"
        );
        let fresh = StagedLegs::new("vim".into(), [true, true, true]);
        assert_eq!(
            legs_status_text(&fresh, '⠋', Mode::Search),
            "Searching for 'vim' — repo ⠋ · aur ⠋ · flatpak ⠋…"
        );
        let repo_only = StagedLegs::new("x".into(), [true, false, false]);
        assert_eq!(
            legs_status_text(&repo_only, '⠸', Mode::Search),
            "Searching for 'x' — repo ⠸…"
        );
        // Degenerate: no planned legs still renders a sane strip.
        let none = StagedLegs::new("x".into(), [false, false, false]);
        assert_eq!(
            legs_status_text(&none, '⠋', Mode::Search),
            "Searching for 'x' — …"
        );
    }

    /// Updates mode stages through the same strip but speaks its own verb —
    /// "Searching for ''" would be a lie on the updates queue.
    #[test]
    fn legs_status_text_speaks_updates_verb_in_updates_mode() {
        let all = legs_fixture([true, true, true]);
        assert_eq!(
            legs_status_text(&all, '⠇', Mode::Updates),
            "Checking for updates — repo 7 · aur 12 · flatpak ⠇…"
        );
        let repo_only = StagedLegs::new("x".into(), [true, false, false]);
        assert_eq!(
            legs_status_text(&repo_only, '⠋', Mode::Updates),
            "Checking for updates — repo ⠋…"
        );
        // Installed rows never stage — only Search/Updates reach here.
        assert_eq!(
            legs_status_text(&all, '⠇', Mode::Installed),
            "Searching for 'firefox' — repo 7 · aur 12 · flatpak ⠇…"
        );
    }

    /// The spinner picks a braille frame; a fresh `started` anchor can only
    /// resolve to the first frame (elapsed < 240 ms), so the pick is
    /// deterministic inside the test window.
    #[test]
    fn spinner_char_starts_at_first_frame() {
        let fresh = std::time::Instant::now();
        assert_eq!(spinner_char(fresh, SPINNER_FRAMES), SPINNER_FRAMES[0]);
        assert_eq!(
            spinner_char(fresh, ASCII_SPINNER_FRAMES),
            ASCII_SPINNER_FRAMES[0]
        );
    }

    /// 0.12 (TUI phase 1): the glyph-set resolver is a pure config decision.
    #[test]
    fn ascii_icons_resolver() {
        // Forced modes ignore the environment.
        assert!(ascii_icons("ascii", false, "xterm-256color", "en_US.UTF-8"));
        assert!(!ascii_icons("unicode", true, "linux", "C"));
        // auto: ASCII-only sessions fall back, normal sessions keep Unicode.
        assert!(ascii_icons("auto", true, "xterm", "en_US.UTF-8"));
        assert!(ascii_icons("auto", false, "linux", "en_US.UTF-8"));
        assert!(ascii_icons("auto", false, "xterm", "C"));
        assert!(ascii_icons("auto", false, "xterm", "POSIX"));
        assert!(!ascii_icons("auto", false, "xterm-256color", "en_US.UTF-8"));
        // Unknown spelling behaves like auto.
        assert!(!ascii_icons("bogus", false, "xterm", "en_US.UTF-8"));
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
