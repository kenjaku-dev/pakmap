use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Tabs, Wrap},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::app::{App, Focus, Mode, Popup};
use super::settings::SettingsModal;

// tui-design skill: calm, predictable, fast — single border depth, semantic tokens, NO_COLOR aware
// v0.3.0: Tabs bar (Search/Installed) + ThemeStyles cache (no per-frame parse_style).
pub fn draw(f: &mut Frame, app: &App) {
    let area = f.area();

    // Responsive floor: configurable via [tui] floor_width/floor_height per tui-design responsive + layout Min
    let fw = app.config.tui.floor_width.max(40);
    let fh = app.config.tui.floor_height.max(10);
    if area.width < fw || area.height < fh {
        let msg = Paragraph::new(format!("Terminal too small — need ≥{}×{}", fw, fh))
            .style(if app.no_color {
                Style::default().add_modifier(Modifier::BOLD)
            } else {
                app.styles.border_error
            })
            .alignment(ratatui::layout::Alignment::Center)
            .wrap(Wrap { trim: true })
            .block(Block::bordered().title(" pakmap "));
        f.render_widget(msg, area);
        return;
    }

    let show_tabs = app.config.tui.show_tabs;
    // Layout: tabs(1) + search(3) + results(Min 8) + status(1) + help(1)
    let ls = app.config.tui.layout_search.max(2);
    let lm = app.config.tui.layout_results_min.max(4);
    if show_tabs {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),  // tabs
                Constraint::Length(ls), // search bar
                Constraint::Min(lm),    // results
                Constraint::Length(1),  // status
                Constraint::Length(1),  // help
            ])
            .split(area);
        draw_tabs(f, app, chunks[0]);
        draw_search(f, app, chunks[1]);
        draw_results(f, app, chunks[2]);
        draw_status(f, app, chunks[3]);
        draw_help(f, app, chunks[4]);
    } else {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(ls), // search bar
                Constraint::Min(lm),    // results
                Constraint::Length(1),  // status
                Constraint::Length(1),  // help
            ])
            .split(area);
        draw_search(f, app, chunks[0]);
        draw_results(f, app, chunks[1]);
        draw_status(f, app, chunks[2]);
        draw_help(f, app, chunks[3]);
    }

    // v0.10.0 phase 3: settings modal above the app view, below popups.
    if app.settings.is_some() {
        draw_settings_modal(f, app, area);
    }

    // Popups on top with Clear hole-punch
    match &app.popup {
        Popup::Info(pkg) => draw_info_popup(f, pkg, area, app),
        Popup::Confirm(pkg) => draw_confirm_popup(f, pkg, area, app),
        Popup::ConfirmRemove(pkg) => draw_confirm_remove_popup(f, pkg, area, app),
        Popup::ConfirmRefresh => draw_confirm_refresh_popup(f, area, app),
        Popup::ConfirmUpgrade => draw_confirm_upgrade_popup(f, area, app),
        Popup::ConfirmClean {
            files,
            bytes,
            flatpak,
        } => draw_confirm_clean_popup(f, *files, *bytes, *flatpak, area, app),
        Popup::Help => draw_help_popup(f, area, app),
        Popup::Message(msg) => draw_message_popup(f, msg, area, app),
        Popup::None => {}
    }
}

fn draw_tabs(f: &mut Frame, app: &App, area: Rect) {
    let use_color = !app.no_color;
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
        .style(if use_color {
            app.styles.tab_normal
        } else {
            Style::default()
        })
        .highlight_style(if use_color {
            app.styles.tab_selected
        } else {
            Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
        })
        .divider(if area.width < 60 { "|" } else { " │ " });
    // Hint on the right is rendered via status/help; keep tabs single-line, no border (clutter audit).
    f.render_widget(tabs, area);
    let _ = Mode::Search; // keep import used in narrow builds
}

fn draw_search(f: &mut Frame, app: &App, area: Rect) {
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
    let style = if app.no_color {
        Style::default()
    } else if app.focus == Focus::Search {
        app.styles.border_focused
    } else {
        app.styles.border_unfocused
    };

    let input_str = app.input.value();
    let width = area.width.saturating_sub(4) as usize; // borders + padding
    // width-aware tail truncation, char-boundary safe per rust-common-pitfalls
    let input_width = input_str.width();
    let display = if input_width > width && width > 3 {
        let target = width.saturating_sub(3);
        let mut w = 0usize;
        let mut start_idx = input_str.len();
        // Walk from end, collect width until target — char-boundary safe
        for (idx, ch) in input_str.char_indices().rev() {
            let cw = ch.width().unwrap_or(0);
            if w + cw > target {
                break;
            }
            w += cw;
            start_idx = idx;
        }
        // start_idx is already at char boundary from char_indices
        format!("...{}", &input_str[start_idx..])
    } else {
        input_str.to_string()
    };

    // Cursor position for tui-input (visual width, not byte index)
    let cursor_pos = app.input.visual_cursor();
    // Width now: `display` moves into the Paragraph below, so measure first.
    let disp_width = display.width();

    let paragraph = Paragraph::new(display)
        .style(if app.no_color {
            Style::default()
        } else {
            app.styles.text
        })
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(style)
                .title(title)
                .border_type(app.border_type),
        );

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

