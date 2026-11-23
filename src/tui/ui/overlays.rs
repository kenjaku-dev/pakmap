//! Overlays (overhaul phase 2): the centered popups — package info, the
//! five confirm dialogs, help, message — all speaking one block vocabulary
//! (`layout::overlay_block`: ALL borders + the config border type + the
//! registry popup background + the dialog's border emphasis) and the same
//! `layout::popup_rect` geometry. Content is the v0.10 copy, byte-for-byte.

use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::Style,
    text::{Line, Span, Text},
    widgets::{Clear, Paragraph, Wrap},
};

use super::layout;
use crate::tui::app::{App, Popup};
use crate::tui::style::ThemeStyles;

/// The package fact-sheet lines shared by the `i` modal and the phase-4
/// preview pane (`body::draw_preview`). v0.10 copy, byte-identical; the
/// modal appends the close note after (see `draw_info_popup`), the pane
/// ends at the Optdepends row. All output is `'static` (owned spans), so
/// the panes never fight lifetimes.
pub(crate) fn info_lines(pkg: &crate::model::Package, pal: &ThemeStyles) -> Vec<Line<'static>> {
    vec![
        Line::from(vec![
            Span::styled("Version: ", pal.emphasized(pal.status_loading)),
            Span::raw(pkg.version.clone()),
            Span::raw(format!("  Arch: {}", pkg.arch.as_deref().unwrap_or("-"))),
        ]),
        Line::from(vec![
            Span::styled("Repo: ", pal.emphasized(pal.status_loading)),
            Span::raw(pkg.repo.clone()),
            Span::raw(if pkg.installed { "  [installed]" } else { "" }),
        ]),
        Line::from(vec![
            Span::styled("URL: ", pal.emphasized(pal.status_loading)),
            Span::raw(pkg.url.as_deref().unwrap_or("-").to_string()),
        ]),
        Line::from(vec![
            Span::styled("Desc: ", pal.emphasized(pal.status_loading)),
            Span::raw(pkg.description.as_deref().unwrap_or("-").to_string()),
        ]),
        Line::raw(""),
        Line::from(vec![
            Span::styled("Votes: ", pal.emphasized(pal.votes)),
            Span::raw(format!("{}", pkg.votes.unwrap_or(0))),
            Span::raw(format!(
                "  Popularity: {:.2}",
                pkg.popularity.unwrap_or(0.0)
            )),
            Span::raw(
                pkg.maintainer
                    .as_deref()
                    .map(|m| format!("  Maintainer: {}", m))
                    .unwrap_or_default(),
            ),
        ]),
        Line::from(vec![Span::styled(pkg.badges().join(" "), pal.out_of_date)]),
        Line::from(vec![
            Span::styled("Reason: ", pal.emphasized(pal.status_loading)),
            Span::raw(pkg.reason.as_deref().unwrap_or("-").to_string()),
            Span::raw(
                pkg.required_by
                    .as_ref()
                    .filter(|v| !v.is_empty())
                    .map(|v| {
                        format!(
                            "  Required by: {}",
                            v.iter().take(5).cloned().collect::<Vec<_>>().join(", ")
                        )
                    })
                    .unwrap_or_default(),
            ),
        ]),
        Line::from(vec![
            Span::styled("Depends: ", pal.emphasized(pal.status_loading)),
            Span::raw(
                pkg.depends
                    .as_ref()
                    .map(|v| v.iter().take(5).cloned().collect::<Vec<_>>().join(", "))
                    .unwrap_or_else(|| "-".into()),
            ),
        ]),
        Line::from(vec![
            Span::styled("Optdepends: ", pal.emphasized(pal.status_loading)),
            Span::raw(
                pkg.optdepends
                    .as_ref()
                    .map(|v| v.iter().take(3).cloned().collect::<Vec<_>>().join(", "))
                    .unwrap_or_else(|| "-".into()),
            ),
        ]),
    ]
}

