//! TUI settings modal (plan §4): every config key, visible and editable in
//! the terminal door. Phase 3 of the settings subsystem.
//!
//! Vantage: the modal is built from a fresh engine resolve — rows describe
//! what the config files (user + trusted project) say right now, exactly
//! what `pakmap config list`/GUI show, independent from the session's boot
//! merges (CLI flags are session overlays, not file truth).
//!
//! Write model: drafts live in this modal only; `s` (save-all) writes each
//! changed key through `settings::write` to the USER file with
//! `create:false` (the engine never creates files silently, mirroring the
//! CLI's unflagged default; refusals surface verbatim). One engine call per
//! changed key, stop at the first failure, no rollback tee (plan §4). Only
//! `[theme]` keys live-apply in-session (styles/no_color rebuild); every
//! other key applies on relaunch. The TUI never writes project files (D2).
//!
//! Keys: Up/Down move, `/` filter, `Space` toggles bools, `Enter` cycles
//! enums or opens the value editor (tui-input mechanics, same as search),
//! `r` resets a row, `s` saves all, `e` opens `$EDITOR`, Left/Right jump
//! categories, `Esc` discards/closes. Rendering lives in `tui/ui.rs`.

use std::path::PathBuf;

use color_eyre::Result;
use crossterm::event::{Event, KeyEventKind};
use ratatui::DefaultTerminal;
use tui_input::{Input, backend::crossterm::EventHandler};

use super::app::{App, Popup};
use super::suspend_and_run;
use crate::config::Config;
use crate::settings::catalog::{self, CATALOG, Group, Kind, section_of};
use crate::settings::origin::Origin;
use crate::settings::write::{self, SetError, Target};

/// One row of the modal: exactly one editable catalog key (surfaces.tui).
pub struct Row {
    pub meta: &'static catalog::SettingMeta,
    /// Rendered value at modal open (`FieldId::get` over the resolved config).
    pub stored: String,
    /// Edited value since open (normalized render). `None` = untouched.
    pub draft: Option<String>,
    /// Last failed validation/commit of this row; cleared on reset or a
    /// successful re-commit. Never blocks navigation, only redraw + save.
    pub error: Option<String>,
    pub origin: Origin,
}

/// The capture box for text/number edits (search-input mechanics).
pub struct Editor {
    /// Row index (`rows[..]`) being edited.
    pub row: usize,
    pub buf: Input,
}

/// Result of one save-all press (App maps this to popups/status + live-apply).
#[derive(Debug)]
pub enum CommitOutcome {
    /// Nothing to write — no draft differs from its stored value.
    Nothing,
    Saved {
        written: usize,
        warnings: Vec<String>,
        /// How many of `written` were `[theme]` keys (live-apply in session).
        theme_written: usize,
    },
    Failed {
        /// Writes that already landed before this one failed (no tee/rollback
        /// per plan §4 — each key commits atomically and independently).
        written: usize,
        error: SetError,
    },
}

pub struct SettingsModal {
    /// Resolved config at open — the reset target for `r`.
    pub base: Config,
    /// Working copy; every accepted edit lands here immediately (drives the
    /// live theme preview and bool/enum values).
    pub draft: Config,
    pub rows: Vec<Row>,
    /// User config file — the only write target (`None` = no path, save
    /// refuses; engine refusals surface verbatim).
    pub user_path: Option<PathBuf>,
    /// Selection into `visible()` (filtered list).
    pub selection: usize,
    pub filter: Input,
    pub filter_focus: bool,
    pub editor: Option<Editor>,
    /// Category rail (distinct groups of the visible rows, CATALOG order);
    /// rebuilt after every filter/selection change.
    pub groups: Vec<Group>,
    /// Selection into `groups` — follows movement, driven by ←/→.
    pub rail_sel: usize,
}

fn origin_of(origins: &[(String, Origin)], key: &str) -> Origin {
    origins
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, o)| o.clone())
        .unwrap_or(Origin::Default)
}