fn draw_results(f: &mut Frame, app: &App, area: Rect) {
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
            .style(if app.no_color {
                Style::default()
            } else {
                app.styles.text_dim
            })
            .alignment(ratatui::layout::Alignment::Center)
            .wrap(Wrap { trim: true })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(app.border_type)
                    .title(title),
            );
        f.render_widget(p, area);
        return;
    }

    let use_color = !app.no_color;
    let items: Vec<ListItem> = app
        .packages
        .iter()
        .map(|pkg| {
            let repo_style = if !use_color {
                Style::default().add_modifier(Modifier::BOLD)
            } else if pkg.repo == "aur" {
                app.styles.repo_aur
            } else {
                match pkg.repo.as_str() {
                    "core" => app.styles.repo_core,
                    "extra" => app.styles.repo_extra,
                    "multilib" => app.styles.repo_multilib,
                    "local" => app.styles.repo_local,
                    _ => app.styles.repo_other,
                }
            };
            // In Installed mode the [installed] tag is noise (all are installed) — hide it.
            let show_installed_tag = pkg.installed && app.mode == Mode::Search;
            let installed = if show_installed_tag {
                Span::styled(
                    " [installed]",
                    if use_color {
                        app.styles.installed
                    } else {
                        Style::default().add_modifier(Modifier::BOLD)
                    },
                )
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
                Span::styled(
                    tags,
                    if use_color {
                        app.styles.votes
                    } else {
                        Style::default()
                    },
                )
            } else if pkg.orphan {
                Span::styled(
                    " [ORPHAN]",
                    if use_color {
                        app.styles.out_of_date
                    } else {
                        Style::default().add_modifier(Modifier::BOLD)
                    },
                )
            } else {
                Span::raw("")
            };

            let first = Line::from(vec![
                Span::styled(format!("{}/{}", pkg.repo, pkg.name), repo_style),
                Span::styled(
                    format!(" {}", pkg.version),
                    if use_color {
                        app.styles.version
                    } else {
                        Style::default()
                    },
                ),
                aur_extra,
                installed,
            ]);

            let second = Line::from(vec![Span::styled(
                format!("  {}", pkg.description.as_deref().unwrap_or("-")),
                if use_color {
                    app.styles.text_dim
                } else {
                    Style::default()
                },
            )]);

            ListItem::new(vec![first, second])
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(app.border_type)
                .title(title),
        )
        .highlight_style(if use_color {
            app.styles.highlight
        } else {
            Style::default().add_modifier(Modifier::REVERSED)
        })
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

fn draw_status(f: &mut Frame, app: &App, area: Rect) {
    let style = if app.no_color {
        Style::default()
    } else if app.is_loading {
        app.styles.status_loading
    } else {
        app.styles.status_idle
    };
    let line = Line::from(vec![Span::styled(app.status.clone(), style)]);
    let p = Paragraph::new(line);
    f.render_widget(p, area);
}

fn draw_help(f: &mut Frame, app: &App, area: Rect) {
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
    let p = Paragraph::new(help)
        .style(if app.no_color {
            Style::default()
        } else {
            app.styles.help
        })
        .block(Block::default());
    f.render_widget(p, area);
}

fn draw_info_popup(f: &mut Frame, pkg: &crate::model::Package, area: Rect, app: &App) {
    let sz = app.config.tui.popup_info.unwrap_or([70, 60]);
    let popup_area = centered_rect(sz[0], sz[1], area);
    f.render_widget(Clear, popup_area);

    let block = Block::default()
        .title(format!(" {} / {} ", pkg.repo, pkg.name))
        .borders(Borders::ALL)
        .border_type(app.border_type)
        .border_style(if app.no_color {
            Style::default()
        } else {
            app.styles.popup_title
        })
        .style(if app.no_color {
            Style::default()
        } else {
            app.styles.popup_bg
        });

    let inner = block.inner(popup_area);
    f.render_widget(block, popup_area);
    let bold = |s: Style| {
        if app.no_color {
            Style::default().add_modifier(Modifier::BOLD)
        } else {
            s.add_modifier(Modifier::BOLD)
        }
    };

    let text = vec![
        Line::from(vec![
            Span::styled("Version: ", bold(app.styles.status_loading)),
            Span::raw(pkg.version.clone()),
            Span::raw(format!("  Arch: {}", pkg.arch.as_deref().unwrap_or("-"))),
        ]),
        Line::from(vec![
            Span::styled("Repo: ", bold(app.styles.status_loading)),
            Span::raw(pkg.repo.clone()),
            Span::raw(if pkg.installed { "  [installed]" } else { "" }),
        ]),
        Line::from(vec![
            Span::styled("URL: ", bold(app.styles.status_loading)),
            Span::raw(pkg.url.as_deref().unwrap_or("-")),
        ]),
        Line::from(vec![
            Span::styled("Desc: ", bold(app.styles.status_loading)),
            Span::raw(pkg.description.as_deref().unwrap_or("-")),
        ]),
        Line::raw(""),
        Line::from(vec![
            Span::styled("Votes: ", bold(app.styles.votes)),
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
        Line::from(vec![Span::styled(
            pkg.badges().join(" "),
            if app.no_color {
                Style::default().add_modifier(Modifier::BOLD)
            } else {
                app.styles.out_of_date
            },
        )]),
        Line::from(vec![
            Span::styled("Reason: ", bold(app.styles.status_loading)),
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
            Span::styled("Depends: ", bold(app.styles.status_loading)),
            Span::raw(
                pkg.depends
                    .as_ref()
                    .map(|v| v.iter().take(5).cloned().collect::<Vec<_>>().join(", "))
                    .unwrap_or_else(|| "-".into()),
            ),
        ]),
        Line::from(vec![
            Span::styled("Optdepends: ", bold(app.styles.status_loading)),
            Span::raw(
                pkg.optdepends
                    .as_ref()
                    .map(|v| v.iter().take(3).cloned().collect::<Vec<_>>().join(", "))
                    .unwrap_or_else(|| "-".into()),
            ),
        ]),
        Line::raw(""),
        Line::from(Span::styled(
            "Press Esc/q/Enter to close",
            if app.no_color {
                Style::default()
            } else {
                app.styles.text_dim
            },
        )),
    ];

    let p = Paragraph::new(text)
        .wrap(Wrap { trim: true })
        .style(if app.no_color {
            Style::default()
        } else {
            app.styles.text
        });
    f.render_widget(p, inner);
}

fn draw_confirm_popup(f: &mut Frame, pkg: &crate::model::Package, area: Rect, app: &App) {
    let sz = app.config.tui.popup_confirm.unwrap_or([60, 30]);
    let popup_area = centered_rect(sz[0], sz[1], area);
    f.render_widget(Clear, popup_area);
    let block = Block::default()
        .title(" Confirm install ")
        .borders(Borders::ALL)
        .border_type(app.border_type)
        .border_style(if app.no_color {
            Style::default()
        } else {
            app.styles.popup_title
        })
        .style(if app.no_color {
            Style::default()
        } else {
            app.styles.popup_bg
        });
    let inner = block.inner(popup_area);
    f.render_widget(block, popup_area);
    let text = vec![
        Line::from(vec![
            Span::raw("Install "),
            Span::styled(
                format!("{}/{} {}", pkg.repo, pkg.name, pkg.version),
                if app.no_color {
                    Style::default().add_modifier(Modifier::BOLD)
                } else {
                    app.styles.installed
                },
            ),
            Span::raw(" ?"),
        ]),
        Line::raw(""),
        Line::from(vec![
            Span::styled(
                "Y",
                if app.no_color {
                    Style::default().add_modifier(Modifier::BOLD)
                } else {
                    app.styles.installed
                },
            ),
            Span::raw("/Enter = yes  "),
            Span::styled(
                "N/Esc",
                if app.no_color {
                    Style::default().add_modifier(Modifier::BOLD)
                } else {
                    app.styles.out_of_date
                },
            ),
            Span::raw(" = cancel"),
        ]),
        Line::raw(""),
        Line::from(Span::styled(
            if pkg.repo == "aur" {
                "AUR PKGBUILD will be shown before build."
            } else {
                "Will run: sudo pacman -S <pkg> [needs password]"
            },
            if app.no_color {
                Style::default()
            } else {
                app.styles.text_dim
            },
        )),
    ];
    let p = Paragraph::new(text)
        .alignment(ratatui::layout::Alignment::Center)
        .wrap(Wrap { trim: true });
    f.render_widget(p, inner);
}

fn draw_confirm_remove_popup(f: &mut Frame, pkg: &crate::model::Package, area: Rect, app: &App) {
    let sz = app.config.tui.popup_confirm.unwrap_or([60, 30]);
    let popup_area = centered_rect(sz[0], sz[1], area);
    f.render_widget(Clear, popup_area);
    let block = Block::default()
        .title(" Confirm remove ")
        .borders(Borders::ALL)
        .border_type(app.border_type)
        .border_style(if app.no_color {
            Style::default()
        } else {
            app.styles.out_of_date
        })
        .style(if app.no_color {
            Style::default()
        } else {
            app.styles.popup_bg
        });
    let inner = block.inner(popup_area);
    f.render_widget(block, popup_area);
    let flag = app.config.remove_flag_arg();
    let text = vec![
        Line::from(vec![
            Span::raw("Remove "),
            Span::styled(
                format!("{}/{} {}", pkg.repo, pkg.name, pkg.version),
                if app.no_color {
                    Style::default().add_modifier(Modifier::BOLD)
                } else {
                    app.styles.out_of_date
                },
            ),
            Span::raw(" ?"),
        ]),
        Line::raw(""),
        Line::from(vec![
            Span::styled(
                "Y",
                if app.no_color {
                    Style::default().add_modifier(Modifier::BOLD)
                } else {
                    app.styles.installed
                },
            ),
            Span::raw("/Enter = yes  "),
            Span::styled(
                "N/Esc",
                if app.no_color {
                    Style::default().add_modifier(Modifier::BOLD)
                } else {
                    app.styles.out_of_date
                },
            ),
            Span::raw(" = cancel"),
        ]),
        Line::raw(""),
        Line::from(Span::styled(
            format!(
                "Will run: sudo pacman {} {} [needs password]",
                flag, pkg.name
            ),
            if app.no_color {
                Style::default()
            } else {
                app.styles.text_dim
            },
        )),
        Line::from(Span::styled(
            "Check Required By in info (i) before removing libs.",
            if app.no_color {
                Style::default()
            } else {
                app.styles.text_dim
            },
        )),
    ];
    let p = Paragraph::new(text)
        .alignment(ratatui::layout::Alignment::Center)
        .wrap(Wrap { trim: true });
    f.render_widget(p, inner);
}

fn draw_confirm_refresh_popup(f: &mut Frame, area: Rect, app: &App) {
    let sz = app.config.tui.popup_confirm.unwrap_or([60, 30]);
    let popup_area = centered_rect(sz[0], sz[1], area);
    f.render_widget(Clear, popup_area);
    let block = Block::default()
        .title(" Confirm refresh ")
        .borders(Borders::ALL)
        .border_type(app.border_type)
        .border_style(if app.no_color {
            Style::default()
        } else {
            app.styles.popup_title
        })
        .style(if app.no_color {
            Style::default()
        } else {
            app.styles.popup_bg
        });
    let inner = block.inner(popup_area);
    f.render_widget(block, popup_area);
    let text = vec![
        Line::from("Refresh package databases?"),
        Line::raw(""),
        Line::from(vec![
            Span::styled(
                "Y",
                if app.no_color {
                    Style::default().add_modifier(Modifier::BOLD)
                } else {
                    app.styles.installed
                },
            ),
            Span::raw("/Enter = yes  "),
            Span::styled(
                "N/Esc",
                if app.no_color {
                    Style::default().add_modifier(Modifier::BOLD)
                } else {
                    app.styles.out_of_date
                },
            ),
            Span::raw(" = cancel"),
        ]),
        Line::raw(""),
        Line::from(Span::styled(
            "Will run: sudo pacman -Sy [needs password]",
            if app.no_color {
                Style::default()
            } else {
                app.styles.text_dim
            },
        )),
    ];
    let p = Paragraph::new(text)
        .alignment(ratatui::layout::Alignment::Center)
        .wrap(Wrap { trim: true });
    f.render_widget(p, inner);
}

fn draw_confirm_upgrade_popup(f: &mut Frame, area: Rect, app: &App) {
    let sz = app.config.tui.popup_confirm.unwrap_or([60, 30]);
    let popup_area = centered_rect(sz[0], sz[1], area);
    f.render_widget(Clear, popup_area);
    let block = Block::default()
        .title(" Confirm upgrade ")
        .borders(Borders::ALL)
        .border_type(app.border_type)
        .border_style(if app.no_color {
            Style::default()
        } else {
            app.styles.popup_title
        })
        .style(if app.no_color {
            Style::default()
        } else {
            app.styles.popup_bg
        });
    let inner = block.inner(popup_area);
    f.render_widget(block, popup_area);
    let mut text = vec![Line::from("Upgrade full system?"), Line::raw("")];
    // 5.1: news-intervention warning inline — the single-step replacement
    // for the old press-U-twice dance.
    if let Some(warn) = app.upgrade_warn.as_deref().filter(|w| !w.trim().is_empty()) {
        text.push(Line::from(vec![Span::styled(
            format!("Manual intervention? {warn}"),
            if app.no_color {
                Style::default().add_modifier(Modifier::BOLD)
            } else {
                app.styles.out_of_date
            },
        )]));
        text.push(Line::raw(""));
    }
    text.extend([
        Line::from(vec![
            Span::styled(
                "Y",
                if app.no_color {
                    Style::default().add_modifier(Modifier::BOLD)
                } else {
                    app.styles.installed
                },
            ),
            Span::raw("/Enter = yes  "),
            Span::styled(
                "N/Esc",
                if app.no_color {
                    Style::default().add_modifier(Modifier::BOLD)
                } else {
                    app.styles.out_of_date
                },
            ),
            Span::raw(" = cancel"),
        ]),
        Line::raw(""),
        Line::from(Span::styled(
            "Will run: sudo pacman -Syu + flatpak update [needs password]",
            if app.no_color {
                Style::default()
            } else {
                app.styles.text_dim
            },
        )),
    ]);
    let p = Paragraph::new(text)
        .alignment(ratatui::layout::Alignment::Center)
        .wrap(Wrap { trim: true });
    f.render_widget(p, inner);
}

fn draw_confirm_clean_popup(
    f: &mut Frame,
    files: usize,
    bytes: u64,
    flatpak: bool,
    area: Rect,
    app: &App,
) {
    // ui-design: one focal number (reclaimable size), one primary CTA.
    let sz = app.config.tui.popup_confirm.unwrap_or([60, 30]);
    let popup_area = centered_rect(sz[0], sz[1], area);
    f.render_widget(Clear, popup_area);
    let block = Block::default()
        .title(" Confirm clean ")
        .borders(Borders::ALL)
        .border_type(app.border_type)
        .border_style(if app.no_color {
            Style::default()
        } else {
            app.styles.popup_title
        })
        .style(if app.no_color {
            Style::default()
        } else {
            app.styles.popup_bg
        });
    let inner = block.inner(popup_area);
    f.render_widget(block, popup_area);
    let size = crate::stats::human_bytes(bytes);
    let text = vec![
        Line::from(vec![
            Span::raw("Remove "),
            Span::styled(
                format!("{files} cached files ({size})"),
                if app.no_color {
                    Style::default().add_modifier(Modifier::BOLD)
                } else {
                    app.styles.installed
                },
            ),
            Span::raw(" ?"),
        ]),
        Line::raw(""),
        Line::from(vec![
            Span::styled(
                "Y",
                if app.no_color {
                    Style::default().add_modifier(Modifier::BOLD)
                } else {
                    app.styles.installed
                },
            ),
            Span::raw("/Enter = yes  "),
            Span::styled(
                "N/Esc",
                if app.no_color {
                    Style::default().add_modifier(Modifier::BOLD)
                } else {
                    app.styles.out_of_date
                },
            ),
            Span::raw(" = cancel"),
        ]),
        Line::raw(""),
        Line::from(Span::styled(
            if flatpak {
                "Will run: paccache -r + flatpak --unused"
            } else {
                "Will run: sudo paccache -r -kN [needs password]"
            },
            if app.no_color {
                Style::default()
            } else {
                app.styles.text_dim
            },
        )),
    ];
    let p = Paragraph::new(text)
        .alignment(ratatui::layout::Alignment::Center)
        .wrap(Wrap { trim: true });
    f.render_widget(p, inner);
}

fn draw_help_popup(f: &mut Frame, area: Rect, app: &App) {
    let sz = app.config.tui.popup_help.unwrap_or([70, 60]);
    let popup_area = centered_rect(sz[0], sz[1], area);
    f.render_widget(Clear, popup_area);
    let block = Block::default()
        .title(" Help (?) ")
        .borders(Borders::ALL)
        .border_type(app.border_type)
        .border_style(if app.no_color {
            Style::default()
        } else {
            app.styles.popup_title
        })
        .style(if app.no_color {
            Style::default()
        } else {
            app.styles.popup_bg
        });
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
        .style(if app.no_color {
            Style::default()
        } else {
            app.styles.text
        });
    f.render_widget(p, inner);
}

/// Char-boundary-safe fixed-width fit: truncate to `w` display columns,
/// then pad with spaces to exactly `w` (the search-field truncation
/// discipline, adapted for table-like cells).
fn fit(s: &str, w: usize) -> String {
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

/// Settings-modal fixed columns (plan §4: entries start col, rail inline-end).
const KEY_COL: usize = 16;
const VALUE_COL: usize = 12;
const RAIL_COL: usize = 19; // "Behavior & safety" (17) + padding

fn draw_settings_modal(f: &mut Frame, app: &App, area: Rect) {
    let Some(s) = app.settings.as_ref() else {
        return;
    };
    let popup_area = centered_rect(94, 90, area);
    f.render_widget(Clear, popup_area);
    let count = s.dirty_count();
    let title = if count > 0 {
        format!(" settings *{count} — / filter · Enter edit · Esc discard ")
    } else {
        " settings — / filter · Enter edit · Esc discard ".to_string()
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(app.border_type)
        .title(title)
        .border_style(if app.no_color {
            Style::default()
        } else {
            app.styles.popup_title
        })
        .style(if app.no_color {
            Style::default()
        } else {
            app.styles.popup_bg
        });
    let inner = block.inner(popup_area);
    f.render_widget(block, popup_area);
    if inner.width < 30 || inner.height < 6 {
        return; // below even the 60×14 floor margin — never draw torn
    }

    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(4),    // rows + rail
            Constraint::Length(1), // theme preview strip
            Constraint::Length(2), // context + info lines
        ])
        .split(inner);
    let h = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(24),                 // entries column (start side)
            Constraint::Length(RAIL_COL as u16), // category rail (inline-end)
        ])
        .split(v[0]);

    let use_color = !app.no_color;

    // --- entries column -----------------------------------------------------
    let ids = s.visible();
    let items: Vec<ListItem> = ids.iter().map(|&i| build_row_item(app, s, i)).collect();
    let mut state = ListState::default();
    state.select(if ids.is_empty() {
        None
    } else {
        Some(s.selection.min(ids.len() - 1))
    });
    let entries = List::new(items)
        .highlight_style(if use_color {
            app.styles.highlight
        } else {
            Style::default().add_modifier(Modifier::REVERSED)
        })
        .highlight_symbol("▸ ");
    f.render_stateful_widget(entries, h[0], &mut state);

    // --- category rail (inline-end) -----------------------------------------
    let rail_items: Vec<ListItem> = s
        .groups
        .iter()
        .map(|g| ListItem::new(g.as_label()))
        .collect();
    let rail_block = Block::default()
        .borders(Borders::ALL)
        .border_type(app.border_type)
        .title(" categories ")
        .border_style(if use_color {
            app.styles.popup_title
        } else {
            Style::default()
        });
    let rail_inner = rail_block.inner(h[1]);
    f.render_widget(rail_block, h[1]);
    let mut rail_state = ListState::default();
    rail_state.select(if s.groups.is_empty() {
        None
    } else {
        Some(s.rail_sel.min(s.groups.len() - 1))
    });
    let rail = List::new(rail_items)
        .highlight_style(if use_color {
            app.styles.highlight
        } else {
            Style::default().add_modifier(Modifier::REVERSED)
        })
        .highlight_symbol("");
    f.render_stateful_widget(rail, rail_inner, &mut rail_state);

    // --- theme preview strip (draft-theme, always colored: NO_COLOR dims the
    // rest of the UI; the strip is literally what the palette keys edit) ----
    let preview = Paragraph::new(Line::from(preview_line(s))).style(if use_color {
        app.styles.text
    } else {
        Style::default().add_modifier(Modifier::BOLD)
    });
    f.render_widget(preview, v[1]);

    // --- context + info lines ------------------------------------------------
    draw_settings_detail(f, app, s, v[2]);
}

/// Danger/caution emphasis slots — plan §4 reuses `border_error` /
/// `out_of_date`; no_color falls back to BOLD.
fn settings_key_style(app: &App, danger: crate::settings::catalog::Danger) -> Style {
    let use_color = !app.no_color;
    match danger.as_str() {
        "danger" if use_color => app.styles.border_error,
        "caution" if use_color => app.styles.out_of_date,
        "danger" | "caution" => Style::default().add_modifier(Modifier::BOLD),
        _ if use_color => app.styles.text,
        _ => Style::default(),
    }
}

fn settings_origin_style(app: &App, origin: &crate::settings::origin::Origin) -> Style {
    if app.no_color {
        return Style::default();
    }
    match origin {
        crate::settings::origin::Origin::UserFile(_) => app.styles.help,
        crate::settings::origin::Origin::ProjectFile(_) => app.styles.votes,
        _ => app.styles.text_dim,
    }
}

/// The value-editor hint line per kind (engine exactness where it exists).
fn settings_edit_hint(meta: &crate::settings::catalog::SettingMeta) -> String {
    use crate::settings::catalog::Kind;
    if let Some((min, max)) = crate::settings::validate::numeric_clamp(meta.key) {
        return format!("range {min}..={max} — Enter applies, Esc cancels");
    }
    match meta.kind {
        Kind::Enum(vals) => format!(
            "enum — Enter cycles: {}",
            vals.iter().map(|v| v.name).collect::<Vec<_>>().join("|")
        ),
        Kind::PairU16 => "two numbers (cols,rows), e.g. 70,24 — Enter applies".into(),
        Kind::Style => "style words, e.g. 'black on cyan bold' — Enter applies".into(),
        Kind::OptText => "blank = unset — Enter applies, Esc cancels".into(),
        Kind::Bool => String::new(), // bools toggle with Space, never edit
        _ => "type a value · Enter applies · Esc cancels".into(),
    }
}

/// One entry row: `key  value  [origin] apply` with draft/error emphasis.
fn build_row_item<'a>(app: &'a App, s: &'a SettingsModal, i: usize) -> ListItem<'a> {
    let row = &s.rows[i];
    let meta = row.meta;
    let cur = row.draft.clone().unwrap_or_else(|| row.stored.clone());
    let changed = row.draft.as_deref().is_some_and(|v| *v != row.stored);
    let use_color = !app.no_color;
    let key_style = settings_key_style(app, meta.danger);
    let plain = if use_color {
        app.styles.text
    } else {
        Style::default()
    };
    let value_text = match meta.kind {
        crate::settings::catalog::Kind::Bool => format!("[{}]", cur == "true"),
        _ => cur.clone(),
    };
    let value_style = if row.error.is_some() {
        if use_color {
            app.styles.border_error
        } else {
            Style::default().add_modifier(Modifier::BOLD)
        }
    } else if changed {
        // REVERSED pops in both palettes without depending on theme colors.
        Style::default().add_modifier(Modifier::REVERSED)
    } else {
        plain
    };

    let mut spans = vec![
        Span::styled(fit(meta.key, KEY_COL), key_style),
        Span::raw(" "),
        Span::styled(fit(value_text.as_str(), VALUE_COL), value_style),
        Span::styled(
            format!(
                " [{}]{}",
                row.origin.as_label(),
                if changed { "*" } else { "" }
            ),
            settings_origin_style(app, &row.origin),
        ),
    ];
    if meta.apply.as_str() != "immediate" {
        spans.push(Span::styled(
            format!(" {}", meta.apply.as_str()),
            if use_color {
                app.styles.text_dim
            } else {
                Style::default()
            },
        ));
    }
    ListItem::new(Line::from(spans))
}

/// The live theme strip: rebuilt from the modal's DRAFT theme per frame —
/// style edits preview instantly before any save (plan §4). Intentionally
/// ignores NO_COLOR: the strip is what palette keys edit.
fn preview_line(s: &SettingsModal) -> Vec<Span<'static>> {
    let t = crate::config::ThemeStyles::from(&s.draft.theme);
    vec![
        Span::styled("preview ", t.text_dim),
        Span::styled("core ", t.repo_core),
        Span::styled("extra ", t.repo_extra),
        Span::styled("multilib ", t.repo_multilib),
        Span::styled("aur ", t.repo_aur),
        Span::styled("install ", t.installed),
        Span::styled("out-of-date ", t.out_of_date),
        Span::styled("selected ", t.highlight),
        Span::styled("dim ", t.text_dim),
        Span::styled("help ", t.help),
        Span::styled("popup ", t.popup_title),
    ]
}

