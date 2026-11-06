//! Overlays (overhaul phase 2): the centered popups — package info, the
//! five confirm dialogs, help, message — all speaking one block vocabulary
//! (`layout::overlay_block`: ALL borders + the config border type + the
//! registry popup background + the dialog's border emphasis) and the same
//! `layout::popup_rect` geometry. Content is the v0.10 copy, byte-for-byte.

use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Span},
    widgets::{Clear, Paragraph, Wrap},
};

use super::layout;
use crate::tui::app::App;
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

pub fn draw_info_popup(f: &mut Frame, pkg: &crate::model::Package, area: Rect, app: &App) {
    let pal = app.palette();
    let sz = app.config.tui.popup_info.unwrap_or([70, 60]);
    let popup_area = layout::popup_rect(sz[0], sz[1], area);
    f.render_widget(Clear, popup_area);

    let block = layout::overlay_block(
        pal,
        app.border_type,
        format!(" {} / {} ", pkg.repo, pkg.name),
        pal.popup_title,
    );

    let inner = block.inner(popup_area);
    f.render_widget(block, popup_area);

    let mut text: Vec<Line> = info_lines(pkg, pal);
    text.push(Line::raw(""));
    text.push(Line::from(Span::styled(
        "Press Esc/q/Enter to close",
        pal.text_dim,
    )));

    let p = Paragraph::new(text)
        .wrap(Wrap { trim: true })
        .style(pal.text);
    f.render_widget(p, inner);
}

/// The one confirm-dialog component (plan §3): the popup block (ALL
/// borders + config border type + the dialog's border emphasis) + centered
/// body + the shared Y/N legend row + the "Will run" footer through the
/// single NO_CONFIRM gear path. All five dialogs render through here.
fn draw_confirm_dialog(
    f: &mut Frame,
    app: &App,
    area: Rect,
    title: &'static str,
    title_style: ratatui::style::Style,
    body: Vec<Line<'static>>,
    run: Vec<Line<'static>>,
) {
    let pal = app.palette();
    let sz = app.config.tui.popup_confirm.unwrap_or([60, 30]);
    let popup_area = layout::popup_rect(sz[0], sz[1], area);
    f.render_widget(Clear, popup_area);
    let block = layout::overlay_block(pal, app.border_type, title, title_style);
    let inner = block.inner(popup_area);
    f.render_widget(block, popup_area);
    let mut text = body;
    text.push(Line::raw(""));
    text.push(yn_row(pal));
    text.push(Line::raw(""));
    text.extend(run);
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
        " Confirm install ",
        pal.popup_title,
        body,
        run,
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
        " Confirm remove ",
        pal.danger_border(),
        body,
        run,
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
        " Confirm refresh ",
        pal.popup_title,
        body,
        run,
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
        " Confirm upgrade ",
        pal.popup_title,
        body,
        run,
    );
}

pub fn draw_confirm_clean_popup(
    f: &mut Frame,
    files: usize,
    bytes: u64,
    flatpak: bool,
    area: Rect,
    app: &App,
) {
    // ui-design discipline held: one focal number (reclaimable size), one CTA.
    let pal = app.palette();
    let size = crate::stats::human_bytes(bytes);
    let body = vec![Line::from(vec![
        Span::raw("Remove "),
        Span::styled(format!("{files} cached files ({size})"), pal.installed),
        Span::raw(" ?"),
    ])];
    let line = if flatpak {
        "Will run: paccache -r + flatpak --unused".to_string()
    } else if app.config.clean_noconfirm() {
        "Will run: sudo paccache -r -kN --noconfirm".to_string()
    } else {
        "Will run: sudo paccache -r -kN [needs password]".to_string()
    };
    let run = vec![confirm_run_line(line, pal)];
    draw_confirm_dialog(f, app, area, " Confirm clean ", pal.popup_title, body, run);
}

pub fn draw_help_popup(f: &mut Frame, area: Rect, app: &App) {
    let pal = app.palette();
    let sz = app.config.tui.popup_help.unwrap_or([70, 60]);
    let popup_area = layout::popup_rect(sz[0], sz[1], area);
    f.render_widget(Clear, popup_area);
    let block = layout::overlay_block(pal, app.border_type, " Help (?) ", pal.popup_title);
    let inner = block.inner(popup_area);
    f.render_widget(block, popup_area);
    // Overhaul phase 3 (plan §3): the `?` screen is GENERATED from the
    // keymap table — a drift test pins the bytes in `tui::keys`. Phase 4:
    // `debug_note` (set only under PAKMAP_TUI_DEBUG) appends one timing
    // line; every default build keeps the pinned bytes inset.
    let lines: Vec<Line> = crate::tui::keys::help_lines_with(app.debug_note.as_deref())
        .into_iter()
        .map(Line::from)
        .collect();
    let p = Paragraph::new(lines)
        .wrap(Wrap { trim: true })
        .style(pal.text);
    f.render_widget(p, inner);
}

pub fn draw_message_popup(f: &mut Frame, msg: &str, area: Rect, app: &App) {
    let pal = app.palette();
    let sz = app.config.tui.popup_message.unwrap_or([60, 20]);
    let popup_area = layout::popup_rect(sz[0], sz[1], area);
    f.render_widget(Clear, popup_area);
    let block = layout::overlay_block(
        pal,
        app.border_type,
        " Message ",
        pal.message_border(msg.starts_with('✓')),
    );
    let inner = block.inner(popup_area);
    f.render_widget(block, popup_area);
    let p = Paragraph::new(msg.to_string())
        .style(pal.text)
        .alignment(ratatui::layout::Alignment::Center)
        .wrap(Wrap { trim: true });
    f.render_widget(p, inner);
}