impl SettingsModal {
    /// Build from an engine resolve (`App::open_settings` resolves for the
    /// session's `--config` replacement; tests pass explicit fixtures).
    pub fn new(config: Config, origins: Vec<(String, Origin)>, user_path: Option<PathBuf>) -> Self {
        let rows = CATALOG
            .iter()
            .filter(|m| m.surfaces.tui)
            .map(|meta| Row {
                meta,
                stored: meta.field.get(&config),
                draft: None,
                error: None,
                origin: origin_of(&origins, meta.key),
            })
            .collect();
        let mut modal = Self {
            base: config.clone(),
            draft: config,
            rows,
            user_path,
            selection: 0,
            filter: Input::default(),
            filter_focus: false,
            editor: None,
            groups: Vec::new(),
            rail_sel: 0,
        };
        modal.sync_rail();
        modal
    }

    /// Row indices shown now (filter applied, CATALOG order).
    pub fn visible(&self) -> Vec<usize> {
        let q = self.filter.value().to_ascii_lowercase();
        if q.is_empty() {
            return (0..self.rows.len()).collect();
        }
        self.rows
            .iter()
            .enumerate()
            .filter(|(_, r)| {
                r.meta.key.to_ascii_lowercase().contains(&q)
                    || r.meta.desc.to_ascii_lowercase().contains(&q)
                    || r.meta.group.as_label().to_ascii_lowercase().contains(&q)
            })
            .map(|(i, _)| i)
            .collect()
    }

    /// The currently selected row, or `None` when the filter matched nothing.
    pub fn selected_row(&self) -> Option<&Row> {
        self.visible().get(self.selection).map(|&i| &self.rows[i])
    }

    fn selected_row_index(&self) -> Option<usize> {
        self.visible().get(self.selection).copied()
    }

    /// Rebuild the rail (distinct groups of visible rows) + track selection.
    /// Groups appear in CATALOG order — a contiguous column, exactly one
    /// highlighted at a time.
    pub fn sync_rail(&mut self) {
        let ids = self.visible();
        self.groups.clear();
        for &i in &ids {
            let g = self.rows[i].meta.group;
            if self.groups.last() != Some(&g) {
                self.groups.push(g);
            }
        }
        if let Some(&i) = ids.get(self.selection)
            && let Some(gi) = self
                .groups
                .iter()
                .position(|g| *g == self.rows[i].meta.group)
        {
            self.rail_sel = gi;
        }
    }

    /// After a filter edit: clamp the selection into the filtered list.
    pub fn sync_after_filter(&mut self) {
        let n = self.visible().len();
        if self.selection >= n {
            self.selection = n.saturating_sub(1);
        }
        self.sync_rail();
    }

    /// Wrap-around selection move (App's list discipline).
    pub fn move_sel(&mut self, delta: isize) {
        let n = self.visible().len();
        if n == 0 {
            return;
        }
        self.selection = (self.selection as isize + delta).rem_euclid(n as isize) as usize;
        self.sync_rail();
    }

    /// ←/→: jump the selection to the first row of the previous/next group.
    pub fn jump_group(&mut self, delta: isize) {
        let n = self.groups.len();
        if n == 0 {
            return;
        }
        self.rail_sel = (self.rail_sel as isize + delta).rem_euclid(n as isize) as usize;
        let target = self.groups[self.rail_sel];
        let ids = self.visible();
        if let Some(pos) = ids.iter().position(|&i| self.rows[i].meta.group == target) {
            self.selection = pos;
        }
    }

    /// Move the selection to the first row matching `key` (tests + future
    /// affordances). Nothing matches = selection stays.
    pub fn select_key(&mut self, key: &str) -> bool {
        let ids = self.visible();
        if let Some(pos) = ids.iter().position(|&i| self.rows[i].meta.key == key) {
            self.selection = pos;
            self.sync_rail();
            true
        } else {
            false
        }
    }