/// 0.12 (TUI phase 3): the scrollable body of info/help/message popups.
/// Confirm dialogs are short by construction and never carry scroll state.
struct ScrollPopup {
    text: Text<'static>,
    rect: Rect,
    content_h: u16,
    centered: bool,
}

fn scroll_popup(app: &App, area: Rect) -> Option<ScrollPopup> {
    let pal = app.palette();
    let (text, percent_x, centered) = match &app.popup {
        Popup::Info(pkg) => {
            let mut lines = info_lines(pkg, pal);
            lines.push(Line::raw(""));
            lines.push(Line::from(Span::styled(
                "Press Esc/q/Enter to close",
                pal.text_dim,
            )));
            (
                Text::from(lines),
                app.config.tui.popup_info.unwrap_or([70, 60])[0],
                false,
            )
        }
        Popup::Help => (
            Text::from(
                crate::tui::keys::help_lines_with(app.debug_note.as_deref())
                    .into_iter()
                    .map(Line::from)
                    .collect::<Vec<_>>(),
            ),
            app.config.tui.popup_help.unwrap_or([70, 60])[0],
            false,
        ),
        Popup::Message(msg) => (
            Text::from(msg.clone()),
            app.config.tui.popup_message.unwrap_or([60, 20])[0],
            true,
        ),
        Popup::OpLog => {
            let mut lines = vec![Line::from(Span::styled(
                " Operation log — memory only, last 20 ",
                pal.text_dim,
            ))];
            if app.ops_log.is_empty() {
                lines.push(Line::raw("No operations yet"));
            }
            for e in app.ops_log.iter().rev() {
                lines.push(Line::from(vec![
                    Span::styled(format!("{} ", e.elapsed), pal.text_dim),
                    Span::styled(format!("{:<12}", e.action), pal.installed),
                    Span::raw(format!("{} — {} ({})", e.target, e.result, e.duration)),
                ]));
            }
            (Text::from(lines), 80, false)
        }
        Popup::History { selected } => {
            let mut lines = vec![Line::from(Span::styled(
                " Query history — this session only ",
                pal.text_dim,
            ))];
            if app.query_history.is_empty() {
                lines.push(Line::raw("No searches yet"));
            }
            for (i, q) in app.query_history.iter().enumerate() {
                let mark = if i == *selected { "▸ " } else { "  " };
                lines.push(Line::from(Span::styled(
                    format!("{mark}{q}"),
                    if i == *selected {
                        pal.highlight
                    } else {
                        pal.text
                    },
                )));
            }
            lines.push(Line::raw("Enter: rerun  d: delete  Esc: close"));
            (Text::from(lines), 70, false)
        }
        _ => return None,
    };
    let inner_w = layout::popup_width(percent_x, area).saturating_sub(2);
    let content_h = Paragraph::new(text.clone())
        .wrap(Wrap { trim: true })
        .line_count(inner_w) as u16;
    let rect = layout::popup_rect_fit(percent_x, content_h, area);
    Some(ScrollPopup {
        text,
        rect,
        content_h,
        centered,
    })
}

/// `(max_scroll, page_rows)` for the current popup, from the very same
/// wrapped content measurement the renderer uses. `(0, 0)` for confirms.
pub fn popup_scroll_metrics(app: &App, area: Rect) -> (u16, u16) {
    let Some(sp) = scroll_popup(app, area) else {
        return (0, 0);
    };
    let inner_h = sp.rect.height.saturating_sub(2);
    if sp.content_h <= inner_h {
        (0, inner_h)
    } else {
        // One row is reserved for the ▲ n/m ▼ indicator while scrolling.
        let body = inner_h.saturating_sub(1);
        (sp.content_h.saturating_sub(body), body)
    }
}

