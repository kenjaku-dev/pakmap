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

use super::chrome::{Glyph, glyph};
use super::layout::{self, RowDensity};
use super::overlays;
use crate::tui::app::{App, Focus, Mode};

pub fn draw_search(f: &mut Frame, app: &App, area: Rect) {
    let pal = app.palette();
    // 0.12 (TUI phase 2): the box title names the region only — focus is the
    // border ring (phase 1), the how-to is the footer/help. No more clipped
    // instruction sentences, no stale two-tab "Tab remover" copy.
    let title = match app.mode {
        Mode::Search => " Search ",
        Mode::Installed => " Filter installed ",
        Mode::Updates => " Filter updates ",
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

/// v0.10.1 phase 3: the results title, lens-honest. Census honesty per the
/// plan — an active needle (`draft-live` or armed) grows the band
/// `— filter 't' sel/view_len` (spinner glyph + mode tag preserved) while
/// every other state keeps the byte-exact v0.10 title.
fn results_title(app: &App) -> String {
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
    let loading = if app.is_loading {
        glyph(app, Glyph::Busy)
    } else {
        ""
    };
    let view = app.view_indices();
    let base = if let Some(needle) = app.active_filter() {
        // Census honesty (plan copy `Results — filter 'fire' 5/120`): the
        // matched rows over the whole population — the seat stays visible
        // in the highlight; out-of-view seats cannot inflate the band.
        format!(
            " Results {mode_tag}{loading} — filter '{needle}' {}/{} ",
            view.len(),
            app.packages.len()
        )
    } else if app.packages.is_empty() {
        format!(" Results {mode_tag}{loading}  ")
    } else {
        format!(
            " Results {mode_tag}{loading} ({}/{}) ",
            app.list_state.selected().map(|i| i + 1).unwrap_or(0),
            app.packages.len()
        )
    };
    // 0.12 (TUI phase 2): the sort ring is visible, not silent. Arrival is the
    // unmarked default (keeps the v0.10 census titles byte-exact); any active
    // sort appends its name so `T` always has a visible effect.
    match app.sort {
        crate::tui::app::SortCycle::Arrival => base,
        crate::tui::app::SortCycle::NameAsc => format!("{base}· sort:name "),
        crate::tui::app::SortCycle::RepoAsc => format!("{base}· sort:repo "),
        crate::tui::app::SortCycle::VersionDesc => format!("{base}· sort:version "),
    }
}

pub fn draw_results(f: &mut Frame, app: &App, area: Rect) {
    let pal = app.palette();
    // 0.12 (TUI phase 3): breakpoints resolve against the whole frame, not
    // the results region (a 24-row terminal is still "comfortable" even
    // though the list box itself is shorter than 20 rows).
    let density = layout::resolve_row_density(&app.config.tui.row_density, f.area());
    let title = results_title(app);
    let view = app.view_indices();
    // 0.12 (TUI phase 1): the list mirrors the search box's focus ring — the
    // focused region carries `border_focused`, the idle one `border_unfocused`,
    // so "where am I typing/acting" is visible without relying on color alone
    // (the search box already swaps the same pair).
    let border = if app.focus == Focus::List {
        pal.border_focused
    } else {
        pal.border_unfocused
    };

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
                Mode::Installed => "Installed — empty filter shows all (o: orphans)",
                Mode::Updates => "Updates — empty shows all (U: upgrade all)",
            }
        } else {
            // 0.12 (TUI phase 2): empty states explain + offer the next action.
            match app.mode {
                Mode::Search => "No results — try fewer words, or F to filter the current list",
                Mode::Installed => "No installed match — clear the filter or Tab to Updates",
                Mode::Updates => "System up to date",
            }
        };
        let p = Paragraph::new(text)
            .style(pal.text_dim)
            .alignment(ratatui::layout::Alignment::Center)
            .wrap(Wrap { trim: true })
            .block(layout::framed(title, app.border_type).border_style(border));
        f.render_widget(p, area);
        return;
    }

    // Phase 4 preview pane: an optional second pane at desktop widths; the
    // plan's fallback contract lives in `split_preview` — off/narrow regions
    // pass-through whole and `i` stays the modal detail surface.
    let (list_area, preview_area) = layout::split_preview(area, app.config.tui.preview_pane);
    let interior = list_area.width.saturating_sub(2) as usize; // 1-cell borders
    // v0.10.1 phase 3: the lens projection is 0 when every row filtered out.
    let selected_view = app.list_state.selected().filter(|_| !view.is_empty());
    if let (Some(pkg), Some(pane)) = (
        selected_view.and_then(|i| app.view_package(i)),
        preview_area,
    ) {
        draw_preview(f, pkg, pane, app);
    }

    // v0.10.1 phase 3: an armed/live lens that filtered every row out.
    if view.is_empty() {
        let text: String = if app.is_loading {
            "Checking the lens against the arriving generation...".into()
        } else {
            format!(
                "No filter matches '{}' — Esc clears, F toggles the lens",
                app.active_filter().unwrap_or("")
            )
        };
        let p = Paragraph::new(text)
            .style(pal.text_dim)
            .alignment(ratatui::layout::Alignment::Center)
            .wrap(Wrap { trim: true })
            .block(layout::framed(title, app.border_type).border_style(border));
        f.render_widget(p, area);
        return;
    }

    let items: Vec<ListItem> = view
        .iter()
        .map(|&pkg_i| {
            // v0.10.1 phase 3: the list renders the LENS projection —
            // `self.packages` stays put; the row construction is verbatim.
            let pkg = &app.packages[pkg_i];
            // Phase 4: fixed origin chip + char-safe caps — the list keeps
            // one column geometry across every row, mode and staged leg.
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
            // Phase 4: the origin signal moves from the old `repo/name`
            // prefix into the fixed-width chip; the name row reads as text
            // (single signal per row, registry-styled).
            let chip = Span::styled(origin_chip(&pkg.repo), repo_style);
            let name = Span::styled(layout::fit(&pkg.name, name_cap(interior)), pal.text);
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
                chip,
                Span::raw(" "),
                name,
                Span::styled(format!(" {}", pkg.version), pal.version),
                aur_extra,
                installed,
            ]);

            // 0.12 (TUI phase 3): compact = one line per row (roughly twice
            // as many packages per screen); comfortable/wide keep the
            // description line.
            if density == RowDensity::Compact {
                return ListItem::new(first);
            }
            // Wide + preview off: the arch/updated column rides the second
            // line so the name/version grid never shifts; when the preview is
            // visible it stays the only fact sheet (no duplicated metadata).
            let meta = if density == RowDensity::Wide && preview_area.is_none() {
                wide_meta(pkg)
            } else {
                String::new()
            };
            let second = if meta.is_empty() {
                Line::from(vec![Span::styled(
                    format!(
                        "  {}",
                        layout::fit(
                            pkg.description.as_deref().unwrap_or("-"),
                            interior.saturating_sub(2)
                        )
                    ),
                    pal.text_dim,
                )])
            } else {
                let mw = 24usize.min(interior / 3);
                let dw = interior.saturating_sub(2 + mw + 3);
                Line::from(vec![Span::styled(
                    format!(
                        "  {} · {}",
                        layout::fit(&meta, mw),
                        layout::fit(pkg.description.as_deref().unwrap_or("-"), dw)
                    ),
                    pal.text_dim,
                )])
            };

            ListItem::new(vec![first, second])
        })
        .collect();

    let marker = app
        .config
        .tui
        .highlight_symbol
        .clone()
        .unwrap_or_else(|| glyph(app, Glyph::Selected).to_string());
    let mut state = app.list_state;

    // 0.12 (TUI phase 5): optional column header — wide rows only, never in
    // compact. The framed block is drawn once; the list gets the inner rows
    // below the header so it stays a single, stateful widget.
    if app.config.tui.column_header && density == RowDensity::Wide {
        let block = layout::framed(title.clone(), app.border_type).border_style(border);
        let inner = block.inner(list_area);
        f.render_widget(block, list_area);
        if inner.height > 0 {
            let header = Line::from(vec![
                Span::styled(layout::fit("repo", CHIP_W), pal.text_dim),
                Span::raw(" "),
                Span::styled(layout::fit("name", name_cap(interior)), pal.text_dim),
                Span::raw(" "),
                Span::styled(layout::fit("version", 12), pal.text_dim),
                Span::raw(" signals"),
            ]);
            f.render_widget(Paragraph::new(header), Rect { height: 1, ..inner });
            let rows = Rect {
                y: inner.y + 1,
                height: inner.height.saturating_sub(1),
                ..inner
            };
            let list = List::new(items)
                .highlight_style(pal.highlight)
                .highlight_symbol(marker);
            f.render_stateful_widget(list, rows, &mut state);
        }
        return;
    }

    let list = List::new(items)
        .block(layout::framed(title, app.border_type).border_style(border))
        .highlight_style(pal.highlight)
        .highlight_symbol(marker);
    f.render_stateful_widget(list, list_area, &mut state);
}

