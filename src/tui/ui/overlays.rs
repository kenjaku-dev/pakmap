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

    let text = vec![
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
            Span::raw(pkg.url.as_deref().unwrap_or("-")),
        ]),
        Line::from(vec![
            Span::styled("Desc: ", pal.emphasized(pal.status_loading)),
            Span::raw(pkg.description.as_deref().unwrap_or("-")),
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
            Span::raw(pkg.reason.as_deref().unwrap_or("-")),
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
        Line::raw(""),
        Line::from(Span::styled("Press Esc/q/Enter to close", pal.text_dim)),
    ];

    let p = Paragraph::new(text)
        .wrap(Wrap { trim: true })
        .style(pal.text);
    f.render_widget(p, inner);
}

pub fn draw_confirm_popup(f: &mut Frame, pkg: &crate::model::Package, area: Rect, app: &App) {
    let pal = app.palette();
    let sz = app.config.tui.popup_confirm.unwrap_or([60, 30]);
    let popup_area = layout::popup_rect(sz[0], sz[1], area);
    f.render_widget(Clear, popup_area);
    let block = layout::overlay_block(pal, app.border_type, " Confirm install ", pal.popup_title);
    let inner = block.inner(popup_area);
    f.render_widget(block, popup_area);
    let text = vec![
        Line::from(vec![
            Span::raw("Install "),
            Span::styled(
                format!("{}/{} {}", pkg.repo, pkg.name, pkg.version),
                pal.installed,
            ),
            Span::raw(" ?"),
        ]),
        Line::raw(""),
        Line::from(vec![
            Span::styled("Y", pal.installed),
            Span::raw("/Enter = yes  "),
            Span::styled("N/Esc", pal.out_of_date),
            Span::raw(" = cancel"),
        ]),
        Line::raw(""),
        Line::from(Span::styled(
            if pkg.repo == "aur" {
                "AUR PKGBUILD will be shown before build."
            } else {
                "Will run: sudo pacman -S <pkg> [needs password]"
            },
            pal.text_dim,
        )),
    ];
    let p = Paragraph::new(text)
        .alignment(ratatui::layout::Alignment::Center)
        .wrap(Wrap { trim: true });
    f.render_widget(p, inner);
}

pub fn draw_confirm_remove_popup(
    f: &mut Frame,
    pkg: &crate::model::Package,
    area: Rect,
    app: &App,
) {
    let pal = app.palette();
    let sz = app.config.tui.popup_confirm.unwrap_or([60, 30]);
    let popup_area = layout::popup_rect(sz[0], sz[1], area);
    f.render_widget(Clear, popup_area);
    let block = layout::overlay_block(
        pal,
        app.border_type,
        " Confirm remove ",
        pal.danger_border(),
    );
    let inner = block.inner(popup_area);
    f.render_widget(block, popup_area);
    let flag = app.config.remove_flag_arg();
    let text = vec![
        Line::from(vec![
            Span::raw("Remove "),
            Span::styled(
                format!("{}/{} {}", pkg.repo, pkg.name, pkg.version),
                pal.out_of_date,
            ),
            Span::raw(" ?"),
        ]),
        Line::raw(""),
        Line::from(vec![
            Span::styled("Y", pal.installed),
            Span::raw("/Enter = yes  "),
            Span::styled("N/Esc", pal.out_of_date),
            Span::raw(" = cancel"),
        ]),
        Line::raw(""),
        Line::from(Span::styled(
            format!(
                "Will run: sudo pacman {} {} [needs password]",
                flag, pkg.name
            ),
            pal.text_dim,
        )),
        Line::from(Span::styled(
            "Check Required By in info (i) before removing libs.",
            pal.text_dim,
        )),
    ];
    let p = Paragraph::new(text)
        .alignment(ratatui::layout::Alignment::Center)
        .wrap(Wrap { trim: true });
    f.render_widget(p, inner);
}

pub fn draw_confirm_refresh_popup(f: &mut Frame, area: Rect, app: &App) {
    let pal = app.palette();
    let sz = app.config.tui.popup_confirm.unwrap_or([60, 30]);
    let popup_area = layout::popup_rect(sz[0], sz[1], area);
    f.render_widget(Clear, popup_area);
    let block = layout::overlay_block(pal, app.border_type, " Confirm refresh ", pal.popup_title);
    let inner = block.inner(popup_area);
    f.render_widget(block, popup_area);
    let text = vec![
        Line::from("Refresh package databases?"),
        Line::raw(""),
        Line::from(vec![
            Span::styled("Y", pal.installed),
            Span::raw("/Enter = yes  "),
            Span::styled("N/Esc", pal.out_of_date),
            Span::raw(" = cancel"),
        ]),
        Line::raw(""),
        Line::from(Span::styled(
            "Will run: sudo pacman -Sy [needs password]",
            pal.text_dim,
        )),
    ];
    let p = Paragraph::new(text)
        .alignment(ratatui::layout::Alignment::Center)
        .wrap(Wrap { trim: true });
    f.render_widget(p, inner);
}

