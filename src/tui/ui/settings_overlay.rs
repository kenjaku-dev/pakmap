//! The settings overlay (overhaul phase 2): the whole `S` modal — entries
//! column, category rail, live theme preview strip, and the detail footer —
//! extracted from the old `ui.rs` monolith once (plan §3 risk note: the
//! split lands *before* any restyle so the pinned tests migrate once).
//!
//! Content is the v0.10.0 modal, byte-for-byte: geometry (94×90 centered,
//! `KEY_COL(16) value(12) rail(19)`), copy, cursor mirror math and the
//! deliberate always-colored preview strip are all unchanged — the pinned
//! **render hash test** at the bottom proves the equivalence (symbols +
//! style encodings over the modal region).

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Clear, List, ListItem, ListState, Paragraph},
};
use unicode_width::UnicodeWidthStr;

use super::layout;
use crate::tui::app::App;
use crate::tui::settings::SettingsModal;
use crate::tui::style::ThemeStyles;

/// Settings-modal fixed columns (plan §4: entries start col, rail inline-end).
const KEY_COL: usize = 16;
const VALUE_COL: usize = 12;
const RAIL_COL: usize = 19; // "Behavior & safety" (17) + padding

pub fn draw_settings_modal(f: &mut Frame, app: &App, area: Rect) {
    let Some(s) = app.settings.as_ref() else {
        return;
    };
    let pal = app.palette();
    let popup_area = layout::popup_rect(94, 90, area);
    f.render_widget(Clear, popup_area);
    let count = s.dirty_count();
    let title = if count > 0 {
        format!(" settings *{count} — / filter · Enter edit · Esc discard ")
    } else {
        " settings — / filter · Enter edit · Esc discard ".to_string()
    };
    let block = layout::overlay_block(pal, app.border_type, title, pal.popup_title);
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

    // --- entries column -----------------------------------------------------
    let ids = s.visible();
    let items: Vec<ListItem> = ids.iter().map(|&i| build_row_item(pal, s, i)).collect();
    let mut state = ListState::default();
    state.select(if ids.is_empty() {
        None
    } else {
        Some(s.selection.min(ids.len() - 1))
    });
    let entries = List::new(items)
        .highlight_style(pal.highlight)
        .highlight_symbol("▸ ");
    f.render_stateful_widget(entries, h[0], &mut state);

    // --- category rail (inline-end) -----------------------------------------
    let rail_items: Vec<ListItem> = s
        .groups
        .iter()
        .map(|g| ListItem::new(g.as_label()))
        .collect();
    let rail_block = layout::framed(" categories ", app.border_type).border_style(pal.popup_title);
    let rail_inner = rail_block.inner(h[1]);
    f.render_widget(rail_block, h[1]);
    let mut rail_state = ListState::default();
    rail_state.select(if s.groups.is_empty() {
        None
    } else {
        Some(s.rail_sel.min(s.groups.len() - 1))
    });
    let rail = List::new(rail_items)
        .highlight_style(pal.highlight)
        .highlight_symbol("");
    f.render_stateful_widget(rail, rail_inner, &mut rail_state);

    // --- theme preview strip (draft-theme, always colored: NO_COLOR dims the
    // rest of the UI; the strip is literally what the palette keys edit) ----
    let preview = Paragraph::new(Line::from(preview_line(s))).style(pal.preview_banner());
    f.render_widget(preview, v[1]);

    // --- context + info lines ------------------------------------------------
    draw_settings_detail(f, pal, s, v[2]);
}

/// Danger/caution emphasis slots (plan §4): `border_error` / `out_of_date`
/// from the active palette — in mono those entries are exactly the old
/// no_color BOLD fallbacks.
fn settings_key_style(pal: &ThemeStyles, danger: crate::settings::catalog::Danger) -> Style {
    match danger.as_str() {
        "danger" => pal.border_error,
        "caution" => pal.out_of_date,
        _ => pal.text,
    }
}