/// 0.12 (TUI phase 3): the wide-only metadata column — `arch` and an
/// absolute `updated` date from the AUR `last_modified` epoch. No wall clock
/// in the render path: the date is a pure function of the timestamp, so
/// snapshots stay deterministic.
fn wide_meta(pkg: &crate::model::Package) -> String {
    let mut out = String::new();
    if let Some(arch) = pkg.arch.as_deref() {
        out.push_str(arch);
    }
    if let Some(ts) = pkg.last_modified {
        if !out.is_empty() {
            out.push_str(" · ");
        }
        out.push_str("upd ");
        out.push_str(&epoch_date(ts));
    }
    out
}

/// Days-from-civil algorithm (Howard Hinnant, public domain) — no date crate,
/// no timezone dependency. `secs` is an Unix timestamp; pre-epoch values are
/// handled by Euclidean division.
fn epoch_date(secs: i64) -> String {
    let z = secs.div_euclid(86_400) + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    if month <= 2 {
        year += 1;
    }
    format!("{year:04}-{:02}-{:02}", month, day)
}

// ---- the phase-4 row vocabulary: chip + caps -------------------------------

/// Width of the fixed origin chip (fits "multilib"; wider sources truncate
/// to the tail marker). The cap is **global**, never per-list: the name/
/// version columns hold the same x whether the repo leg returned 1 row or
/// the AUR leg just landed 40 rows with third-party sources.
const CHIP_W: usize = 8;