/// Detail footer: line 1 = context (filter / editor / selected summary with
/// native cursor over the capture line), line 2 = row error > editing hint >
/// description / filter census. Priority: error never hides behind the hint.
fn draw_settings_detail(f: &mut Frame, app: &App, s: &SettingsModal, area: Rect) {
    let use_color = !app.no_color;
    let dim_style = if use_color {
        app.styles.text_dim
    } else {
        Style::default()
    };
    let text_style = if use_color {
        app.styles.text
    } else {
        Style::default()
    };
    let err_style = if use_color {
        app.styles.border_error
    } else {
        Style::default().add_modifier(Modifier::BOLD)
    };

    let line1: Vec<Span<'static>>;
    let line2: Vec<Span<'static>>;
    // (prefix width, visual cursor, buffer width) — cursor types here only.
    let cursor: Option<(usize, usize, usize)>;

    if let Some(ed) = &s.editor {
        let idx = ed.row.min(s.rows.len().saturating_sub(1));
        let prefix = format!("{}> ", s.rows[idx].meta.key);
        let text = ed.buf.value().to_string();
        line1 = vec![
            Span::styled(prefix.clone(), dim_style),
            Span::styled(text.clone(), text_style),
        ];
        cursor = Some((prefix.width(), ed.buf.visual_cursor(), text.width()));
        line2 = match &s.rows[idx].error {
            Some(err) => vec![Span::styled(format!("⚠ {err}"), err_style)],
            None => vec![Span::styled(
                settings_edit_hint(s.rows[idx].meta),
                dim_style,
            )],
        };
    } else if s.filter_focus {
        let text = s.filter.value().to_string();
        line1 = vec![
            Span::styled("filter> ".to_string(), dim_style),
            Span::styled(text.clone(), text_style),
        ];
        cursor = Some(("filter> ".width(), s.filter.visual_cursor(), text.width()));
        line2 = vec![Span::styled(
            format!(
                "{}/{} keys — Enter leaves the filter, Esc clears it",
                s.visible().len(),
                s.rows.len()
            ),
            dim_style,
        )];
    } else if let Some(row) = s.selected_row() {
        let meta = row.meta;
        let mut spans = vec![
            Span::styled(meta.key.to_string(), settings_key_style(app, meta.danger)),
            Span::styled(
                format!(" [{}]", row.origin.as_label()),
                settings_origin_style(app, &row.origin),
            ),
        ];
        match meta.danger.as_str() {
            "danger" => spans.extend([Span::styled(" danger", err_style)]),
            "caution" => spans.extend([Span::styled(
                " caution",
                if use_color {
                    app.styles.out_of_date
                } else {
                    Style::default().add_modifier(Modifier::BOLD)
                },
            )]),
            _ => {}
        }
        if meta.apply.as_str() != "immediate" {
            spans.push(Span::styled(
                format!(" apply:{}", meta.apply.as_str()),
                dim_style,
            ));
        }
        let pending = row.draft.as_deref().is_some_and(|v| v != row.stored);
        if pending {
            spans.push(Span::styled(
                format!(
                    "  ({} → {})",
                    row.stored,
                    row.draft.clone().unwrap_or_default()
                ),
                dim_style,
            ));
        }
        line1 = spans;
        line2 = match &row.error {
            Some(err) => vec![Span::styled(format!("⚠ {err}"), err_style)],
            None => vec![Span::styled(meta.desc.to_string(), dim_style)],
        };
        cursor = None;
    } else {
        line1 = vec![Span::styled(
            "no rows match the filter".to_string(),
            dim_style,
        )];
        line2 = Vec::new();
        cursor = None;
    }

    f.render_widget(
        Paragraph::new(vec![Line::from(line1), Line::from(line2)]),
        area,
    );

    if let Some((prefix, vis, buf_w)) = cursor {
        // Mirror draw_search: when the buffer outgrows the line, shift the
        // cursor left by the hidden width so it tracks the visible tail.
        let avail = (area.width as usize).saturating_sub(prefix + 1);
        let vis = if buf_w > avail {
            vis.saturating_sub(buf_w.saturating_sub(avail))
        } else {
            vis
        }
        .min(avail);
        let x = area.x + (prefix as u16) + (vis as u16).min(avail as u16);
        f.set_cursor_position((x, area.y));
    }
}