    /// `Space` on a bool row: flip it in the draft config. No-op elsewhere.
    pub fn bool_toggle(&mut self) {
        let Some(i) = self.selected_row_index() else {
            return;
        };
        if self.rows[i].meta.kind != Kind::Bool {
            return;
        }
        let meta = self.rows[i].meta;
        let next = if meta.field.get(&self.draft) == "true" {
            "false"
        } else {
            "true"
        };
        if meta.field.set(&mut self.draft, next).is_ok() {
            self.rows[i].draft = Some(meta.field.get(&self.draft));
            self.rows[i].error = None;
        }
    }

    /// `Enter` on an enum row: cycle to the next value in CATALOG order
    /// (wraps). No-op on other kinds (bools are Space-only per plan §4).
    pub fn enum_cycle(&mut self) {
        let Some(i) = self.selected_row_index() else {
            return;
        };
        let vals = match self.rows[i].meta.kind {
            Kind::Enum(vals) => vals,
            _ => return,
        };
        let meta = self.rows[i].meta;
        let cur = meta.field.get(&self.draft);
        let idx = vals.iter().position(|v| v.name == cur).unwrap_or(0);
        let next = vals[(idx + 1) % vals.len()].name;
        if meta.field.set(&mut self.draft, next).is_ok() {
            self.rows[i].draft = Some(meta.field.get(&self.draft));
            self.rows[i].error = None;
        }
    }

    /// `Enter` dispatch on a clean press: enums cycle, other non-bool kinds
    /// open the value editor, bools intentionally inert.
    pub fn enter_selected(&mut self) {
        let kind = match self.selected_row() {
            Some(r) => r.meta.kind,
            None => return,
        };
        match kind {
            Kind::Enum(_) => self.enum_cycle(),
            Kind::Bool => {}
            _ => self.begin_edit(),
        }
    }

    /// Open the capture box seeded with the row's current value. Style
    /// strings seed verbatim; `False`-flavored kinds (bool/enum) never open
    /// this box per `enter_selected`.
    pub fn begin_edit(&mut self) {
        let Some(i) = self.selected_row_index() else {
            return;
        };
        let value = self.rows[i].meta.field.get(&self.draft);
        let mut buf = Input::default();
        pump_input(&mut buf, &value);
        self.editor = Some(Editor { row: i, buf });
    }

    /// Commit the capture box: engine validation first (the same chain as
    /// `write::set`, without touching files), then the kind parser. On
    /// failure the error sticks to the row and the editor stays open for
    /// fixing; on success the edit lands in the draft config + row.
    pub fn commit_edit(&mut self) {
        let Some(editor) = self.editor.take() else {
            return;
        };
        let i = editor.row;
        let raw = editor.buf.value().to_string();
        let meta = self.rows[i].meta;
        let committed = crate::settings::write::validate_value(meta, &raw)
            .and_then(|()| meta.field.set(&mut self.draft, &raw));
        match committed {
            Ok(()) => {
                self.rows[i].draft = Some(meta.field.get(&self.draft));
                self.rows[i].error = None;
            }
            Err(msg) => {
                self.rows[i].error = Some(msg);
                self.editor = Some(editor); // keep the buffer for fixing
            }
        }
    }

    /// Cancel the capture box (the draft is untouched; the row error — which
    /// belonged to the abandoned attempt only — is cleared).
    pub fn cancel_edit(&mut self) {
        if let Some(ed) = self.editor.take()
            && ed.row < self.rows.len()
        {
            self.rows[ed.row].error = None;
        }
    }

    /// `r` on the selected row: drop its draft + error and restore the
    /// opened-at value in the draft config. The rebuild re-applies every
    /// other kept draft through the kind parser (all values already passed
    /// `set()` once), so empty pair/opt defaults never round-trip through
    /// `set` (pairs have no empty literal).
    pub fn reset_row(&mut self) {
        let Some(i) = self.selected_row_index() else {
            return;
        };
        self.rows[i].draft = None;
        self.rows[i].error = None;
        let keep: Vec<(usize, String)> = self
            .rows
            .iter()
            .enumerate()
            .filter(|(idx, r)| *idx != i && r.draft.is_some())
            .map(|(idx, r)| (idx, r.draft.clone().unwrap_or_default()))
            .collect();
        self.draft = self.base.clone();
        for (idx, value) in keep {
            let meta = self.rows[idx].meta;
            let _ = meta.field.set(&mut self.draft, &value);
        }
    }