/// The fixed-width origin chip: names at/below the cap padded as-is,
/// longer ones keep their distinguishable tail (`...-aur`-style via
/// `layout::tail`) — the full source always stays one glance away in the
/// preview/`i` fact sheet. Cell-count exact, so the column grid never
/// drifts on overflow.
pub(crate) fn origin_chip(repo: &str) -> String {
    let mut out = layout::tail(repo, CHIP_W);
    while UnicodeWidthStr::width(out.as_str()) < CHIP_W {
        out.push(' ');
    }
    out
}

/// Name cap: interior minus the fixed prefix (chip + separator), a minimum
/// version window, and the widest row tail the list can render (aur rows
/// carry `(votes pop)` + the three safety badges ≈ 40 cells) — short-name
/// rows must keep every badge inside the frame, long ones truncate
/// char-safely (`fit`) while the x-grid stays put across modes/stages.
const NAME_MAX: usize = 40;

fn name_cap(interior: usize) -> usize {
    let usable = interior
        .saturating_sub(CHIP_W + 1 + 4) // chip + separator + shortest version
        .saturating_sub(40); // worst-case aur tail
    usable.clamp(8, NAME_MAX)
}

// ---- the phase-4 preview pane ----------------------------------------------

/// The fact-sheet pane: the focused row rendered through the shared v0.10
/// info renderer (`overlays::info_lines`) at the desktop-width split. The
/// pane is read-only furniture — install/remove actions and the `i` modal
/// all live in the keymap, unchanged (plan D2); the registry draws it with
/// the calm unfocused border, since the pane mirrors the list's focus.
fn draw_preview(f: &mut Frame, pkg: &crate::model::Package, area: Rect, app: &App) {
    let pal = app.palette();
    let block = layout::framed(format!(" {} / {} ", pkg.repo, pkg.name), app.border_type)
        .border_style(pal.border_unfocused);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let lines: Vec<Line> = overlays::info_lines(pkg, pal);
    let p = Paragraph::new(lines)
        .wrap(Wrap { trim: true })
        .style(pal.text);
    f.render_widget(p, inner);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Package;
    use tui_input::Input;
    use unicode_width::UnicodeWidthStr;

    /// v0.10.1 phase 3: an active lens grows the census band while every
    /// other state keeps the byte-exact v0.10 title (unfiltered titles
    /// byte-identical pin — the plan's census honesty line).
    #[test]
    fn results_title_census_band_grows_only_with_an_active_filter() {
        fn pkg_of(name: &str) -> Package {
            Package::minimal(name.into(), "1.0".into(), "repo".into())
        }
        let mut app = App::new("".into());
        app.packages = vec![pkg_of("alpha"), pkg_of("bash"), pkg_of("bat")];
        // Unfiltered Search: the exact old string, no band.
        assert_eq!(results_title(&app), " Results  (0/3) ");
        app.list_state.select(Some(1));
        assert_eq!(
            results_title(&app),
            " Results  (2/3) ",
            "unfiltered census byte-stable (the old template's literal spaces)"
        );
        app.mode = Mode::Installed;
        assert_eq!(results_title(&app), " Results installed  (2/3) ");

        // Live narrowing (line open) — seat inside the view; the mode tag's
        // trailing space keeps the old template's double-space quirk.
        app.filter_draft = Some(Input::new("ba".into()));
        app.list_state.select(Some(0));
        assert_eq!(
            results_title(&app),
            " Results installed  — filter 'ba' 2/3 ",
            "matches/population (2 of 3 rows hold 'ba') — the seat is the highlight, not the census"
        );
        app.list_state.select(Some(1));
        assert_eq!(
            results_title(&app),
            " Results installed  — filter 'ba' 2/3 ",
            "moving the seat never moves the census"
        );

        // Nothing matching — the band stays honest (clamped 0 over the
        // population; the out-of-view seat cannot lie).
        app.filter_draft = Some(Input::new("zz".into()));
        assert_eq!(
            results_title(&app),
            " Results installed  — filter 'zz' 0/3 "
        );

        // Armed without the line — the identical band.
        app.filter_draft = None;
        app.filter_armed = Some("zz".into());
        assert_eq!(
            results_title(&app),
            " Results installed  — filter 'zz' 0/3 "
        );
    }

    /// The chip is cell-exact at `CHIP_W` for every repo depth: short names
    /// pad, long ones tail-truncate (keeps the distinguishing `-aur`-style
    /// suffix) — the x-grid the rows share holds on overflow too.
    #[test]
    fn origin_chip_is_cell_exact_under_any_repo() {
        for (repo, expected) in [
            ("aur", "aur     "),
            ("extra", "extra   "),
            ("multilib", "multilib"),
            ("local", "local   "),
            ("flatpak", "flatpak "),
        ] {
            assert_eq!(origin_chip(repo), expected, "verbatim/padded {repo}");
            assert_eq!(origin_chip(repo).width(), CHIP_W);
        }
        for wide in [
            "chaotic-aur",
            "linux-surface",
            "日本語のすごく長いリポジトリ名",
        ] {
            let chip = origin_chip(wide);
            assert_eq!(
                chip.width(),
                CHIP_W,
                "wide {wide} still cell-exact: {chip:?}"
            );
        }
        assert_eq!(origin_chip("chaotic-aur"), "...c-aur");
        assert_eq!(origin_chip(""), "        ");
    }

    /// The cap clamps to the sane band and leaves room for the worst-case
    /// row tail (chip + sep + version + aur badges must fit the frame).
    #[test]
    fn name_cap_reserves_the_row_tail() {
        // 80-col terminal: interior 78 → aur tail budget honored.
        assert_eq!(name_cap(78), 25);
        // Wide desktops pin at NAME_MAX; narrow ones floor-clamp to 8.
        assert_eq!(name_cap(118), NAME_MAX);
        assert_eq!(name_cap(40), 8);
        // Degenerate interiors stay ≥ the 8-cell floor.
        assert_eq!(name_cap(0), 8);
        // 80-col full row: 9 (prefix) + cap + 4 (version) + 40 (aur tail)
        // fills the interior exactly — badges never clip on short names.
        assert_eq!(9 + name_cap(78) + 4 + 40, 78);
    }
}