/// The one scrollable-popup renderer: content-sized block + a dim page
/// indicator on the last inner row only when the copy does not fit.
fn draw_scrollable(
    f: &mut Frame,
    app: &App,
    area: Rect,
    title: Line<'static>,
    border_style: Style,
) {
    let pal = app.palette();
    let Some(sp) = scroll_popup(app, area) else {
        return;
    };
    f.render_widget(Clear, sp.rect);
    let block = layout::overlay_block(pal, app.border_type, title, border_style);
    let inner = block.inner(sp.rect);
    f.render_widget(block, sp.rect);
    let (max_scroll, body_h) = popup_scroll_metrics(app, area);
    let scroll = app.popup_scroll.min(max_scroll);
    let paragraph = Paragraph::new(sp.text)
        .wrap(Wrap { trim: true })
        .style(pal.text)
        .alignment(if sp.centered {
            Alignment::Center
        } else {
            Alignment::Left
        })
        .scroll((scroll, 0));
    if max_scroll > 0 && inner.height > 1 {
        let body = Rect {
            height: body_h.min(inner.height - 1),
            ..inner
        };
        f.render_widget(paragraph, body);
        let page_rows = body.height.max(1);
        let pages = sp.content_h.div_ceil(page_rows).max(1);
        let page = scroll / page_rows + 1;
        let note = Paragraph::new(Line::from(Span::styled(
            format!(" ▲ {page}/{pages} ▼ "),
            pal.text_dim,
        )))
        .alignment(Alignment::Right);
        let row = Rect {
            y: inner.y + body.height,
            height: 1,
            ..inner
        };
        f.render_widget(note, row);
    } else {
        f.render_widget(paragraph, inner);
    }
}

pub fn draw_info_popup(f: &mut Frame, pkg: &crate::model::Package, area: Rect, app: &App) {
    let title = Line::from(format!(" {} / {} ", pkg.repo, pkg.name));
    draw_scrollable(f, app, area, title, app.palette().popup_title);
}

/// The one confirm-dialog component (plan §3): the popup block (ALL
/// borders + config border type + the dialog's border emphasis) + centered
/// body + the shared Y/N legend row + the "Will run" footer through the
/// single NO_CONFIRM gear path. All five dialogs render through here.
/// Everything a confirm dialog renders. The five dialogs differ only in this
/// payload — bundling it keeps the shared renderer under the arg-count lint
/// and gives the clone-prune compact mode a named, documented knob.
struct ConfirmDialog {
    title: &'static str,
    title_style: ratatui::style::Style,
    body: Vec<Line<'static>>,
    run: Vec<Line<'static>>,
    /// Extra body rows (the clone-prune line) drop one gap row so the run
    /// line still fits the default 80x24 popup height.
    compact: bool,
}

fn draw_confirm_dialog(f: &mut Frame, app: &App, area: Rect, dialog: ConfirmDialog) {
    let ConfirmDialog {
        title,
        title_style,
        body,
        run,
        compact,
    } = dialog;
    let pal = app.palette();
    let sz = app.config.tui.popup_confirm.unwrap_or([60, 30]);
    // 0.12 (TUI phase 3): content-sized too — a dialog grows to its wrapped
    // copy instead of clipping in a 30%-of-frame box.
    let inner_w = layout::popup_width(sz[0], area).saturating_sub(2);
    let mut text = body;
    text.push(Line::raw(""));
    text.push(yn_row(pal));
    // compact dialogs (extra body row, e.g. the clone-prune line) drop the
    // second gap so the run line still fits the default 80x24 popup height
    if !compact {
        text.push(Line::raw(""));
    }
    text.extend(run);
    let content_h = Paragraph::new(text.clone())
        .wrap(Wrap { trim: true })
        .line_count(inner_w) as u16;
    let popup_area = layout::popup_rect_fit(sz[0], content_h, area);
    f.render_widget(Clear, popup_area);
    let block = layout::overlay_block(pal, app.border_type, title, title_style);
    let inner = block.inner(popup_area);
    f.render_widget(block, popup_area);
    let p = Paragraph::new(text)
        .alignment(ratatui::layout::Alignment::Center)
        .wrap(Wrap { trim: true });
    f.render_widget(p, inner);
}