pub fn draw_confirm_upgrade_popup(f: &mut Frame, area: Rect, app: &App) {
    let pal = app.palette();
    let sz = app.config.tui.popup_confirm.unwrap_or([60, 30]);
    let popup_area = layout::popup_rect(sz[0], sz[1], area);
    f.render_widget(Clear, popup_area);
    let block = layout::overlay_block(pal, app.border_type, " Confirm upgrade ", pal.popup_title);
    let inner = block.inner(popup_area);
    f.render_widget(block, popup_area);
    let mut text = vec![Line::from("Upgrade full system?"), Line::raw("")];
    // 5.1: news-intervention warning inline — the single-step replacement
    // for the old press-U-twice dance.
    if let Some(warn) = app.upgrade_warn.as_deref().filter(|w| !w.trim().is_empty()) {
        text.push(Line::from(vec![Span::styled(
            format!("Manual intervention? {warn}"),
            pal.out_of_date,
        )]));
        text.push(Line::raw(""));
    }
    text.extend([
        Line::from(vec![
            Span::styled("Y", pal.installed),
            Span::raw("/Enter = yes  "),
            Span::styled("N/Esc", pal.out_of_date),
            Span::raw(" = cancel"),
        ]),
        Line::raw(""),
        Line::from(Span::styled(
            "Will run: sudo pacman -Syu + flatpak update [needs password]",
            pal.text_dim,
        )),
    ]);
    let p = Paragraph::new(text)
        .alignment(ratatui::layout::Alignment::Center)
        .wrap(Wrap { trim: true });
    f.render_widget(p, inner);
}

pub fn draw_confirm_clean_popup(
    f: &mut Frame,
    files: usize,
    bytes: u64,
    flatpak: bool,
    area: Rect,
    app: &App,
) {
    // ui-design: one focal number (reclaimable size), one primary CTA.
    let pal = app.palette();
    let sz = app.config.tui.popup_confirm.unwrap_or([60, 30]);
    let popup_area = layout::popup_rect(sz[0], sz[1], area);
    f.render_widget(Clear, popup_area);
    let block = layout::overlay_block(pal, app.border_type, " Confirm clean ", pal.popup_title);
    let inner = block.inner(popup_area);
    f.render_widget(block, popup_area);
    let size = crate::stats::human_bytes(bytes);
    let text = vec![
        Line::from(vec![
            Span::raw("Remove "),
            Span::styled(format!("{files} cached files ({size})"), pal.installed),
            Span::raw(" ?"),
        ]),
        Line::raw(""),
        Line::from(vec![
            Span::styled("Y", pal.installed),
            Span::raw("/Enter = yes  "),
            Span::styled("N/Esc", pal.out_of_date),
            Span::raw(" = cancel"),
        ]),
        Line::raw(""),
        Line::from(Span::styled(
            if flatpak {
                "Will run: paccache -r + flatpak --unused"
            } else {
                "Will run: sudo paccache -r -kN [needs password]"
            },
            pal.text_dim,
        )),
    ];
    let p = Paragraph::new(text)
        .alignment(ratatui::layout::Alignment::Center)
        .wrap(Wrap { trim: true });
    f.render_widget(p, inner);
}

pub fn draw_help_popup(f: &mut Frame, area: Rect, app: &App) {
    let pal = app.palette();
    let sz = app.config.tui.popup_help.unwrap_or([70, 60]);
    let popup_area = layout::popup_rect(sz[0], sz[1], area);
    f.render_widget(Clear, popup_area);
    let block = layout::overlay_block(pal, app.border_type, " Help (?) ", pal.popup_title);
    let inner = block.inner(popup_area);
    f.render_widget(block, popup_area);
    let lines = vec![
        Line::from("Tab / BackTab : cycle Search -> Installed -> Updates"),
        Line::from("F5 / Ctrl+R   : refresh sync DBs (confirm, anywhere)"),
        Line::from(""),
        Line::from("Search    : type+Enter search, ↑↓ move, Enter install,"),
        Line::from("              i info+deps, r refresh (list)"),
        Line::from("Installed : filter, Enter/d/x remove, o orphans-only,"),
        Line::from("              c clean cache, i info, r refresh (list)"),
        Line::from("Updates   : filter, Enter upgrade-one, U upgrade-all,"),
        Line::from("              c clean cache, i info"),
        Line::from("Global    : / search, Esc toggle, ? help, q quit (list)"),
        Line::from("Settings  : S opens the settings modal (list focus) — every"),
        Line::from("            config key: / filter, Space toggle, Enter edit,"),
        Line::from("            r reset, s save (user file), e $EDITOR, ←→ categories"),
        Line::from(""),
        Line::from("Press Esc/q/Enter/? to close"),
    ];
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