    /// Pending diff: rows whose draft differs from their stored value.
    fn changed(&self) -> Vec<usize> {
        (0..self.rows.len())
            .filter(|&i| {
                self.rows[i]
                    .draft
                    .as_deref()
                    .is_some_and(|v| v != self.rows[i].stored)
            })
            .collect()
    }

    /// Pending-change count (modal title shows `*n` when nonzero).
    pub fn dirty_count(&self) -> usize {
        self.changed().len()
    }

    /// `s` — write each changed key to the user file via the engine
    /// (`create:false`). One write per changed key, first failure stops the
    /// loop (plan §4: no partial-write tee); every landed write marks its
    /// row back to stored + user-file origin. Returns the post-commit modal
    /// (rows mutated) alongside the outcome.
    pub fn commit_all(self) -> (Self, CommitOutcome) {
        let changed = self.changed();
        if changed.is_empty() {
            return (self, CommitOutcome::Nothing);
        }
        let Some(user_path) = self.user_path.clone() else {
            return (
                self,
                CommitOutcome::Failed {
                    written: 0,
                    error: SetError::Io(
                        "no user config file (HOME/XDG_CONFIG_HOME unset) — nothing to save into"
                            .into(),
                    ),
                },
            );
        };
        let mut modal = self;
        let mut written = 0;
        let mut warnings = Vec::new();
        let mut theme_written = 0;
        for &i in &changed {
            let Some(value) = modal.rows[i].draft.clone() else {
                continue;
            };
            let meta = modal.rows[i].meta;
            match write::set(
                meta.key,
                &value,
                &Target::User {
                    path: user_path.clone(),
                    create: false,
                },
            ) {
                Ok(out) => {
                    warnings.extend(out.warnings);
                    if section_of(meta.key) == "theme" {
                        theme_written += 1;
                    }
                    modal.rows[i].stored = value;
                    modal.rows[i].draft = None;
                    modal.rows[i].error = None;
                    modal.rows[i].origin = Origin::UserFile(user_path.clone());
                    written += 1;
                }
                Err(err) => {
                    modal.rows[i].error = Some(err.message());
                    return (
                        modal,
                        CommitOutcome::Failed {
                            written,
                            error: err,
                        },
                    );
                }
            }
        }
        (
            modal,
            CommitOutcome::Saved {
                written,
                warnings,
                theme_written,
            },
        )
    }
}

// ---------------------------------------------------------------------------
// App integration (phase 3): open guard, event handling, save/editor actions.
// ---------------------------------------------------------------------------

impl App {
    /// Open the modal from a fresh engine resolve (honors the session's
    /// `--config` replacement user-layer; plan §1.3).
    pub(crate) fn open_settings(&mut self) {
        let env = |k: &str| std::env::var(k).ok();
        let r = crate::settings::resolve(self.cli_config.as_deref(), &env);
        self.settings = Some(SettingsModal::new(r.config, r.origins, r.user_path));
    }