fn settings_origin_style(pal: &ThemeStyles, origin: &crate::settings::origin::Origin) -> Style {
    match origin {
        crate::settings::origin::Origin::UserFile(_) => pal.help,
        crate::settings::origin::Origin::ProjectFile(_) => pal.votes,
        _ => pal.text_dim,
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
fn build_row_item<'a>(pal: &'a ThemeStyles, s: &'a SettingsModal, i: usize) -> ListItem<'a> {
    let row = &s.rows[i];
    let meta = row.meta;
    let cur = row.draft.clone().unwrap_or_else(|| row.stored.clone());
    let changed = row.draft.as_deref().is_some_and(|v| *v != row.stored);
    let key_style = settings_key_style(pal, meta.danger);
    let value_text = match meta.kind {
        crate::settings::catalog::Kind::Bool => format!("[{}]", cur == "true"),
        _ => cur.clone(),
    };
    let value_style = if row.error.is_some() {
        pal.border_error
    } else if changed {
        // REVERSED pops in both palettes without depending on theme colors.
        pal.draft_changed()
    } else {
        pal.text
    };

    let mut spans = vec![
        Span::styled(layout::fit(meta.key, KEY_COL), key_style),
        Span::raw(" "),
        Span::styled(layout::fit(value_text.as_str(), VALUE_COL), value_style),
        Span::styled(
            format!(
                " [{}]{}",
                row.origin.as_label(),
                if changed { "*" } else { "" }
            ),
            settings_origin_style(pal, &row.origin),
        ),
    ];
    if meta.apply.as_str() != "immediate" {
        spans.push(Span::styled(
            format!(" {}", meta.apply.as_str()),
            pal.text_dim,
        ));
    }
    ListItem::new(Line::from(spans))
}

/// The live theme strip: rebuilt from the modal's DRAFT theme per frame —
/// style edits preview instantly before any save (plan §4). Intentionally
/// ignores NO_COLOR: the strip is what palette keys edit.
fn preview_line(s: &SettingsModal) -> Vec<Span<'static>> {
    let t = crate::tui::style::ThemeStyles::colored(&s.draft.theme);
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
fn draw_settings_detail(f: &mut Frame, pal: &ThemeStyles, s: &SettingsModal, area: Rect) {
    let dim_style = pal.text_dim;
    let text_style = pal.text;
    let err_style = pal.border_error;

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
            Span::styled(meta.key.to_string(), settings_key_style(pal, meta.danger)),
            Span::styled(
                format!(" [{}]", row.origin.as_label()),
                settings_origin_style(pal, &row.origin),
            ),
        ];
        match meta.danger.as_str() {
            "danger" => spans.extend([Span::styled(" danger", err_style)]),
            "caution" => spans.extend([Span::styled(" caution", pal.out_of_date)]),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use ratatui::{Terminal, backend::TestBackend};

    fn fnv1a(data: &[u8], mut hash: u64) -> u64 {
        // FNV-1a 64-bit prime & offset
        for b in data {
            hash ^= u64::from(*b);
            hash = hash.wrapping_mul(0x100_0000_01b3);
        }
        hash
    }

    /// Render the full frame at 100×40 with the settings modal open over a
    /// hermetic default config and collect, over the modal region only,
    /// (per-line trimmed text, a fnv1a hash over every cell's symbol plus
    /// its fg/bg/underline/modifier encodings).
    fn modal_render() -> (String, u64) {
        let area = Rect::new(0, 0, 100, 40);
        let mut terminal = Terminal::new(TestBackend::new(100, 40)).unwrap();
        let mut app = App::new(String::new());
        app.no_color = false; // hermetic: NO_COLOR env must not shift the pin
        app.focus = crate::tui::app::Focus::List;
        app.settings = Some(SettingsModal::new(Config::default(), Vec::new(), None));
        terminal.draw(|f| crate::tui::ui::draw(f, &app)).unwrap();
        let buf = terminal.backend().buffer();
        let r = layout::popup_rect(94, 90, area);
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        let mut text = String::new();
        for y in r.y..r.y + r.height {
            let mut line = String::new();
            for x in r.x..r.x + r.width {
                let c = &buf[(x, y)];
                let encoding = format!(
                    "{:?}|{:?}|{:?}|{:?}|{:?}",
                    c.symbol(),
                    c.fg,
                    c.bg,
                    c.underline_color,
                    c.modifier
                );
                hash = fnv1a(encoding.as_bytes(), hash);
                line.push_str(c.symbol());
            }
            text.push_str(line.trim_end());
            text.push('\n');
        }
        (text, hash)
    }

    /// Content sanity with human-visible diagnostics (the hash pin below is
    /// the exact gate; these explain a regression in prose).
    #[test]
    fn settings_modal_renders_the_three_bands() {
        let (text, _) = modal_render();
        assert!(text.contains("settings"), "title: {text}");
        assert!(text.contains("categories"), "rail: {text}");
        assert!(text.contains("preview "), "strip: {text}");
        assert!(text.contains("[default]"), "origins: {text}");
    }

    /// Acceptance (plan §3, D7): `draw_settings_modal` renders byte-identical
    /// content through the new component set. Pre-split fingerprint of the
    /// modal region (100×40, config defaults, dated to the pre-phase-2
    /// `ui.rs` monolith): a mismatch means the split disturbed the modal —
    /// fix the file, not the test; a deliberate restyle re-pins with an
    /// owner-noted change. Re-pinned for phase 4 (v0.11): the catalog grew
    /// by `tui.preview_pane` (the Appearance band renders one row taller);
    /// no other render disturbance.
    #[test]
    fn settings_modal_render_is_byte_identical_through_the_split() {
        let (_, hash) = modal_render();
        assert_eq!(
            hash, 0x6e47_d206_9df3_0cb8,
            "the settings modal render changed during the ui.rs split"
        );
    }
}