/// The shared confirm legend row — byte-identical across all five dialogs
/// (one component, one styling set).
fn yn_row(pal: &ThemeStyles) -> Line<'static> {
    Line::from(vec![
        Span::styled("Y", pal.installed),
        Span::raw("/Enter = yes  "),
        Span::styled("N/Esc", pal.out_of_date),
        Span::raw(" = cancel"),
    ])
}

/// The one NO_CONFIRM gear path (plan §3): the catalog `behavior.*_noconfirm`
/// flags swap the credential note for the noconfirm flag the dialog would
/// actually send. Default-config output is byte-identical to v0.10.
fn confirm_run_line(text: String, pal: &ThemeStyles) -> Line<'static> {
    Line::from(Span::styled(text, pal.text_dim))
}

pub fn draw_confirm_popup(f: &mut Frame, pkg: &crate::model::Package, area: Rect, app: &App) {
    let pal = app.palette();
    let body = vec![Line::from(vec![
        Span::raw("Install "),
        Span::styled(
            format!("{}/{} {}", pkg.repo, pkg.name, pkg.version),
            pal.installed,
        ),
        Span::raw(" ?"),
    ])];
    let run = vec![confirm_run_line(
        if pkg.repo == "aur" {
            "AUR PKGBUILD will be shown before build.".to_string()
        } else {
            "Will run: sudo pacman -S {pkg} [needs password]".replace("{pkg}", &pkg.name)
        },
        pal,
    )];
    draw_confirm_dialog(
        f,
        app,
        area,
        ConfirmDialog {
            title: " Confirm install ",
            title_style: pal.popup_title,
            body,
            run,
            compact: false,
        },
    );
}

/// 0.12 (TUI phase 2): the single-row upgrade dialog — honest "upgrade" copy
/// with the old → new transition, and the command the flow actually runs
/// (pacman -S upgrades a repo row; flatpak update upgrades an installed ref).
pub fn draw_confirm_upgrade_one_popup(
    f: &mut Frame,
    pkg: &crate::model::Package,
    old_version: &str,
    area: Rect,
    app: &App,
) {
    let pal = app.palette();
    let body = vec![Line::from(vec![
        Span::raw("Upgrade "),
        Span::styled(
            format!(
                "{}/{} {} → {}",
                pkg.repo, pkg.name, old_version, pkg.version
            ),
            pal.installed,
        ),
        Span::raw(" ?"),
    ])];
    let run = vec![confirm_run_line(
        if pkg.repo == "aur" {
            "AUR PKGBUILD will be shown before build.".to_string()
        } else if pkg.repo == "flatpak" {
            format!("Will run: flatpak update {}", pkg.name)
        } else {
            "Will run: sudo pacman -S {pkg} [needs password]".replace("{pkg}", &pkg.name)
        },
        pal,
    )];
    draw_confirm_dialog(
        f,
        app,
        area,
        ConfirmDialog {
            title: " Confirm upgrade ",
            title_style: pal.popup_title,
            body,
            run,
            compact: false,
        },
    );
}

pub fn draw_confirm_remove_popup(
    f: &mut Frame,
    pkg: &crate::model::Package,
    area: Rect,
    app: &App,
) {
    let pal = app.palette();
    let body = vec![Line::from(vec![
        Span::raw("Remove "),
        Span::styled(
            format!("{}/{} {}", pkg.repo, pkg.name, pkg.version),
            pal.out_of_date,
        ),
        Span::raw(" ?"),
    ])];
    let flag = app.config.remove_flag_arg();
    let line = if pkg.repo == "flatpak" {
        // do_remove runs `flatpak uninstall` for these rows — the old copy
        // claimed pacman; the unified path states the honest command.
        if app.config.flatpak_noconfirm() {
            format!("Will run: flatpak uninstall {} -y", pkg.name)
        } else {
            format!(
                "Will run: flatpak uninstall {} [confirm in terminal]",
                pkg.name
            )
        }
    } else if app.config.remove_noconfirm() {
        format!(
            "Will run: sudo pacman {flag} {pkg} --noconfirm",
            pkg = pkg.name
        )
    } else {
        // v0.10 copy, byte-identical
        format!(
            "Will run: sudo pacman {flag} {pkg} [needs password]",
            pkg = pkg.name
        )
    };
    let run = vec![
        confirm_run_line(line, pal),
        confirm_run_line(
            "Check Required By in info (i) before removing libs.".to_string(),
            pal,
        ),
    ];
    draw_confirm_dialog(
        f,
        app,
        area,
        ConfirmDialog {
            title: " Confirm remove ",
            title_style: pal.danger_border(),
            body,
            run,
            compact: false,
        },
    );
}