    /// `s` inside the modal: flush drafts through the engine + live-apply
    /// `[theme]` keys (styles/no_color rebuild; plan §4 — theme + no_color
    /// live only). Failures surface as a Message popup with the exact
    /// engine refusal; the modal stays open with its rows/drafts.
    pub(crate) fn save_settings(&mut self) {
        let Some(modal) = self.settings.take() else {
            return;
        };
        let (modal, outcome) = modal.commit_all();
        self.settings = Some(modal);
        match outcome {
            CommitOutcome::Nothing => {
                self.status = "No changes to save".into();
            }
            CommitOutcome::Saved {
                written,
                warnings,
                theme_written,
            } => {
                if theme_written > 0
                    && let Some(theme) = self.settings.as_ref().map(|m| m.draft.theme.clone())
                {
                    // Live-apply rebuilds both registry palettes (overhaul
                    // phase 1): colored from the new theme, mono re-issued
                    // fresh — `no_color` below decides which one `palette()`
                    // serves; the saved no_color is one-way.
                    self.styles = crate::tui::style::ThemeStyles::colored(&theme);
                    self.styles_mono = crate::tui::style::ThemeStyles::monochrome();
                    self.no_color = self.no_color_cli
                        || self.config.theme.no_color
                        || theme.no_color
                        || std::env::var("NO_COLOR").is_ok();
                }
                let mut msg = format!("Saved {written} change(s) to the user config");
                if theme_written < written {
                    msg.push_str("; non-theme keys apply on relaunch");
                }
                if let Some(w) = warnings.first() {
                    msg.push_str(" — ");
                    msg.push_str(w);
                }
                self.status = msg;
            }
            CommitOutcome::Failed { written, error } => {
                self.popup = Popup::Message(save_failure_message(written, &error));
            }
        }
    }

    /// `e` inside the modal: suspend the TUI, run `$EDITOR` on the user
    /// file, then reopen the modal from a fresh resolve (drafts discard —
    /// the file may have changed under them; plan §4 conflict discipline).
    pub(crate) fn edit_settings_file(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        let Some(path) = self.settings.as_ref().and_then(|m| m.user_path.clone()) else {
            self.popup = Popup::Message(
                "no user config file — set HOME/XDG_CONFIG_HOME (or pass --config)".into(),
            );
            return Ok(());
        };
        let editor = std::env::var("EDITOR")
            .or_else(|_| std::env::var("VISUAL"))
            .unwrap_or_else(|_| "vi".to_string());
        let status = suspend_and_run(terminal, move || {
            let st = std::process::Command::new(editor)
                .arg(&path)
                .status()
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            if st.success() {
                Ok(())
            } else {
                Err(anyhow::anyhow!(
                    "editor exited with {}",
                    st.code().unwrap_or(-1)
                ))
            }
        });
        match status {
            Ok(()) => {
                self.open_settings();
                self.status = "edited the user config — settings reloaded".into();
            }
            Err(e) => {
                self.popup = Popup::Message(format!("editor failed: {e}"));
            }
        }
        Ok(())
    }

    /// The modal owns the keyboard while open (runs before `handle_event`).
    /// Message popups opened by save failures keep their own precedence in
    /// the run loop (popup handler dismisses; the next key lands here).
    /// Overhaul phase 3: the rows walk the keymap table (`tui::keys`,
    /// Layer::Modal) — the typing guard is the table's fallback rows, so
    /// unclaimed keys can never escape into the search.
    pub(crate) fn handle_settings_event(
        &mut self,
        ev: &Event,
        terminal: Option<&mut DefaultTerminal>,
    ) -> Result<()> {
        let Event::Key(key) = ev else {
            return Ok(());
        };
        if key.kind != KeyEventKind::Press {
            return Ok(());
        }
        let editing = self.settings.as_ref().is_some_and(|m| m.editor.is_some());
        let filtering = self.settings.as_ref().is_some_and(|m| m.filter_focus);
        if let Some(action) = crate::tui::keys::resolve(
            crate::tui::keys::Layer::Modal,
            crate::tui::keys::Ctx::modal(editing, filtering),
            key,
        ) {
            self.execute(action, ev, terminal)?;
        }
        Ok(())
    }
}

/// Popup copy for a failed save (unit-pinned plan §4): exact engine refusal
/// + how many keys landed before the failure (none = straight refusal).
pub fn save_failure_message(written: usize, error: &SetError) -> String {
    if written == 0 {
        format!("save refused: {}", error.message())
    } else {
        format!(
            "save stopped after {written} written: {} — Esc discards the rest",
            error.message()
        )
    }
}

/// Type chars into a tui-input buffer exactly like `App::new` seeds the
/// search field (pre-fill idiom).
fn pump_input(input: &mut Input, s: &str) {
    for ch in s.chars() {
        input.handle_event(&crossterm::event::Event::Key(
            crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char(ch),
                crossterm::event::KeyModifiers::empty(),
            ),
        ));
    }
}

