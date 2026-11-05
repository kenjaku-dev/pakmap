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

use crate::tui::app::{App, Focus, Mode};
use crate::tui::style::ThemeStyles;

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
    let _ = Mode::Search; // keep import used in narrow builds
}

pub fn draw_status(f: &mut Frame, app: &App, area: Rect) {
    let pal = app.palette();
    let style = if app.is_loading {
        pal.status_loading
    } else {
        pal.status_idle
    };
    let line = Line::from(vec![Span::styled(app.status.clone(), style)]);
    let p = Paragraph::new(line);
    f.render_widget(p, area);
}

pub fn draw_help(f: &mut Frame, app: &App, area: Rect) {
    let pal = app.palette();
    let help = match (app.mode, app.focus) {
        (Mode::Search, Focus::Search) => {
            " Enter:search F5:refresh Esc:list Tab:remover ?:help Ctrl+C:quit "
        }
        (Mode::Search, Focus::List) => {
            " ↑↓/j k:move Enter:install i:info r/F5:refresh S:settings Tab:remover ?:help q:quit "
        }
        (Mode::Installed, Focus::Search) => {
            " Enter:filter o:orphans F5:refresh Esc:list Tab:updates ?:help Ctrl+C:quit "
        }
        (Mode::Installed, Focus::List) => {
            " ↑↓/j k:move Enter/d:remove o:orphans c:clean i:info S:settings Tab:updates ?:help q:quit "
        }
        (Mode::Updates, Focus::Search) => {
            " Enter:filter U:upgrade-all F5:refresh Esc:list Tab:search ?:help Ctrl+C:quit "
        }
        (Mode::Updates, Focus::List) => {
            " ↑↓/j k:move Enter:upgrade-one U:upgrade-all c:clean i:info S:settings Tab:search ?:help q:quit "
        }
    };
    let p = Paragraph::new(help).style(pal.help).block(Block::default());
    f.render_widget(p, area);
}