fn draw_message_popup(f: &mut Frame, msg: &str, area: Rect, app: &App) {
    let sz = app.config.tui.popup_message.unwrap_or([60, 20]);
    let popup_area = centered_rect(sz[0], sz[1], area);
    f.render_widget(Clear, popup_area);
    let block = Block::default()
        .title(" Message ")
        .borders(Borders::ALL)
        .border_type(app.border_type)
        .border_style(if app.no_color {
            Style::default()
        } else if msg.starts_with('✓') {
            app.styles.installed
        } else {
            app.styles.out_of_date
        })
        .style(if app.no_color {
            Style::default()
        } else {
            app.styles.popup_bg
        });
    let inner = block.inner(popup_area);
    f.render_widget(block, popup_area);
    let p = Paragraph::new(msg.to_string())
        .style(if app.no_color {
            Style::default()
        } else {
            app.styles.text
        })
        .alignment(ratatui::layout::Alignment::Center)
        .wrap(Wrap { trim: true });
    f.render_widget(p, inner);
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    // Phase D5: cache-free, no Layout alloc — manual centered math (vs 2 Layout splits before).
    // Clamped to the area so oversized percents (>100) can never produce an
    // out-of-bounds rect. u32 math: width * percent can overflow u16 on wide
    // terminals (debug builds panic on overflow).
    let popup_width = ((r.width as u32 * percent_x as u32) / 100).min(r.width as u32) as u16;
    let popup_height = ((r.height as u32 * percent_y as u32) / 100).min(r.height as u32) as u16;
    let popup_x = r.x + (r.width.saturating_sub(popup_width)) / 2;
    let popup_y = r.y + (r.height.saturating_sub(popup_height)) / 2;
    Rect::new(popup_x, popup_y, popup_width, popup_height)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centered_rect_stays_in_bounds() {
        let area = Rect::new(0, 0, 80, 24);
        let p = centered_rect(60, 30, area);
        assert!(p.x + p.width <= area.width);
        assert!(p.y + p.height <= area.height);
        // Oversized percents clamp to the full area instead of overflowing.
        let big = centered_rect(200, 200, area);
        assert_eq!(
            (big.x, big.y, big.width, big.height),
            (0u16, 0u16, 80u16, 24u16)
        );
        // Degenerate area never underflows.
        let tiny = centered_rect(60, 30, Rect::new(0, 0, 0, 0));
        assert_eq!((tiny.width, tiny.height), (0u16, 0u16));
    }
}