pub fn draw_confirm_refresh_popup(f: &mut Frame, area: Rect, app: &App) {
    let pal = app.palette();
    let body = vec![Line::from("Refresh package databases?")];
    let line = if app.config.refresh_noconfirm() {
        "Will run: sudo pacman -Sy --noconfirm"
    } else {
        "Will run: sudo pacman -Sy [needs password]"
    };
    let run = vec![confirm_run_line(line.to_string(), pal)];
    draw_confirm_dialog(
        f,
        app,
        area,
        ConfirmDialog {
            title: " Confirm refresh ",
            title_style: pal.popup_title,
            body,
            run,
            compact: false,
        },
    );
}

pub fn draw_confirm_upgrade_popup(f: &mut Frame, area: Rect, app: &App) {
    let pal = app.palette();
    // 5.1: news-intervention warning inline — the single-step replacement
    // for the old press-U-twice dance. Rows: question (+ blank, warn) — the
    // dialog adds the blank row before the legend, so the no-warn default
    // keeps the exact v0.10 5-row rhythm (declare the detail later).
    let mut body = vec![Line::from("Upgrade full system?")];
    if let Some(warn) = app.upgrade_warn.as_deref().filter(|w| !w.trim().is_empty()) {
        body.push(Line::raw(""));
        body.push(Line::from(vec![Span::styled(
            format!("Manual intervention? {warn}"),
            pal.out_of_date,
        )]));
    }
    // One NO_CONFIRM gear path: each flag tags its own command segment;
    // the credential note survives only while neither flag is set (the
    // default-config bytes are exactly the v0.10 line).
    let line = format!(
        "Will run: sudo pacman -Syu{pacman} + flatpak update{flatpak}{suffix}",
        pacman = if app.config.upgrade_noconfirm() {
            " --noconfirm"
        } else {
            ""
        },
        flatpak = if app.config.flatpak_noconfirm() {
            " -y"
        } else {
            ""
        },
        suffix = if app.config.upgrade_noconfirm() || app.config.flatpak_noconfirm() {
            ""
        } else {
            " [needs password]"
        },
    );
    let run = vec![confirm_run_line(line, pal)];
    draw_confirm_dialog(
        f,
        app,
        area,
        ConfirmDialog {
            title: " Confirm upgrade ",
            title_style: pal.popup_title,
            body,
            run,
            compact: false,
        },
    );
}