// ---------------------------------------------------------------------------
// Unit tests (plan §4): catalog↔row mapping, save-all one-write-per-changed-
// key, refusal/conflict mapping, enum cycling order. All offline.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// Default-config fixture: no layers, no user path (None). No disk IO.
    fn fixture() -> SettingsModal {
        SettingsModal::new(Config::default(), Vec::new(), None)
    }

    fn row_of<'a>(m: &'a SettingsModal, key: &str) -> &'a Row {
        m.rows.iter().find(|r| r.meta.key == key).unwrap()
    }

    fn select_and(m: &mut SettingsModal, key: &str) {
        assert!(m.select_key(key), "row not visible: {key}");
    }

    /// Replace the capture box contents (seeded buffer dropped) and type `s`.
    fn type_editor(m: &mut SettingsModal, s: &str) {
        if let Some(ed) = m.editor.as_mut() {
            ed.buf = Input::default();
            pump_input(&mut ed.buf, s);
        }
    }

    #[test]
    fn rows_mirror_catalog_tui_surface() {
        let mut m = fixture();
        let want: Vec<&str> = CATALOG
            .iter()
            .filter(|mt| mt.surfaces.tui)
            .map(|mt| mt.key)
            .collect();
        let got: Vec<&str> = m.rows.iter().map(|r| r.meta.key).collect();
        assert_eq!(got, want);
        // Exactly one catalog key is TUI-hidden (gui.theme), nothing else.
        assert_eq!(want.len(), CATALOG.len() - 1);
        assert!(!m.select_key("gui.theme"));
        // Untouched at open: every row keeps its resolved stored render.
        for r in &m.rows {
            assert!(r.draft.is_none() && r.error.is_none(), "{}", r.meta.key);
            assert_eq!(r.stored, r.meta.field.get(&m.base), "{}", r.meta.key);
            assert_eq!(r.origin.as_label(), "default", "{}", r.meta.key);
        }
    }

    #[test]
    fn rail_groups_follow_catalog_order_and_jump() {
        let mut m = fixture();
        let mut want: Vec<Group> = Vec::new();
        for r in &m.rows {
            if want.last() != Some(&r.meta.group) {
                want.push(r.meta.group);
            }
        }
        assert_eq!(m.groups, want);
        assert_eq!(m.rail_sel, 0);
        m.jump_group(1);
        assert_eq!(m.rail_sel, 1);
        let head = m.rows[m.visible()[m.selection]].meta.group;
        assert_eq!(head, want[1], "→ lands on the group's first row");
        m.jump_group(-1);
        assert_eq!(m.rail_sel, 0);
        m.jump_group(-1); // wraps: first → last
        assert_eq!(m.rail_sel, want.len() - 1);
    }

    #[test]
    fn filter_narrows_and_clamps_selection() {
        let mut m = fixture();
        m.selection = 5;
        pump_input(&mut m.filter, "debounce_ms");
        m.sync_after_filter();
        let got: Vec<&str> = m.visible().iter().map(|&i| m.rows[i].meta.key).collect();
        assert_eq!(got, vec!["tui.debounce_ms"]);
        assert!(m.selection < got.len().max(1));
        // clearing restores the full list
        m.filter = Input::default();
        m.sync_after_filter();
        assert_eq!(m.visible().len(), m.rows.len());
        // unfiltered rail = run-length distinct groups in CATALOG order
        let mut distinct: Vec<Group> = Vec::new();
        for i in m.visible() {
            if distinct.last() != Some(&m.rows[i].meta.group) {
                distinct.push(m.rows[i].meta.group);
            }
        }
        assert_eq!(m.groups, distinct);
    }

    #[test]
    fn space_toggles_bools_like_the_file_would() {
        let mut m = fixture();
        select_and(&mut m, "search.regex");
        m.bool_toggle();
        {
            let r = row_of(&m, "search.regex");
            assert_eq!(r.draft.as_deref(), Some("true"));
            assert!(r.error.is_none());
        }
        assert!(m.draft.search.regex);
        assert_eq!(m.dirty_count(), 1);
        // back to the stored default: diff collapses to nothing
        m.bool_toggle();
        assert_eq!(row_of(&m, "search.regex").draft.as_deref(), Some("false"));
        assert_eq!(m.dirty_count(), 0);
        // non-bool rows ignore Space (enum/numeric use Enter)
        select_and(&mut m, "search.limit");
        m.bool_toggle();
        assert!(row_of(&m, "search.limit").draft.is_none());
    }

    #[test]
    fn enter_cycles_enums_in_catalog_order_and_wraps() {
        let mut m = fixture();
        select_and(&mut m, "search.source");
        let values: Vec<&str> = crate::settings::catalog::SEARCH_SOURCE_VALUES
            .iter()
            .map(|v| v.name)
            .collect();
        assert_eq!(m.draft.search.source, values[0]); // "all" is the default
        m.enum_cycle();
        assert_eq!(
            row_of(&m, "search.source").draft.as_deref(),
            Some(values[1])
        );
        for _ in 1..values.len() - 1 {
            m.enum_cycle();
        }
        assert_eq!(m.draft.search.source, values[values.len() - 1]);
        m.enum_cycle(); // wraps back to the CATALOG head
        assert_eq!(m.draft.search.source, values[0]);
        assert_eq!(
            row_of(&m, "search.source").draft.as_deref(),
            Some(values[0])
        );
    }

    #[test]
    fn editor_commits_valid_values_and_flags_invalid() {
        let mut m = fixture();
        select_and(&mut m, "search.timeout_secs");
        m.enter_selected(); // numeric → capture box
        assert!(m.editor.is_some());
        type_editor(&mut m, "1");
        m.commit_edit();
        assert_eq!(
            row_of(&m, "search.timeout_secs").error.as_deref(),
            Some("expected 3..=30") // engine clamp single-source copy
        );
        assert!(m.editor.is_some(), "stays open for fixing");
        type_editor(&mut m, "12");
        m.commit_edit();
        {
            let r = row_of(&m, "search.timeout_secs");
            assert!(r.error.is_none());
            assert_eq!(r.draft.as_deref(), Some("12"));
        }
        assert_eq!(m.draft.search.timeout_secs, 12);
        assert!(m.editor.is_none());
        // Esc on an untouched edit: draft stays None, error stays absent
        select_and(&mut m, "search.limit");
        m.enter_selected();
        assert!(row_of(&m, "search.limit").draft.is_none());
        m.cancel_edit();
        assert!(m.editor.is_none());
        assert!(row_of(&m, "search.limit").draft.is_none());
        assert!(row_of(&m, "search.limit").error.is_none());
    }

    #[test]
    fn save_all_writes_once_per_changed_key() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[search]\nlimit = 4\n").unwrap();
        let mut m = SettingsModal::new(Config::default(), Vec::new(), Some(path.clone()));
        select_and(&mut m, "search.regex");
        m.bool_toggle(); // false → true
        select_and(&mut m, "tui.debounce_ms");
        m.enter_selected();
        type_editor(&mut m, "800");
        m.commit_edit();
        // Retyping the stored value is NOT a change (diff is value-based).
        select_and(&mut m, "search.limit");
        m.enter_selected();
        type_editor(&mut m, "50");
        m.commit_edit();
        assert_eq!(m.dirty_count(), 2);

        let (m, out) = m.commit_all();
        let CommitOutcome::Saved {
            written,
            theme_written,
            warnings,
        } = out
        else {
            panic!("expected Saved, got {out:?}");
        };
        assert_eq!(written, 2);
        assert_eq!(theme_written, 0);
        assert!(warnings.is_empty());
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("regex = true"));
        assert_eq!(text.matches("debounce_ms = 800").count(), 1);
        // the untouched row keeps the file byte-for-byte
        assert!(text.contains("limit = 4"));
        assert_eq!(text.matches("limit = 4").count(), 1);
        // post-commit rows: stored refreshed, draft dropped, origin flips
        let r = row_of(&m, "search.regex");
        assert_eq!(r.stored, "true");
        assert!(r.draft.is_none());
        assert_eq!(r.origin.as_label(), "user");
        // limit was never written: still default origin
        assert_eq!(row_of(&m, "search.limit").origin.as_label(), "default");
        assert_eq!(m.dirty_count(), 0);
    }

    #[test]
    fn save_refusal_surfaces_engine_taxonomy() {
        let dir = tempdir().unwrap();
        // A directory is not a regular file — the engine's owner gate refuses.
        let path = dir.path().join("adir");
        std::fs::create_dir(&path).unwrap();
        let mut m = SettingsModal::new(Config::default(), Vec::new(), Some(path));
        select_and(&mut m, "search.regex");
        m.bool_toggle();
        let (m, out) = m.commit_all();
        let CommitOutcome::Failed { written, error } = out else {
            panic!("expected Failed");
        };
        assert_eq!(written, 0);
        assert_eq!(error.code(), "policy-refused");
        assert!(
            row_of(&m, "search.regex").error.is_some(),
            "row keeps the refusal"
        );
        // Pathless save: same envelope via the Io branch.
        let mut m2 = fixture();
        select_and(&mut m2, "search.regex");
        m2.bool_toggle();
        let (_, out) = m2.commit_all();
        match out {
            CommitOutcome::Failed { written, error } => {
                assert_eq!(written, 0);
                assert_eq!(error.code(), "io");
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[test]
    fn failure_message_maps_the_conflict_copy() {
        let err = SetError::Conflict(
            "/tmp/x changed on disk while it was read (editor or another pakmap?) — \
             re-check `pakmap config get search.limit` and retry"
                .into(),
        );
        let msg = save_failure_message(0, &err);
        assert!(msg.starts_with("save refused: "));
        assert!(msg.contains("changed on disk"));
        let msg = save_failure_message(3, &err);
        assert!(msg.starts_with("save stopped after 3 written: "));
        assert!(msg.contains("changed on disk"));
    }

    #[test]
    fn reset_restores_the_opened_at_value() {
        let mut m = fixture();
        select_and(&mut m, "search.source");
        m.enum_cycle(); // all → next
        select_and(&mut m, "search.regex");
        m.bool_toggle(); // false → true
        assert_eq!(m.dirty_count(), 2);
        select_and(&mut m, "search.source");
        m.reset_row();
        {
            let r = row_of(&m, "search.source");
            assert!(r.draft.is_none());
            assert!(r.error.is_none());
        }
        assert_eq!(m.draft.search.source, "all");
        // the bool draft survives the rebuild
        assert_eq!(row_of(&m, "search.regex").draft.as_deref(), Some("true"));
        assert!(m.draft.search.regex);
        assert_eq!(m.dirty_count(), 1);
    }

    #[test]
    fn rendered_stored_values_roundtrip_through_set() {
        let m = fixture();
        for r in &m.rows {
            if r.meta.kind == Kind::PairU16 && r.stored.is_empty() {
                // Engine `None` pairs have no empty literal — they are never
                // round-tripped through set() (reset rebuilds from `base`).
                continue;
            }
            let mut c = Config::default();
            r.meta
                .field
                .set(&mut c, &r.stored)
                .unwrap_or_else(|e| panic!("{}: set({:?}): {e}", r.meta.key, r.stored));
            assert_eq!(r.meta.field.get(&c), r.stored, "{} roundtrip", r.meta.key);
        }
    }

    #[test]
    fn open_settings_from_app_builds_the_modal() {
        let mut app = App::new(String::new());
        assert!(app.settings.is_none());
        app.open_settings();
        let modal = app.settings.as_ref().unwrap();
        assert_eq!(modal.rows.len(), CATALOG.len() - 1);
        assert_eq!(modal.selection, 0);
        assert!(!modal.filter_focus);
        // the session's --config replacement reaches the resolve (None here)
        assert!(app.cli_config.is_none());
    }
}
