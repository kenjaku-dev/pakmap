//! Body regions (overhaul phase 2): the search/filter input box and the
//! results/installed/updates list — the two regions whose contents respond
//! to state. Geometry and truncation resolve through `super::layout`
//! (`framed`, `tail`); styles resolve through the palette.

use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Span},
    widgets::{List, ListItem, Paragraph, Wrap},
};
use unicode_width::UnicodeWidthStr;

use super::layout;
use crate::tui::app::{App, Focus, Mode};

pub fn draw_search(f: &mut Frame, app: &App, area: Rect) {
    let pal = app.palette();
    let title = match (app.mode, app.focus) {
        (Mode::Search, Focus::Search) => " Search (Enter to search, F5 refresh, Tab remover) ",
        (Mode::Search, Focus::List) => " Search (/ to focus, F5 refresh, Tab remover) ",
        (Mode::Installed, Focus::Search) => {
            " Filter installed (Enter filter, o orphans, F5 refresh, Tab updates) "
        }
        (Mode::Installed, Focus::List) => {
            " Filter installed (/ to focus, o orphans, F5 refresh, Tab updates) "
        }
        (Mode::Updates, Focus::Search) => {
            " Filter updates (Enter filter, U upgrade all, Tab search) "
        }
        (Mode::Updates, Focus::List) => {
            " Filter updates (/ to focus, Enter upgrade one, U upgrade all, Tab search) "
        }
    };
    let style = if app.focus == Focus::Search {
        pal.border_focused
    } else {
        pal.border_unfocused
    };

    let input_str = app.input.value();
    let width = area.width.saturating_sub(4) as usize; // borders + padding
    // width-aware tail truncation via the phase-2 vocabulary (char-boundary
    // safe, verbatim v0.10 walk inside `layout::tail`)
    let input_width = input_str.width();
    let display = layout::tail(input_str, width);

    // Cursor position for tui-input (visual width, not byte index)
    let cursor_pos = app.input.visual_cursor();
    // Width now: `display` moves into the Paragraph below, so measure first.
    let disp_width = display.width();

    let paragraph = Paragraph::new(display)
        .style(pal.text)
        .block(layout::framed(title, app.border_type).border_style(style));

    f.render_widget(paragraph, area);

    // Show cursor when focused
    if app.focus == Focus::Search {
        // tui-input gives visual cursor, but we place native cursor for terminal.
        // When the query is tail-truncated ("...suffix"), shift the cursor left
        // by the hidden width so it tracks the visible text; identical to the
        // old computation when nothing is truncated.
        let hidden = input_width.saturating_sub(disp_width);
        let vis = cursor_pos.saturating_sub(hidden).min(disp_width);
        let x = area.x + 1 + (vis as u16).min(area.width.saturating_sub(3));
        let y = area.y + 1;
        f.set_cursor_position((x, y));
    }
}

pub fn draw_results(f: &mut Frame, app: &App, area: Rect) {
    let pal = app.palette();
    let mode_tag = match app.mode {
        Mode::Search => "",
        Mode::Installed => {
            if app.orphans_only {
                "orphans "
            } else {
                "installed "
            }
        }
        Mode::Updates => "updates ",
    };
    let title = format!(
        " Results {}{} {} ",
        mode_tag,
        if app.is_loading { "⏳" } else { "" },
        if app.packages.is_empty() {
            "".into()
        } else {
            format!(
                "({}/{})",
                app.list_state.selected().map(|i| i + 1).unwrap_or(0),
                app.packages.len()
            )
        }
    );

    if app.packages.is_empty() {
        let text = if app.is_loading {
            match app.mode {
                Mode::Search => "Searching...",
                Mode::Installed => "Listing installed...",
                Mode::Updates => "Checking for updates...",
            }
        } else if app.input.value().trim().is_empty() {
            match app.mode {
                Mode::Search => "Type a package name above and press Enter — e.g. firefox",
                Mode::Installed => {
                    "Installed mode — type to filter, empty shows all (o: orphans, Tab: updates)"
                }
                Mode::Updates => "Updates mode — empty shows all (U: upgrade all, Tab: search)",
            }
        } else {
            match app.mode {
                Mode::Search => "No packages found. Try another query or check filters.",
                Mode::Installed => "No installed packages match. Clear filter or press Tab.",
                Mode::Updates => "No updates match. System up to date.",
            }
        };
        let p = Paragraph::new(text)
            .style(pal.text_dim)
            .alignment(ratatui::layout::Alignment::Center)
            .wrap(Wrap { trim: true })
            .block(layout::framed(title, app.border_type));
        f.render_widget(p, area);
        return;
    }

    let items: Vec<ListItem> = app
        .packages
        .iter()
        .map(|pkg| {
            // mono repo entries are BOLD — the palette covers both paths
            let repo_style = if pkg.repo == "aur" {
                pal.repo_aur
            } else {
                match pkg.repo.as_str() {
                    "core" => pal.repo_core,
                    "extra" => pal.repo_extra,
                    "multilib" => pal.repo_multilib,
                    "local" => pal.repo_local,
                    _ => pal.repo_other,
                }
            };
            // In Installed mode the [installed] tag is noise (all are installed) — hide it.
            let show_installed_tag = pkg.installed && app.mode == Mode::Search;
            let installed = if show_installed_tag {
                Span::styled(" [installed]", pal.installed)
            } else {
                Span::raw("")
            };
            let aur_extra = if pkg.repo == "aur" {
                let votes = pkg.votes.unwrap_or(0);
                let pop = pkg.popularity.unwrap_or(0.0);
                let mut tags = format!(" (+{} {:.2})", votes, pop);
                for b in pkg.badges() {
                    tags.push(' ');
                    tags.push_str(b);
                }
                Span::styled(tags, pal.votes)
            } else if pkg.orphan {
                Span::styled(" [ORPHAN]", pal.out_of_date)
            } else {
                Span::raw("")
            };

            let first = Line::from(vec![
                Span::styled(format!("{}/{}", pkg.repo, pkg.name), repo_style),
                Span::styled(format!(" {}", pkg.version), pal.version),
                aur_extra,
                installed,
            ]);

            let second = Line::from(vec![Span::styled(
                format!("  {}", pkg.description.as_deref().unwrap_or("-")),
                pal.text_dim,
            )]);

            ListItem::new(vec![first, second])
        })
        .collect();

    let list = List::new(items)
        .block(layout::framed(title, app.border_type))
        .highlight_style(pal.highlight)
        .highlight_symbol(
            app.config
                .tui
                .highlight_symbol
                .clone()
                .unwrap_or_else(|| "▸ ".into()),
        );

    // ListState is stateful per ecosystem-rust.md
    let mut state = app.list_state;
    f.render_stateful_widget(list, area, &mut state);
}