pub fn draw_confirm_clean_popup(
    f: &mut Frame,
    files: usize,
    bytes: u64,
    flatpak: bool,
    clones: crate::install::clean::ClonePlan,
    area: Rect,
    app: &App,
) {
    // ui-design discipline held: one focal number (reclaimable size), one CTA.
    let pal = app.palette();
    let size = crate::stats::human_bytes(bytes);
    let csize = crate::stats::human_bytes(clones.bytes);
    let mut body: Vec<Line> = Vec::new();
    if files > 0 || clones.dirs == 0 {
        // the pre-phase-2 popup bytes stay byte-identical when no clones
        // are pending (the settings default 0 = off renders today's dialog)
        body.push(Line::from(vec![
            Span::raw("Remove "),
            Span::styled(format!("{files} cached files ({size})"), pal.installed),
            Span::raw(" ?"),
        ]));
    }
    if clones.dirs > 0 {
        // Compact phrasing: the default 80x24 popup has a 46-col inner width
        // and a 5-row inner height, so a longer line wraps and clips the run
        // row, and an extra body row is only affordable because `compact`
        // drops one gap row below. The run line stays untouched (it already
        // sits at the width limit).
        let q = if files == 0 { " ?" } else { "" };
        body.push(Line::from(vec![
            Span::raw("+ "),
            Span::styled(
                format!(
                    "{dirs} AUR clone dirs ({csize}), {days}+ days old{q}",
                    dirs = clones.dirs,
                    days = clones.days
                ),
                pal.installed,
            ),
        ]));
    }
    let line = if flatpak {
        "Will run: paccache -r + flatpak --unused".to_string()
    } else if app.config.clean_noconfirm() {
        "Will run: sudo paccache -r -kN --noconfirm".to_string()
    } else {
        "Will run: sudo paccache -r -kN [needs password]".to_string()
    };
    let run = vec![confirm_run_line(line, pal)];
    draw_confirm_dialog(
        f,
        app,
        area,
        ConfirmDialog {
            title: " Confirm clean ",
            title_style: pal.popup_title,
            body,
            run,
            compact: clones.dirs > 0,
        },
    );
}

pub fn draw_help_popup(f: &mut Frame, area: Rect, app: &App) {
    // The `?` content is generated from the keymap table (drift gate in
    // `tui::keys`); phase 3 adds j/k/PageUp/PageDown/g/G scrolling so all of
    // it is reachable even on 60x14.
    draw_scrollable(
        f,
        app,
        area,
        Line::from(" Help (?) "),
        app.palette().popup_title,
    );
}

pub fn draw_message_popup(f: &mut Frame, msg: &str, area: Rect, app: &App) {
    let border = app.palette().message_border(msg.starts_with('✓'));
    draw_scrollable(f, app, area, Line::from(" Message "), border);
}

pub fn draw_op_log_popup(f: &mut Frame, area: Rect, app: &App) {
    draw_scrollable(
        f,
        app,
        area,
        Line::from(" Operations log "),
        app.palette().popup_title,
    );
}

pub fn draw_history_popup(f: &mut Frame, area: Rect, app: &App) {
    draw_scrollable(
        f,
        app,
        area,
        Line::from(" Query history "),
        app.palette().popup_title,
    );
}

/// 0.12 (TUI phase 4): the command palette is a generated view of
/// `keys::BINDINGS` — every row is an existing `Action`, so it cannot drift
/// into a second, stale command list.
pub fn draw_command_palette(f: &mut Frame, area: Rect, app: &App) {
    let Popup::CommandPalette { query, selected } = &app.popup else {
        return;
    };
    let pal = app.palette();
    let commands = crate::tui::keys::palette_commands(query);
    let mut lines = vec![Line::from(vec![
        Span::styled("> ", pal.text_dim),
        Span::styled(query.clone(), pal.text),
    ])];
    for (i, cmd) in commands.iter().enumerate() {
        let mark = if i == *selected { "▸ " } else { "  " };
        lines.push(Line::from(vec![
            Span::styled(mark, pal.text_dim),
            Span::styled(layout::fit(&cmd.label, 32), pal.text),
            Span::styled(cmd.keys.clone(), pal.text_dim),
        ]));
    }
    if commands.is_empty() {
        lines.push(Line::raw("No command matches — Esc closes"));
    }
    let rect = layout::popup_rect_fit(70, (lines.len() + 1) as u16, area);
    f.render_widget(Clear, rect);
    let block = layout::overlay_block(
        pal,
        app.border_type,
        Line::from(" Command palette "),
        pal.popup_title,
    );
    let inner = block.inner(rect);
    f.render_widget(block, rect);
    let view = inner.height.saturating_sub(1);
    let offset = selected.saturating_sub(view.saturating_sub(1) as usize) as u16;
    let p = Paragraph::new(lines)
        .wrap(Wrap { trim: true })
        .style(pal.text)
        .scroll((offset, 0));
    f.render_widget(p, inner);
}
