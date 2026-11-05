use color_eyre::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::{
    DefaultTerminal,
    widgets::{BorderType, ListState},
};
use std::collections::{HashMap, VecDeque};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{self},
};
use std::thread;
use std::time::{Duration, Instant};
use tui_input::{Input, backend::crossterm::EventHandler};

use crate::cli::{AurBy, Source};
use crate::config::Config;
use crate::model::Package;
use crate::search::aur::search_aur_blocking_with_config;
use crate::tui::style::{ThemeStyles, border_type_from_str};

use super::ui;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Search,
    List,
}

/// TUI mode — Tab cycles installing / removing / updating.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Search,
    Installed,
    Updates,
}

impl Mode {
    pub fn toggle(self) -> Self {
        // Tab cycles all three (Search -> Installed -> Updates -> Search).
        match self {
            Mode::Search => Mode::Installed,
            Mode::Installed => Mode::Updates,
            Mode::Updates => Mode::Search,
        }
    }
    /// BackTab reverses the cycle (Updates -> Installed -> Search).
    pub fn toggle_back(self) -> Self {
        match self {
            Mode::Search => Mode::Updates,
            Mode::Installed => Mode::Search,
            Mode::Updates => Mode::Installed,
        }
    }
    pub fn tab_index(self) -> usize {
        match self {
            Mode::Search => 0,
            Mode::Installed => 1,
            Mode::Updates => 2,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Popup {
    None,
    Info(Package),
    Confirm(Package),
    ConfirmRemove(Package),
    ConfirmRefresh,
    ConfirmUpgrade,
    ConfirmClean {
        files: usize,
        bytes: u64,
        flatpak: bool,
    },
    Help,
    Message(String),
}

/// Which backend a staged result block came from (P4 perf: each search leg
/// paints as soon as its join completes instead of holding the UI for the
/// slowest backend). `Done` finalizes: loading off, cache fill, final status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SearchStage {
    Repo,
    Aur,
    Flatpak,
    Done,
}

/// One search-result message. Staged legs carry their block (`Repo`/`Aur`/
/// `Flatpak`, in fixed platform order matching the old combined list); the
/// search orchestrator finishes with an empty-payload `Done`. The installed/
/// updates workers keep their single-message flow and send their whole list
/// inside `Done` — semantics unchanged there.
struct SearchMsg {
    search_id: u64,
    stage: SearchStage,
    query: String,
    packages: Vec<Package>,
}

pub struct App {
    pub input: Input,
    pub packages: Vec<Package>,
    pub list_state: ListState,
    pub focus: Focus,
    pub mode: Mode,
    pub popup: Popup,
    /// v0.10.0 phase 3: the settings modal (`S`) — `None` when closed.
    pub settings: Option<super::settings::SettingsModal>,
    /// Session's `--config` replacement user-layer (plan §1.3); the settings
    /// modal resolves rows and writes through it. `None` = standard XDG.
    pub cli_config: Option<std::path::PathBuf>,
    /// `--no-color` from the CLI flag — live-apply of `theme.no_color` must
    /// never erase an explicit session flag.
    pub no_color_cli: bool,
    pub is_loading: bool,
    pub status: String,
    pub initial_query: String,
    pub last_query: String,
    /// Last query per mode — switching tabs restores each mode's results without refetch.
    pub last_query_search: String,
    pub last_query_installed: String,
    pub last_query_updates: String,
    /// 0.5.0: readonly browse mode (no mutating ops).
    pub readonly: bool,
    /// 0.5.0: Installed-tab orphan filter (`o` toggles `pacman -Qdt` view).
    pub orphans_only: bool,
    /// 0.5.0: raw updates for upgrade-all (display mirrors `packages`).
    pub updates: Vec<crate::updates::PkgUpdate>,
    /// 5.1: news-intervention warning shown inline in the upgrade popup.
    /// Set when `U` opens ConfirmUpgrade; cleared on dismiss/confirm.
    pub upgrade_warn: Option<String>,
    pub limit: usize,
    pub source: Source,
    pub aur_by: AurBy,
    pub use_regex: bool,
    pub installed_only: bool,
    pub no_color: bool,
    pub should_quit: bool,
    pub needs_search: bool,
    pub last_input_change: Instant,
    pub config: Config,
    pub styles: ThemeStyles,
    /// The NO_COLOR-aware registry pair (overhaul phase 1): drawing code
    /// picks a palette through `palette()`, never mixes the two or builds
    /// ad-hoc styles.
    pub styles_mono: ThemeStyles,
    /// Cached border type — parsed once from config instead of per-widget per-frame.
    pub border_type: BorderType,
    // Phase B: non-blocking search channel + dedup id per ratatui async skill
    search_rx: Option<std::sync::mpsc::Receiver<SearchMsg>>,
    search_id: u64,
    next_search_id: u64,
    /// Generation guard shared with worker threads: a trigger stores its id
    /// here; workers skip `tx.send` when a newer search superseded them, so
    /// stale threads never wake the UI with dead results.
    active_search: Arc<std::sync::atomic::AtomicU64>,
    /// P4 staged search: per-search accumulation buffer. First stage arrival
    /// for a generation replaces it (superseded leftovers never mix), later
    /// legs append in platform order; `Done` swaps it into `packages` at once.
    staged_packages: Vec<Package>,
    staged_search_id: Option<u64>,
    /// Small FIFO result cache (std-only) — retyping / tab-switching a recent
    /// query serves instantly with zero disk/network. Cleared on install/remove.
    search_cache: HashMap<String, Vec<Package>>,
    search_cache_order: VecDeque<String>,
    /// Redraw only when state changed (or while loading) instead of every poll tick.
    dirty: bool,
    /// Last full draw — while a search is in flight without state changes,
    /// redraws are throttled to ~2fps (spinner still animates, list rebuilds stop).
    last_draw: Instant,
}

/// Max cached search results (each entry ≤ limit packages).
const SEARCH_CACHE_CAP: usize = 32;

impl App {
    pub fn new(initial_query: String) -> Self {
        let mut input = Input::default();
        if !initial_query.is_empty() {
            // pre-fill input
            for ch in initial_query.chars() {
                input.handle_event(&Event::Key(crossterm::event::KeyEvent::new(
                    KeyCode::Char(ch),
                    KeyModifiers::empty(),
                )));
            }
        }
        let mut list_state = ListState::default();
        list_state.select(None);
        let config = Config::default();
        let styles = ThemeStyles::colored(&config.theme);
        let styles_mono = ThemeStyles::monochrome();
        let border_type = border_type_from_str(&config.tui.border);
        Self {
            input,
            packages: Vec::new(),
            list_state,
            focus: if initial_query.is_empty() {
                Focus::Search
            } else {
                Focus::List
            },
            mode: Mode::Search,
            popup: Popup::None,
            settings: None,
            cli_config: None,
            no_color_cli: false,
            is_loading: false,
            status: "Type to search, Enter search, F5 refresh, Tab remover, ? help".into(),
            initial_query: initial_query.clone(),
            last_query: String::new(),
            last_query_search: String::new(),
            last_query_installed: String::new(),
            last_query_updates: String::new(),
            readonly: false,
            orphans_only: false,
            updates: Vec::new(),
            upgrade_warn: None,
            limit: 50,
            source: Source::All,
            aur_by: AurBy::NameDesc,
            use_regex: false,
            installed_only: false,
            no_color: std::env::var("NO_COLOR").is_ok(),
            should_quit: false,
            needs_search: !initial_query.is_empty(),
            last_input_change: Instant::now(),
            config,
            styles,
            styles_mono,
            border_type,
            search_rx: None,
            search_id: 0,
            next_search_id: 0,
            active_search: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            staged_packages: Vec::new(),
            staged_search_id: None,
            search_cache: HashMap::new(),
            search_cache_order: VecDeque::new(),
            dirty: true,
            last_draw: Instant::now(),
        }
    }

    pub fn new_with_cli(initial_query: String, cli: &crate::cli::Cli) -> Self {
        let mut app = Self::new(initial_query);
        app.limit = cli.limit;
        app.source = cli.source;
        app.aur_by = cli.by;
        app.use_regex = cli.regex;
        app.installed_only = cli.installed_only;
        app.no_color = cli.no_color || std::env::var("NO_COLOR").is_ok();
        app.readonly = cli.readonly;
        app.cli_config = cli.config.clone();
        app.no_color_cli = cli.no_color;
        app
    }

    pub fn new_with_config(initial_query: String, cli: &crate::cli::Cli, cfg: &Config) -> Self {
        let mut app = Self::new(initial_query);
        // CLI already merged with cfg in main.rs, but keep cfg for theme/layout
        app.limit = cli.limit;
        app.source = cli.source;
        app.aur_by = cli.by;
        app.use_regex = cli.regex;
        app.installed_only = cli.installed_only;
        app.no_color = cli.no_color || cfg.theme.no_color || std::env::var("NO_COLOR").is_ok();
        app.readonly = cli.readonly || cfg.is_readonly();
        app.config = cfg.clone();
        app.styles = ThemeStyles::colored(&cfg.theme);
        app.styles_mono = ThemeStyles::monochrome();
        app.border_type = border_type_from_str(&cfg.tui.border);
        app.cli_config = cli.config.clone();
        app.no_color_cli = cli.no_color;
        app
    }

    /// The palette this frame draws with (`no_color` → the mono pair,
    /// otherwise the colored catalog palette). The single NO_COLOR selector:
    /// draw code calls this once per function and owns no ternaries
    /// (overhaul phase 1, plan D1/D2).
    pub fn palette(&self) -> &ThemeStyles {
        if self.no_color {
            &self.styles_mono
        } else {
            &self.styles
        }
    }

    fn cache_key(&self, query: &str) -> String {
        format!(
            "{:?}|{}|{}|{:?}|{:?}|{}|{}|{}",
            self.mode,
            query,
            self.limit,
            self.source,
            self.aur_by,
            self.use_regex,
            self.installed_only,
            self.orphans_only
        )
    }

    fn cache_get(&self, key: &str) -> Option<Vec<Package>> {
        self.search_cache.get(key).cloned()
    }

    fn cache_put(&mut self, key: String, pkgs: Vec<Package>) {
        if !self.search_cache.contains_key(&key) {
            while self.search_cache_order.len() >= SEARCH_CACHE_CAP {
                match self.search_cache_order.pop_front() {
                    Some(old) => {
                        self.search_cache.remove(&old);
                    }
                    None => break,
                }
            }
            self.search_cache_order.push_back(key.clone());
        }
        self.search_cache.insert(key, pkgs);
    }

    fn clear_cache(&mut self) {
        self.search_cache.clear();
        self.search_cache_order.clear();
        self.staged_packages.clear();
        self.staged_search_id = None;
    }

    /// Apply freshly fetched results: selection, status text, per-mode memory.
    /// v0.9.3 Phase 3 (CWE-150): display-boundary sanitization — AUR/RPC
    /// strings only render after passing `sanitize_terminal`, at the single
    /// choke point every mode's results flow through (`--json` output is a
    /// different, non-terminal path and stays raw). Real names/descriptions
    /// are byte-identical through the filter, and operations that read these
    /// packages re-validate names (`safety::names`), so no data can drift.
    fn apply_results(&mut self, mut pkgs: Vec<Package>, query: String) {
        for p in pkgs.iter_mut() {
            p.name = crate::safety::text::sanitize_terminal(&p.name);
            p.version = crate::safety::text::sanitize_terminal(&p.version);
            if let Some(d) = p.description.take() {
                p.description = Some(crate::safety::text::sanitize_terminal(&d));
            }
        }
        let total = pkgs.len();
        self.packages = pkgs;
        self.list_state.select(if self.packages.is_empty() {
            None
        } else {
            Some(0)
        });
        self.is_loading = false;
        self.last_query = query.clone();
        if self.mode == Mode::Installed {
            // Remember per-mode query for Tab restore
            self.last_query_installed = query.clone();
            if total == 0 {
                self.status = if query.is_empty() {
                    if self.orphans_only {
                        "No orphans — system clean".into()
                    } else {
                        "No installed packages".into()
                    }
                } else {
                    format!("No installed match for '{}'", query)
                };
            } else if self.orphans_only {
                self.status = format!("Orphans {} (unrequired deps)", total);
            } else {
                self.status = format!("Installed {} (local {})", total, total);
            }
        } else if self.mode == Mode::Updates {
            self.last_query_updates = query.clone();
            let aur_count = self.packages.iter().filter(|p| p.repo == "aur").count();
            let flatpak_count = self.packages.iter().filter(|p| p.repo == "flatpak").count();
            let repo_count = total
                .saturating_sub(aur_count)
                .saturating_sub(flatpak_count);
            if total == 0 {
                self.status = "System up to date".into();
            } else {
                self.status = format!(
                    "Updates {} (repo {} aur {} flatpak {}) — U:upgrade all",
                    total, repo_count, aur_count, flatpak_count
                );
            }
        } else {
            self.last_query_search = query.clone();
            let aur_count = self.packages.iter().filter(|p| p.repo == "aur").count();
            let flatpak_count = self.packages.iter().filter(|p| p.repo == "flatpak").count();
            let repo_count = total
                .saturating_sub(aur_count)
                .saturating_sub(flatpak_count);
            if total == 0 {
                self.status = format!("No results for '{}'", query);
            } else {
                self.status = format!(
                    "Found {} (repo {} aur {} flatpak {})",
                    total, repo_count, aur_count, flatpak_count
                );
            }
        }
        self.dirty = true;
    }

    /// Switch Search <-> Installed <-> Updates (Tab). Clears list, restores per-mode hint,
    /// and triggers a fresh search (Installed/Updates with empty filter list all).
    pub fn switch_mode(&mut self) {
        self.switch_mode_to(self.mode.toggle());
    }

    /// BackTab: same as Tab but reversed (Updates -> Installed -> Search).
    pub fn switch_mode_back(&mut self) {
        self.switch_mode_to(self.mode.toggle_back());
    }

    fn switch_mode_to(&mut self, next: Mode) {
        // Stash current query per outgoing mode.
        match self.mode {
            Mode::Search => self.last_query_search = self.last_query.clone(),
            Mode::Installed => self.last_query_installed = self.last_query.clone(),
            Mode::Updates => self.last_query_updates = self.last_query.clone(),
        }
        self.mode = next;
        // Restore incoming mode's query.
        self.last_query = match self.mode {
            Mode::Search => self.last_query_search.clone(),
            Mode::Installed => self.last_query_installed.clone(),
            Mode::Updates => self.last_query_updates.clone(),
        };
        self.packages.clear();
        self.staged_packages.clear();
        self.staged_search_id = None;
        self.list_state.select(None);
        self.popup = Popup::None;
        self.search_rx = None;
        let ro = if self.readonly { " [RO]" } else { "" };
        self.status = match self.mode {
            Mode::Search => {
                format!("Type to search, Enter search, F5 refresh, Tab remover, ? help{ro}")
            }
            Mode::Installed => {
                format!(
                    "Installed mode — type to filter, Enter remove, o orphans, F5 refresh, Tab updates, ? help{ro}"
                )
            }
            Mode::Updates => {
                format!(
                    "Updates — Enter upgrade one, U upgrade all, F5 refresh, Tab search, ? help{ro}"
                )
            }
        };
        // Installed/Updates with empty filter list all; Search with empty clears.
        let should_fetch =
            !matches!(self.mode, Mode::Search) || !self.input.value().trim().is_empty();
        if should_fetch {
            self.is_loading = true;
            self.trigger_search();
        } else {
            self.is_loading = false;
        }
        self.needs_search = false;
        self.dirty = true;
    }

    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        // Best-effort signal handling — per tui-design lifecycle, ensure raw mode restored on SIGTERM/SIGINT
        let term_flag = Arc::new(AtomicBool::new(false));
        #[cfg(unix)]
        {
            let flag = term_flag.clone();
            let _ = signal_hook::flag::register(signal_hook::consts::SIGTERM, flag.clone());
            let _ = signal_hook::flag::register(signal_hook::consts::SIGINT, flag);
            let _ = signal_hook::flag::register(signal_hook::consts::SIGHUP, term_flag.clone());
        }

        // Orphan watchdog (see stdio_dead): if the terminal vanishes while we
        // run (closed emulator, reparented to init), crossterm's poll spins
        // inside its fd-read loop on EIO and never returns — no timeout, and
        // its EINTR paths `continue`, so signal flags are never observed
        // either (a spinning orphan even survives SIGTERM). A parked watchdog
        // notices the dead stdio instead and exits quietly.
        thread::spawn(|| {
            loop {
                thread::sleep(Duration::from_secs(5));
                if stdio_dead() {
                    std::process::exit(0);
                }
            }
        });

        // If initial query provided, do first search immediately (no debounce) — now non-blocking
        if self.needs_search {
            self.trigger_search();
            self.needs_search = false;
        }

        while !self.should_quit {
            if term_flag.load(Ordering::Relaxed) {
                self.should_quit = true;
                break;
            }

            // Poll for background search results before draw so spinner updates immediately
            self.poll_search_results();

            // Redraw only on state change (or while a search is in flight),
            // instead of every poll tick — idle frames cost zero ListItem rebuilds.
            // While loading with no state change, throttle to ~2fps: the status
            // spinner still animates, but full list rebuilds stop hammering CPU
            // on slow networks.
            if self.dirty || self.is_loading {
                let throttled =
                    !self.dirty && self.last_draw.elapsed() < Duration::from_millis(500);
                if !throttled {
                    terminal.draw(|f| ui::draw(f, self))?;
                    self.last_draw = Instant::now();
                    self.dirty = false;
                }
            }

            // poll with timeout to allow debounce & spinner & signal check.
            // Upper clamp: a huge poll_ms would freeze input handling.
            let poll_ms = self.config.tui.poll_ms.clamp(50, 1000);
            if event::poll(Duration::from_millis(poll_ms))? {
                let ev = event::read()?;
                if self.handle_popup_event(&ev, Some(&mut *terminal))? {
                    self.dirty = true;
                    continue;
                }
                // v0.10.0 phase 3: the settings modal owns the keyboard while
                // open — Message popups it raises keep popup precedence above.
                if self.settings.is_some() {
                    self.handle_settings_event(&ev, Some(&mut *terminal))?;
                    self.dirty = true;
                    continue;
                }
                self.handle_event(&ev, Some(&mut *terminal))?;
                self.dirty = true;
            }

            // Also poll after handling event for immediate fetch after Enter
            self.poll_search_results();

            // Debounced search: if input changed and debounce passed without new key.
            // Upper clamp: a huge debounce_ms would make Enter feel dead.
            let debounce = Duration::from_millis(self.config.tui.debounce_ms.clamp(50, 5000));
            if self.focus == Focus::Search
                && self.needs_search
                && self.last_input_change.elapsed() > debounce
            {
                self.trigger_search();
                self.needs_search = false;
            }
        }
        Ok(())
    }

    fn poll_search_results(&mut self) {
        // Drain pending results first so method calls below don't fight the
        // channel borrow (only the latest search_id is accepted).
        let pending: Vec<SearchMsg> = match &self.search_rx {
            Some(rx) => {
                let mut v = Vec::new();
                while let Ok(msg) = rx.try_recv() {
                    v.push(msg);
                }
                v
            }
            None => return,
        };
        for msg in pending {
            // Only accept latest search_id; discard stale generations whole.
            if msg.search_id != self.search_id {
                tracing::debug!(
                    id = msg.search_id,
                    search_id = self.search_id,
                    "discard stale search result"
                );
                continue;
            }
            let key = self.cache_key(&msg.query);
            match msg.stage {
                SearchStage::Done => {
                    // Search orchestrator finishes with an empty payload (the
                    // legs were staged into `staged_packages`); installed/
                    // updates workers carry their whole list in `Done`.
                    let pkgs = if msg.packages.is_empty() {
                        std::mem::take(&mut self.staged_packages)
                    } else {
                        msg.packages
                    };
                    self.apply_results(pkgs.clone(), msg.query);
                    self.cache_put(key, pkgs);
                }
                SearchStage::Repo | SearchStage::Aur | SearchStage::Flatpak => {
                    // Staged leg: leave the visible list (and cursor) showing
                    // the last complete generation while the new generation
                    // accumulates — first arrival replaces the scratch buffer,
                    // later legs append. Count tick in the interim status.
                    if self.staged_search_id == Some(msg.search_id) {
                        self.staged_packages.extend(msg.packages);
                    } else {
                        self.staged_packages = msg.packages;
                        self.staged_search_id = Some(msg.search_id);
                    }
                    self.status = format!(
                        "Searching for '{}' — {} found so far...",
                        msg.query,
                        self.staged_packages.len()
                    );
                    self.dirty = true;
                }
            }
        }
    }

    /// Popup layer: the run loop's first consumer (D2 precedence frozen).
    /// The rows live in the keymap table; the bodies are the exact v0.10
    /// arms, moved verbatim into [`App::execute`].
    fn handle_popup_event(
        &mut self,
        ev: &Event,
        terminal: Option<&mut DefaultTerminal>,
    ) -> Result<bool> {
        if self.popup == Popup::None {
            return Ok(false);
        }
        if let Event::Key(key) = ev {
            if key.kind != KeyEventKind::Press {
                return Ok(true);
            }
            if let Some(action) = crate::tui::keys::resolve(
                crate::tui::keys::Layer::Popup,
                crate::tui::keys::Ctx::popup(crate::tui::keys::PopupClass::of(&self.popup)),
                key,
            ) {
                self.execute(action, ev, terminal)?;
            }
        }
        // Every popup-layer key is consumed: the modal and frame layers
        // never see an event while a popup is open.
        Ok(true)
    }

    fn handle_event(&mut self, ev: &Event, terminal: Option<&mut DefaultTerminal>) -> Result<()> {
        match ev {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                // Frame layer: reached only when no popup and no modal is
                // open (run-loop precedence) — the table's resolve replaces
                // the old arm walk, first-match-wins in the same order.
                if let Some(action) = crate::tui::keys::resolve(
                    crate::tui::keys::Layer::Frame,
                    crate::tui::keys::Ctx::frame(self.mode, self.focus),
                    key,
                ) {
                    self.execute(action, ev, terminal)?;
                }
            }
            Event::Resize(_, _) => {
                // ratatui will re-layout next draw, nothing extra
            }
            _ => {}
        }
        Ok(())
    }

    /// The v0.10 arm bodies, moved verbatim behind the keymap table's
    /// action ids (plan §3: dispatch *walks* are `tui::keys::resolve`;
    /// execution stays here so every behavioral body remains reviewable
    /// in one match — zero behavior change).
    ///
    /// `terminal` is `Some` in production (the run loop's terminal). The
    /// applied-matrix tests pass `None` and drive only the state-only arms;
    /// the subprocess arms (`ConfirmActive`, `ModalEditFile`, and anything
    /// suspending the TUI) short-circuit on `None` — those rows are
    /// resolved-only in the tests, exactly as the plan's dispatch-matrix
    /// acceptance prescribes.
    pub(crate) fn execute(
        &mut self,
        action: crate::tui::keys::Action,
        ev: &Event,
        terminal: Option<&mut DefaultTerminal>,
    ) -> Result<()> {
        use crate::tui::keys::Action;
        match action {
            // --- popup layer ---
            Action::ClosePopupDismiss => {
                self.popup = Popup::None;
                self.upgrade_warn = None;
            }
            Action::ToggleHelpPopup => {
                // Toggle help — only the Help popup toggles; other popups
                // swallow `?` (the row guard holds the exact old condition).
                if self.popup == Popup::Help {
                    self.popup = Popup::None;
                }
            }
            Action::ConfirmActive => {
                let Some(terminal) = terminal else {
                    return Ok(()); // resolver-only in the applied-matrix tests
                };
                // Confirm install / remove / refresh / upgrade / clean
                match self.popup.clone() {
                    Popup::Confirm(pkg) => {
                        self.popup = Popup::None;
                        self.do_install(pkg, terminal)?;
                    }
                    Popup::ConfirmRemove(pkg) => {
                        self.popup = Popup::None;
                        self.do_remove(pkg, terminal)?;
                    }
                    Popup::ConfirmRefresh => {
                        self.popup = Popup::None;
                        self.do_refresh(terminal)?;
                    }
                    Popup::ConfirmUpgrade => {
                        self.popup = Popup::None;
                        self.upgrade_warn = None;
                        self.do_upgrade(terminal)?;
                    }
                    Popup::ConfirmClean {
                        files,
                        bytes,
                        flatpak,
                    } => {
                        self.popup = Popup::None;
                        self.do_clean(files, bytes, flatpak, terminal)?;
                    }
                    _ => {} // unreachable: the row guard is PopupConfirm
                }
            }
            Action::ClosePopupSimple => {
                self.popup = Popup::None;
            }
            // --- modal layer ---
            Action::ModalCancelEditor => {
                if let Some(m) = self.settings.as_mut() {
                    m.cancel_edit();
                }
            }
            Action::ModalClearFilter => {
                if let Some(m) = self.settings.as_mut() {
                    m.filter = Input::default();
                    m.filter_focus = false;
                    m.sync_after_filter();
                }
            }
            Action::ModalClose => {
                self.settings = None; // Esc discards drafts (plan §4); S toggles closed
            }
            Action::ModalMovePrev => {
                if let Some(m) = self.settings.as_mut() {
                    m.move_sel(-1);
                }
            }
            Action::ModalMoveNext => {
                if let Some(m) = self.settings.as_mut() {
                    m.move_sel(1);
                }
            }
            Action::ModalPrevGroup => {
                if let Some(m) = self.settings.as_mut() {
                    m.jump_group(-1);
                }
            }
            Action::ModalNextGroup => {
                if let Some(m) = self.settings.as_mut() {
                    m.jump_group(1);
                }
            }
            Action::ModalFilterFocus => {
                if let Some(m) = self.settings.as_mut() {
                    m.filter_focus = true;
                }
            }
            Action::ModalCommitEditor => {
                if let Some(m) = self.settings.as_mut() {
                    m.commit_edit();
                }
            }
            Action::ModalLeaveFilter => {
                if let Some(m) = self.settings.as_mut() {
                    m.filter_focus = false; // keep the filter, leave the box
                }
            }
            Action::ModalEnterRow => {
                if let Some(m) = self.settings.as_mut() {
                    m.enter_selected();
                }
            }
            Action::ModalToggleBool => {
                if let Some(m) = self.settings.as_mut() {
                    m.bool_toggle();
                }
            }
            Action::ModalResetRow => {
                if let Some(m) = self.settings.as_mut() {
                    m.reset_row();
                }
            }
            Action::ModalSave => {
                self.save_settings();
            }
            Action::ModalEditFile => {
                let Some(terminal) = terminal else {
                    return Ok(()); // resolver-only in the applied-matrix tests
                };
                self.edit_settings_file(terminal)?;
            }
            Action::ModalTypeEditor => {
                // The typing guard: while the value editor is open every
                // unclaimed key types into the box (never escapes to search).
                if let Some(m) = self.settings.as_mut()
                    && let Some(ed) = m.editor.as_mut()
                {
                    ed.buf.handle_event(ev);
                }
            }
            Action::ModalTypeFilter => {
                if let Some(m) = self.settings.as_mut() {
                    m.filter.handle_event(ev);
                    m.sync_after_filter();
                }
            }
            // --- frame layer ---
            Action::CycleModeNext => {
                self.switch_mode();
            }
            Action::CycleModePrev => {
                self.switch_mode_back();
            }
            Action::OpenPopupHelp => {
                self.popup = Popup::Help;
            }
            Action::OpenSettings => {
                self.open_settings();
            }
            Action::ConfirmRefresh => {
                self.popup = Popup::ConfirmRefresh;
            }
            Action::Quit => {
                // `q` only from list focus (row guard); Ctrl+C anywhere.
                self.should_quit = true;
            }
            Action::ToggleToList => {
                self.focus = Focus::List;
            }
            Action::ToggleToSearch => {
                self.focus = Focus::Search;
            }
            Action::JumpToSearch => {
                self.focus = Focus::Search;
            }
            Action::SubmitSearch => {
                // Trigger search and move to list
                self.do_search()?;
                self.focus = Focus::List;
            }
            Action::ConfirmSelected => {
                // Install / remove / upgrade-one depending on mode
                if let Some(idx) = self.list_state.selected()
                    && let Some(pkg) = self.packages.get(idx).cloned()
                {
                    self.popup = match self.mode {
                        Mode::Search => Popup::Confirm(pkg),
                        Mode::Installed => Popup::ConfirmRemove(pkg),
                        Mode::Updates => Popup::Confirm(pkg),
                    };
                }
            }
            Action::UpgradeAll => {
                // 5.1: U upgrades all, scoped to Updates tab (in Search/Installed
                // list it types/filters instead of surprising with an upgrade prompt).
                // News-intervention warning is fetched once here and rendered
                // inline in the popup — single step, no second-press dance.
                let news = crate::news::fetch_arch_news(&self.config);
                self.upgrade_warn = if crate::news::has_intervention(&news, true) {
                    Some(
                        news.iter()
                            .filter(|i| i.needs_action)
                            .take(2)
                            .map(|i| i.title.clone())
                            .collect::<Vec<_>>()
                            .join("; "),
                    )
                } else {
                    None
                };
                self.popup = Popup::ConfirmUpgrade;
            }
            Action::ToggleOrphans => {
                // 0.5.0: o toggles orphans-only in Installed tab.
                self.orphans_only = !self.orphans_only;
                self.last_query.clear();
                self.clear_cache();
                self.is_loading = true;
                self.trigger_search();
            }
            Action::CleanPreview => {
                // 5.1: c cleans the package cache (Installed/Updates list).
                // Preview is a fast local paccache scan; missing paccache is a
                // message, never a panic.
                if self.readonly {
                    self.popup = Popup::Message("Readonly mode — clean disabled".into());
                } else {
                    let keep = self.config.behavior.clean_keep.max(1);
                    match crate::install::clean::preview_clean(keep) {
                        Ok(p) => {
                            self.popup = Popup::ConfirmClean {
                                files: p.files,
                                bytes: p.bytes,
                                flatpak: self.config.flatpak_enabled()
                                    && crate::sources::has_flatpak(),
                            };
                        }
                        Err(e) => {
                            self.popup = Popup::Message(format!("Cache preview: {e}"));
                        }
                    }
                }
            }
            Action::RemoveSelected => {
                if let Some(idx) = self.list_state.selected()
                    && let Some(pkg) = self.packages.get(idx).cloned()
                {
                    self.popup = Popup::ConfirmRemove(pkg);
                }
            }
            Action::InfoSelected => {
                if let Some(idx) = self.list_state.selected()
                    && let Some(pkg) = self.packages.get(idx).cloned()
                {
                    self.popup = Popup::Info(pkg);
                }
            }
            Action::MovePrev => {
                self.select_prev();
            }
            Action::MoveNext => {
                self.select_next();
            }
            Action::TypeForced => {
                // The search branch of the old `i` / Up / Down arms: flags set
                // unconditionally (kept verbatim against the old arms).
                self.input.handle_event(ev);
                self.needs_search = true;
                self.last_input_change = Instant::now();
            }
            Action::TypeSearch => {
                // For search focus, forward all to tui-input.
                let prev = self.input.value().to_string();
                self.input.handle_event(ev);
                if prev != self.input.value() {
                    self.needs_search = true;
                    self.last_input_change = Instant::now();
                }
            }
            Action::TypeFromList => {
                // If typing in list mode (letter), jump to search and insert.
                self.focus = Focus::Search;
                self.input.handle_event(ev);
                self.needs_search = true;
                self.last_input_change = Instant::now();
            }
        }
        Ok(())
    }

    fn select_next(&mut self) {
        if self.packages.is_empty() {
            return;
        }
        let i = match self.list_state.selected() {
            Some(i) => (i + 1) % self.packages.len(),
            None => 0,
        };
        self.list_state.select(Some(i));
    }

    fn select_prev(&mut self) {
        if self.packages.is_empty() {
            return;
        }
        let i = match self.list_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.packages.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.list_state.select(Some(i));
    }

    // Phase B: non-blocking — spawn background thread, parallel repo+aur via OnceLock client & cached Config
    // Phase C: respect CLI filters (source, by, regex, installed_only, limit) per tui audit
    // v0.3.0: Installed mode uses localdb (allows empty filter = list all); Search uses cached cfg for AUR.
    // 0.5.0: Updates mode lists repo+AUR updates; Installed `o` filters orphans-only.
    pub(crate) fn trigger_search(&mut self) {
        // Updates mode: empty filter lists all updates (repo + AUR).
        if self.mode == Mode::Updates {
            let query = self.input.value().trim().to_string();
            if query == self.last_query && !self.packages.is_empty() {
                self.is_loading = false;
                return;
            }
            let key = self.cache_key(&query);
            if let Some(pkgs) = self.cache_get(&key) {
                self.search_rx = None;
                self.apply_results(pkgs, query);
                return;
            }
            self.is_loading = true;
            self.dirty = true;
            self.status = "Checking for updates...".into();
            self.next_search_id = self.next_search_id.wrapping_add(1);
            self.search_id = self.next_search_id;
            self.active_search.store(self.search_id, Ordering::Relaxed);
            let search_id = self.search_id;
            let active = self.active_search.clone();
            let limit = self.limit;
            let use_regex = self.use_regex;
            let query_clone = query.clone();
            let cfg = self.config.clone();
            let (tx, rx) = mpsc::channel();
            self.search_rx = Some(rx);
            thread::spawn(move || {
                // Perf P3: repo/AUR/flatpak checks run on parallel threads
                // (mirror of the search fan-out); joins carry panic guards.
                // Timings live inside each thread for true per-backend latency.
                // One-shot RPC warm on its own thread (blocking HEAD never
                // serializes this orchestrator); the AUR info probe then rides
                // a warm pool.
                {
                    let cfg_w = cfg.clone();
                    std::thread::spawn(move || crate::search::aur::warm_blocking_aur(&cfg_w));
                }
                let repo_h = std::thread::spawn(|| {
                    let timer = crate::timing::start("tui-repo-updates");
                    let out = crate::updates::check_repo_updates().unwrap_or_default();
                    timer.finish(out.len());
                    out
                });
                let cfg_a = cfg.clone();
                let aur_h = std::thread::spawn(move || {
                    let timer = crate::timing::start("tui-aur-updates");
                    let out = crate::updates::check_aur_updates_blocking(&cfg_a);
                    timer.finish(out.len());
                    out
                });
                let cfg_f = cfg.clone();
                let fp_h = std::thread::spawn(move || {
                    let timer = crate::timing::start("tui-flatpak-updates");
                    let out = crate::sources::check_flatpak_updates_cached(&cfg_f);
                    timer.finish(out.len());
                    out
                });
                let mut all = match repo_h.join() {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::warn!("repo updates worker panicked: {:?}", e);
                        Vec::new()
                    }
                };
                all.extend(match aur_h.join() {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::warn!("aur updates worker panicked: {:?}", e);
                        Vec::new()
                    }
                });
                all.extend(match fp_h.join() {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::warn!("flatpak updates worker panicked: {:?}", e);
                        Vec::new()
                    }
                });
                all.sort_by(|a, b| a.repo.cmp(&b.repo).then(a.name.cmp(&b.name)));
                // Filter by input (name substring, or regex when enabled).
                let filtered: Vec<crate::updates::PkgUpdate> = if query_clone.is_empty() {
                    all
                } else if use_regex {
                    match regex::RegexBuilder::new(&query_clone)
                        .case_insensitive(true)
                        .build()
                    {
                        Ok(re) => all.into_iter().filter(|u| re.is_match(&u.name)).collect(),
                        Err(_) => all,
                    }
                } else {
                    let ql = query_clone.to_lowercase();
                    all.into_iter()
                        .filter(|u| u.name.to_lowercase().contains(&ql))
                        .collect()
                };
                let mut pkgs: Vec<Package> = filtered
                    .iter()
                    .map(|u| {
                        let mut p =
                            Package::minimal(u.name.clone(), u.new_version.clone(), u.repo.clone());
                        p.installed = true;
                        // Update rows render versions from the AUR RPC into a
                        // description slot — display-sanitize at the boundary.
                        p.description = Some(crate::safety::text::sanitize_terminal(&format!(
                            "{} -> {}",
                            u.old_version, u.new_version
                        )));
                        p.out_of_date = if u.out_of_date { Some(1) } else { None };
                        p
                    })
                    .collect();
                if limit != 0 && pkgs.len() > limit {
                    pkgs.truncate(limit);
                }
                if active.load(Ordering::Relaxed) != search_id {
                    return;
                }
                let _ = tx.send(SearchMsg {
                    search_id,
                    stage: SearchStage::Done,
                    query: query_clone,
                    packages: pkgs,
                });
            });
            return;
        }
        // Installed mode: empty filter lists all installed (up to limit)
        if self.mode == Mode::Installed {
            let query = self.input.value().trim().to_string();
            if query == self.last_query && !self.packages.is_empty() {
                self.is_loading = false;
                return;
            }
            // Serve recent filters instantly (no disk scan).
            let key = self.cache_key(&query);
            if let Some(pkgs) = self.cache_get(&key) {
                self.search_rx = None;
                self.apply_results(pkgs, query);
                return;
            }
            self.is_loading = true;
            self.dirty = true;
            self.status = if query.is_empty() {
                if self.orphans_only {
                    "Listing orphans...".into()
                } else {
                    "Listing installed packages...".into()
                }
            } else {
                format!("Filtering installed for '{}'...", query)
            };
            self.next_search_id = self.next_search_id.wrapping_add(1);
            self.search_id = self.next_search_id;
            self.active_search.store(self.search_id, Ordering::Relaxed);
            let search_id = self.search_id;
            let active = self.active_search.clone();
            let limit = self.limit;
            let use_regex = self.use_regex;
            let orphans_only = self.orphans_only;
            let query_clone = query.clone();
            let (tx, rx) = mpsc::channel();
            self.search_rx = Some(rx);
            thread::spawn(move || {
                let mut res = crate::search::repo::search_local(&query_clone, limit, use_regex)
                    .or_else(|_| {
                        crate::search::repo::search_local_fallback(&query_clone, limit, use_regex)
                    })
                    .unwrap_or_default();
                if orphans_only {
                    let orphans = crate::orphans::list_orphans()
                        .or_else(|_| crate::orphans::list_orphans_fallback())
                        .unwrap_or_default();
                    let names: std::collections::HashSet<String> =
                        orphans.into_iter().map(|p| p.name).collect();
                    res.retain(|p| names.contains(&p.name));
                    // Enrich orphan flag for badges.
                    for p in &mut res {
                        p.orphan = true;
                        p.reason = Some("dependency".to_string());
                    }
                }
                // Superseded while scanning (e.g. rapid Tab switches): drop the
                // result instead of waking the UI with stale data.
                if active.load(Ordering::Relaxed) != search_id {
                    return;
                }
                let _ = tx.send(SearchMsg {
                    search_id,
                    stage: SearchStage::Done,
                    query: query_clone,
                    packages: res,
                });
            });
            return;
        }

        let query = self.input.value().trim().to_string();
        if query.is_empty() {
            self.packages.clear();
            self.status = "Type a query and press Enter".into();
            self.is_loading = false;
            self.search_rx = None;
            self.dirty = true;
            return;
        }
        if query == self.last_query && !self.packages.is_empty() {
            self.is_loading = false;
            return;
        }
        // Serve recent queries instantly (no disk/network).
        let key = self.cache_key(&query);
        if let Some(pkgs) = self.cache_get(&key) {
            self.search_rx = None;
            self.apply_results(pkgs, query);
            return;
        }
        self.is_loading = true;
        self.dirty = true;
        self.status = format!("Searching for '{}'...", query);
        self.next_search_id = self.next_search_id.wrapping_add(1);
        self.search_id = self.next_search_id;
        self.active_search.store(self.search_id, Ordering::Relaxed);
        let search_id = self.search_id;
        let active = self.active_search.clone();
        let limit = self.limit;
        let source = self.source;
        let aur_by = self.aur_by;
        let use_regex = self.use_regex;
        let installed_only = self.installed_only;
        let query_clone = query.clone();

        let (tx, rx) = mpsc::channel();
        self.search_rx = Some(rx);
        // Clone cached config once — avoids Config::load() file IO per keystroke in AUR path.
        let cfg = self.config.clone();

        thread::spawn(move || {
            let aur_by_str = aur_by.as_str().to_string();
            // One-shot RPC warm (P3) on its own thread — a HEAD when nothing
            // else can serve it, overlapping the repo leg of this same query;
            // later queries reuse the pooled connection for instant AUR latency.
            {
                let cfg_w = cfg.clone();
                std::thread::spawn(move || crate::search::aur::warm_blocking_aur(&cfg_w));
            }
            // Spawn repo, aur, flatpak in parallel, respecting source filter.
            // Flatpak is fail-open empty when disabled/binary missing (P2).
            let flatpak_on = matches!(source, Source::Flatpak | Source::All)
                && !installed_only
                && cfg.flatpak_enabled();
            let q1 = query_clone.clone();
            let repo_handle = if matches!(source, Source::Aur | Source::Flatpak) {
                None
            } else {
                Some(thread::spawn(move || {
                    let timer = crate::timing::start("tui-repo-search");
                    let out: Vec<Package> =
                        crate::search::repo::search_repo(&q1, limit, use_regex, installed_only)
                            .or_else(|_| crate::search::repo::search_repo_fallback(&q1, limit))
                            .unwrap_or_default();
                    timer.finish(out.len());
                    out
                }))
            };
            let q2 = query_clone.clone();
            let cfg2 = cfg.clone();
            let aur_handle = if matches!(source, Source::Repo | Source::Flatpak) || installed_only {
                None
            } else {
                Some(thread::spawn(move || {
                    let timer = crate::timing::start("tui-aur-search");
                    let out: Vec<Package> =
                        search_aur_blocking_with_config(&q2, &aur_by_str, limit, use_regex, &cfg2)
                            .unwrap_or_else(|e| {
                                tracing::warn!(err=?e, "AUR blocking search failed");
                                vec![]
                            });
                    timer.finish(out.len());
                    out
                }))
            };
            let q3 = query_clone.clone();
            let cfg3 = cfg.clone();
            let flatpak_handle = if !flatpak_on {
                None
            } else {
                Some(thread::spawn(move || {
                    let timer = crate::timing::start("tui-flatpak-search");
                    let out: Vec<Package> =
                        crate::sources::search_flatpak_with_config(&q3, limit, use_regex, &cfg3)
                            .unwrap_or_else(|e| {
                                tracing::warn!(err=?e, "flatpak search failed");
                                vec![]
                            });
                    timer.finish(out.len());
                    out
                }))
            };
            let total_timer = crate::timing::start("tui-search-total");

            // P1 perf: per-backend timings (TUI logs to file; slow calls warn).
            let repo_res = repo_handle
                .map(|h| match h.join() {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::warn!("repo worker panicked: {:?}", e);
                        Vec::new()
                    }
                })
                .unwrap_or_default();
            let aur_res = aur_handle
                .map(|h| match h.join() {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::warn!("aur worker panicked: {:?}", e);
                        Vec::new()
                    }
                })
                .unwrap_or_default();
            let flatpak_res = flatpak_handle
                .map(|h| match h.join() {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::warn!("flatpak worker panicked: {:?}", e);
                        Vec::new()
                    }
                })
                .unwrap_or_default();
            // Respect source order: repo, aur, flatpak (bottom_up handled in
            // UI) — the stages below arrive and accumulate in exactly this
            // order because the joins are sequential.
            let total_results = repo_res
                .len()
                .saturating_add(aur_res.len())
                .saturating_add(flatpak_res.len());

            // P4 staged: paint each leg the moment it joins — the repo block
            // (~150ms) no longer waits for AUR. Stage sends validate the
            // generation too, so a superseded search drops out incrementally.
            if active.load(Ordering::Relaxed) == search_id {
                let _ = tx.send(SearchMsg {
                    search_id,
                    stage: SearchStage::Repo,
                    query: query_clone.clone(),
                    packages: repo_res,
                });
            }
            if active.load(Ordering::Relaxed) == search_id {
                let _ = tx.send(SearchMsg {
                    search_id,
                    stage: SearchStage::Aur,
                    query: query_clone.clone(),
                    packages: aur_res,
                });
            }
            if active.load(Ordering::Relaxed) == search_id {
                let _ = tx.send(SearchMsg {
                    search_id,
                    stage: SearchStage::Flatpak,
                    query: query_clone.clone(),
                    packages: flatpak_res,
                });
            }
            total_timer.finish(total_results);

            // Final marker: loading off + cache + final status atomically.
            if active.load(Ordering::Relaxed) == search_id {
                let _ = tx.send(SearchMsg {
                    search_id,
                    stage: SearchStage::Done,
                    query: query_clone,
                    packages: Vec::new(),
                });
            }
        });
    }

    fn do_search(&mut self) -> Result<()> {
        // Keep API compat for handle_event and initial search; now non-blocking
        self.trigger_search();
        Ok(())
    }

    fn do_install(&mut self, pkg: Package, terminal: &mut DefaultTerminal) -> Result<()> {
        if self.readonly {
            self.popup = Popup::Message("Readonly mode — install disabled".into());
            self.status = "Refused install: readonly mode".into();
            self.dirty = true;
            return Ok(());
        }
        // Suspend TUI and run pacman / makepkg / flatpak per aur-guides + tui-design lifecycle
        let pkg_clone = pkg.clone();
        let is_aur = pkg.repo == "aur";
        let is_flatpak = pkg.repo == "flatpak";
        let name = pkg.name.clone();
        let name_for_closure = name.clone();
        let cfg_for_closure = self.config.clone();

        // Prepare install function that runs outside TUI
        let install_result = super::super::tui::suspend_and_run(terminal, move || {
            if is_flatpak {
                // Updates-tab rows carry installed=true (built from PkgUpdate);
                // Search rows carry installed=false. Update vs install follows.
                if pkg_clone.installed {
                    crate::sources::update_flatpak_package(&name_for_closure, &cfg_for_closure)
                } else {
                    crate::sources::install_flatpak_package(&name_for_closure, &cfg_for_closure)
                }
            } else if is_aur {
                crate::install::aur::install_aur_package(&pkg_clone)
            } else {
                crate::install::repo::install_repo_package(&name_for_closure)
            }
        });

        match install_result {
            Ok(()) => {
                if is_flatpak {
                    crate::sources::invalidate_flatpak_memo();
                    crate::sources::invalidate_flatpak_list_memo();
                }
                self.status = format!("Installed {}", name);
                self.popup = Popup::Message(format!("✓ Installed {}", name));
            }
            Err(e) => {
                self.status = format!("Install failed: {}", e);
                self.popup = Popup::Message(format!("✗ Failed {}: {}", name, e));
            }
        }
        // Refresh package list to update [installed] marker — force even if query==last_query
        self.last_query.clear();
        self.clear_cache();
        self.dirty = true;
        self.trigger_search();
        Ok(())
    }

    fn do_remove(&mut self, pkg: Package, terminal: &mut DefaultTerminal) -> Result<()> {
        if self.readonly {
            self.popup = Popup::Message("Readonly mode — remove disabled".into());
            self.status = "Refused remove: readonly mode".into();
            self.dirty = true;
            return Ok(());
        }
        let name = pkg.name.clone();
        let name_for_closure = name.clone();
        let cfg = self.config.clone();
        let flag_preview = cfg.remove_flag_arg();
        let is_flatpak = pkg.repo == "flatpak";

        let remove_result = super::super::tui::suspend_and_run(terminal, move || {
            if is_flatpak {
                crate::sources::remove_flatpak_package(&name_for_closure, &cfg)
            } else {
                crate::install::remove::remove_package_with_config(&name_for_closure, &cfg)
            }
        });

        match remove_result {
            Ok(()) => {
                let marker = if is_flatpak {
                    "flatpak"
                } else {
                    flag_preview.as_str()
                };
                if is_flatpak {
                    crate::sources::invalidate_flatpak_memo();
                    crate::sources::invalidate_flatpak_list_memo();
                }
                self.status = format!("Removed {} ({})", name, marker);
                self.popup = Popup::Message(format!("✓ Removed {} ({})", name, marker));
            }
            Err(e) => {
                self.status = format!("Remove failed: {}", e);
                self.popup = Popup::Message(format!("✗ Failed {}: {}", name, e));
            }
        }
        // Refresh installed list — force even if filter unchanged
        self.last_query.clear();
        self.last_query_installed.clear();
        self.clear_cache();
        self.dirty = true;
        self.trigger_search();
        Ok(())
    }

    fn do_refresh(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        if self.readonly {
            self.popup = Popup::Message("Readonly mode — refresh disabled".into());
            self.status = "Refused refresh: readonly mode".into();
            self.dirty = true;
            return Ok(());
        }
        // Suspend TUI and run `sudo pacman -Sy` with inherited stdio (real TTY for sudo)
        let cfg = self.config.clone();
        let refresh_result = super::super::tui::suspend_and_run(terminal, move || {
            crate::install::refresh::refresh_sync_db(&cfg)
        });

        match refresh_result {
            Ok(()) => {
                self.status = "Sync databases refreshed".into();
                self.popup = Popup::Message("✓ Sync databases refreshed".into());
            }
            Err(e) => {
                self.status = format!("Refresh failed: {e}");
                self.popup = Popup::Message(format!("✗ Refresh failed: {e}"));
            }
        }
        // Sync DBs changed — force re-query even if query==last_query
        self.last_query.clear();
        self.last_query_search.clear();
        self.last_query_installed.clear();
        self.last_query_updates.clear();
        self.clear_cache();
        crate::updates::invalidate_aur_memo();
        crate::sources::invalidate_flatpak_memo();
        self.dirty = true;
        let should_fetch =
            !matches!(self.mode, Mode::Search) || !self.input.value().trim().is_empty();
        if should_fetch {
            self.is_loading = true;
            self.trigger_search();
        }
        Ok(())
    }

    fn do_upgrade(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        if self.readonly {
            self.popup = Popup::Message("Readonly mode — upgrade disabled".into());
            self.status = "Refused upgrade: readonly mode".into();
            self.dirty = true;
            return Ok(());
        }
        // 5.1: news warning already surfaced inline in the popup (set on `U`);
        // confirming here means the user read it — proceed directly.
        // Flatpak refs upgrade alongside pacman (best-effort, no sudo).
        let cfg = self.config.clone();
        let res = super::super::tui::suspend_and_run(terminal, move || -> anyhow::Result<()> {
            crate::install::upgrade::upgrade_system(&cfg)?;
            if let Err(e) = crate::sources::update_all_flatpak(&cfg) {
                eprintln!("warning: flatpak update failed: {e}");
            }
            Ok(())
        });
        match res {
            Ok(()) => {
                self.status = "System upgraded".into();
                self.popup = Popup::Message("✓ System upgraded".into());
            }
            Err(e) => {
                self.status = format!("Upgrade failed: {e}");
                self.popup = Popup::Message(format!("✗ Upgrade failed: {e}"));
            }
        }
        self.last_query.clear();
        self.last_query_search.clear();
        self.last_query_installed.clear();
        self.last_query_updates.clear();
        self.clear_cache();
        crate::updates::invalidate_aur_memo();
        crate::sources::invalidate_flatpak_memo();
        crate::sources::invalidate_flatpak_list_memo();
        self.dirty = true;
        let should_fetch =
            !matches!(self.mode, Mode::Search) || !self.input.value().trim().is_empty();
        if should_fetch {
            self.is_loading = true;
            self.trigger_search();
        }
        Ok(())
    }

    fn do_clean(
        &mut self,
        files: usize,
        bytes: u64,
        flatpak: bool,
        terminal: &mut DefaultTerminal,
    ) -> Result<()> {
        if self.readonly {
            self.popup = Popup::Message("Readonly mode — clean disabled".into());
            self.status = "Refused clean: readonly mode".into();
            self.dirty = true;
            return Ok(());
        }
        // Popup confirm covers both stages: force -y downstream.
        let keep = self.config.behavior.clean_keep.max(1);
        let cfg = self.config.clone();
        let res = super::super::tui::suspend_and_run(terminal, move || -> anyhow::Result<bool> {
            if files > 0 {
                crate::install::clean::run_clean(&cfg, keep)?;
            }
            if flatpak {
                let mut cfg_fp = cfg.clone();
                cfg_fp.behavior.flatpak_noconfirm = true;
                crate::sources::clean_unused_flatpak(&cfg_fp)?;
            }
            Ok(flatpak)
        });
        match res {
            Ok(fp) => {
                let size = crate::stats::human_bytes(bytes);
                let mut msg = format!("Cleaned {files} files ({size})");
                if fp {
                    msg.push_str(" + flatpak unused");
                    // P5 audit: the unused-runtime drop changed the flatpak
                    // inventory — the 60s list memo must not serve stale rows.
                    crate::sources::invalidate_flatpak_list_memo();
                }
                self.status = msg.clone();
                self.popup = Popup::Message(format!("✓ {msg}"));
            }
            Err(e) => {
                self.status = format!("Clean failed: {e}");
                self.popup = Popup::Message(format!("✗ Clean failed: {e}"));
            }
        }
        self.last_query.clear();
        self.last_query_search.clear();
        self.last_query_installed.clear();
        self.last_query_updates.clear();
        self.clear_cache();
        crate::updates::invalidate_aur_memo();
        crate::sources::invalidate_flatpak_memo();
        self.dirty = true;
        let should_fetch =
            !matches!(self.mode, Mode::Search) || !self.input.value().trim().is_empty();
        if should_fetch {
            self.is_loading = true;
            self.trigger_search();
        }
        Ok(())
    }
}

/// True when `path` (a `/proc/self/fd/N` link) is unusable: the link shows
/// `(deleted)`, re-opening it fails (EIO once a pty is torn down), or a
/// zero-byte write fails. Zero-byte writes are side-effect-free on live fds.
/// Linux-only, like the rest of this binary.
fn path_dead(path: &str) -> bool {
    use std::io::Write;
    if std::fs::read_link(path)
        .map(|t| t.to_string_lossy().contains("(deleted)"))
        .unwrap_or(false)
    {
        return true;
    }
    match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
    {
        Ok(mut f) => f.write(b"").is_err(),
        Err(_) => true,
    }
}

/// The TUI can never interact again once stdio's terminal is gone — and
/// crossterm's poll busy-spins on the resulting EIO instead of returning.
/// The watchdog (see run()) uses this to quit instead of spinning at ~100%
/// CPU as an orphan.
fn stdio_dead() -> bool {
    path_dead("/proc/self/fd/0") || path_dead("/proc/self/fd/1")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn path_dead_classifies_fds() {
        // Healthy character device: usable.
        assert!(!path_dead("/dev/null"));
        // Missing path: unusable.
        assert!(path_dead("/proc/self/fd/99999"));
        // Unlinked path: unusable (same signal as a torn-down pty).
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("x");
        std::fs::write(&p, "x").unwrap();
        let _held = std::fs::File::open(&p).unwrap();
        std::fs::remove_file(&p).unwrap();
        assert!(path_dead(p.to_str().unwrap()));
        // Our own stdio in the test harness is never dead.
        assert!(!stdio_dead());
    }

    #[test]
    fn trigger_search_is_non_blocking() {
        // Spawns real repo/AUR workers: hermetic for assertions (no result
        // dependency), but pass PAKMAP_SKIP_LIVE=1 for fully offline runs.
        if std::env::var("PAKMAP_SKIP_LIVE").is_ok() {
            return;
        }
        let mut app = App::new("firefox".into());
        let start = Instant::now();
        app.trigger_search();
        let elapsed = start.elapsed();
        // Should return immediately without blocking on network/disk (background thread)
        assert!(
            elapsed < Duration::from_millis(100),
            "trigger_search should be non-blocking, took {:?}",
            elapsed
        );
        assert!(app.is_loading, "should be loading after trigger");
        // Poll should not panic even if no result yet
        app.poll_search_results();
    }

    #[test]
    fn trigger_search_empty_clears() {
        let mut app = App::new("".into());
        app.trigger_search();
        assert!(!app.is_loading);
        assert!(app.packages.is_empty());
    }

    // -------------------------------------------------------------------
    // P4 staged results: per-leg paints, atomic finalization.
    // -------------------------------------------------------------------

    /// Feed messages to the app exactly as the poll loop would see them.
    fn feed_messages(app: &mut App, msgs: Vec<SearchMsg>) {
        let (tx, rx) = std::sync::mpsc::channel();
        for m in msgs {
            let _ = tx.send(m);
        }
        app.search_rx = Some(rx);
        app.poll_search_results();
        drop(tx);
    }

    fn pkg(repo: &str, name: &str) -> Package {
        Package::minimal(name.to_string(), "1.0".to_string(), repo.to_string())
    }

    // v0.9.3 Phase 3 (CWE-150): results become visible only after the
    // display-boundary filter — escapes/bidi can no longer hide review lines
    // in the TUI, while ordinary text (Arabic, CJK) stays byte-identical.
    #[test]
    fn applied_results_are_sanitize_terminal_filtered() {
        let mut app = App::new("".into());
        let mut hostile = pkg("aur", "n\u{1b}[31normal\u{200e}name");
        hostile.version = "1\u{202e}0-1".into();
        hostile.description = Some("desc\u{1b}]0;clip\u{7}x".into());
        let mut clean = pkg("repo", "الجزء示例3️⃣");
        clean.description = Some("النص واقعي\tسطر".into());

        app.apply_results(vec![hostile, clean], "q".into());
        let [a, b] = app.packages.as_slice() else {
            panic!("two rows expected");
        };
        // Control/bidi bytes dropped; sequence payloads stay inert text.
        assert_eq!(a.name, "n[31normalname");
        assert_eq!(a.version, "10-1");
        assert_eq!(a.description.as_deref(), Some("desc]0;clipx"));
        // Ordinary multilingual content survives byte-identical.
        assert_eq!(b.name, "الجزء示例3️⃣");
        assert_eq!(b.description.as_deref(), Some("النص واقعي\tسطر"));
    }

    #[test]
    fn staged_results_accumulate_then_swap_atomically() {
        let mut app = App::new("".into());
        app.search_id = 7;
        app.packages = vec![pkg("repo", "old-row")]; // last complete generation
        app.is_loading = true;

        feed_messages(
            &mut app,
            vec![SearchMsg {
                search_id: 7,
                stage: SearchStage::Repo,
                query: "ff".into(),
                packages: vec![pkg("repo", "r1"), pkg("repo", "r2")],
            }],
        );
        // Old generation stays visible; scratch accumulates; still loading.
        assert_eq!(app.packages.len(), 1, "old rows must stay until Done");
        assert_eq!(app.staged_packages.len(), 2);
        assert!(app.is_loading);
        assert!(app.status.contains("2 found so far"), "{}", app.status);

        feed_messages(
            &mut app,
            vec![
                SearchMsg {
                    search_id: 7,
                    stage: SearchStage::Aur,
                    query: "ff".into(),
                    packages: vec![pkg("aur", "a1"), pkg("aur", "a2")],
                },
                SearchMsg {
                    search_id: 7,
                    stage: SearchStage::Flatpak,
                    query: "ff".into(),
                    packages: vec![pkg("flatpak", "f1"), pkg("flatpak", "f2")],
                },
            ],
        );
        assert_eq!(app.packages.len(), 1, "still the old generation");
        assert_eq!(app.staged_packages.len(), 6);
        assert_eq!(
            app.staged_packages[0].repo, "repo",
            "pinned platform order repo→aur→flatpak"
        );
        assert_eq!(app.staged_packages[2].repo, "aur");

        feed_messages(
            &mut app,
            vec![SearchMsg {
                search_id: 7,
                stage: SearchStage::Done,
                query: "ff".into(),
                packages: vec![],
            }],
        );
        assert!(!app.is_loading);
        assert_eq!(app.packages.len(), 6);
        assert_eq!(app.packages[0].name, "r1");
        assert_eq!(app.packages[2].name, "a1");
        assert_eq!(app.packages[4].name, "f1");
        assert_eq!(app.status, "Found 6 (repo 2 aur 2 flatpak 2)");
        assert_eq!(app.list_state.selected(), Some(0));
        // Final generation is cached like the old combined send was.
        assert_eq!(
            app.cache_get(&app.cache_key("ff"))
                .unwrap_or_default()
                .len(),
            6
        );
        assert!(app.staged_packages.is_empty(), "scratch drained at Done");
    }

    #[test]
    fn staged_superseded_generation_is_discarded() {
        let mut app = App::new("".into());
        app.search_id = 9; // newer generation alive; ids 7 are stale
        let before_len = app.packages.len();
        let before_status = app.status.clone();

        feed_messages(
            &mut app,
            vec![
                SearchMsg {
                    search_id: 7,
                    stage: SearchStage::Repo,
                    query: "old".into(),
                    packages: vec![pkg("repo", "stale")],
                },
                SearchMsg {
                    search_id: 7,
                    stage: SearchStage::Done,
                    query: "old".into(),
                    packages: vec![],
                },
            ],
        );
        assert!(app.staged_packages.is_empty(), "stale stages never mix");
        assert_eq!(app.packages.len(), before_len);
        assert_eq!(app.status, before_status);
    }

    #[test]
    fn done_with_carry_payload_wins_over_scratch() {
        // Installed/updates workers keep the single-message flow: their Done
        // carries the whole list and must not depend on staged scratch.
        let mut app = App::new("".into());
        app.search_id = 3;
        app.packages = vec![pkg("repo", "stale-rows"), pkg("repo", "more-rows")];
        feed_messages(
            &mut app,
            vec![SearchMsg {
                search_id: 3,
                stage: SearchStage::Done,
                query: "inst".into(),
                packages: vec![pkg("local", "installed-1")],
            }],
        );
        assert!(!app.is_loading);
        assert_eq!(app.packages.len(), 1);
        assert_eq!(app.packages[0].name, "installed-1");
    }

    #[test]
    fn first_stage_replaces_scratch_from_superseded_searches() {
        // A superseded search left scratch behind before Done ever fired; the
        // next generation's first arrival must replace, not append to it.
        let mut app = App::new("".into());
        app.search_id = 4;
        app.staged_packages = vec![pkg("repo", "ghost")];
        app.staged_search_id = Some(2);
        feed_messages(
            &mut app,
            vec![
                SearchMsg {
                    search_id: 4,
                    stage: SearchStage::Repo,
                    query: "q".into(),
                    packages: vec![], // repo leg disabled/empty
                },
                SearchMsg {
                    search_id: 4,
                    stage: SearchStage::Aur,
                    query: "q".into(),
                    packages: vec![pkg("aur", "fresh")],
                },
                SearchMsg {
                    search_id: 4,
                    stage: SearchStage::Done,
                    query: "q".into(),
                    packages: vec![],
                },
            ],
        );
        assert_eq!(app.packages.len(), 1, "ghost must not survive");
        assert_eq!(app.packages[0].name, "fresh");
        assert_eq!(app.status, "Found 1 (repo 0 aur 1 flatpak 0)");
    }

    #[test]
    fn q_in_search_does_not_quit() {
        let mut app = App::new("".into());
        app.focus = Focus::Search;
        let ev = Event::Key(crossterm::event::KeyEvent::new(
            KeyCode::Char('q'),
            KeyModifiers::empty(),
        ));
        let _ = app.handle_event(&ev, None);
        assert!(!app.should_quit, "q in Search should type, not quit");
        assert_eq!(app.input.value(), "q");
    }

    #[test]
    fn q_in_list_quits_through_a_real_terminal() {
        // The Some-path proof: the run loop always passes the terminal down;
        // this drives the same handler shape through a real DefaultTerminal.
        let mut app = App::new("firefox".into());
        app.focus = Focus::List;
        app.popup = Popup::None;
        let ev = Event::Key(crossterm::event::KeyEvent::new(
            KeyCode::Char('q'),
            KeyModifiers::empty(),
        ));
        let backend = ratatui::backend::CrosstermBackend::new(std::io::stdout());
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let _ = app.handle_event(&ev, Some(&mut terminal));
        assert!(app.should_quit, "q in List should quit");
    }

    #[test]
    fn new_with_cli_respects_filters() {
        let cli = crate::cli::Cli {
            query: Some("test".into()),
            tui: false,
            no_tui: false,
            source: crate::cli::Source::Aur,
            by: crate::cli::AurBy::Name,
            limit: 10,
            json: false,
            regex: true,
            installed_only: true,
            bottom_up: false,
            verbose: 0,
            no_color: true,
            config: None,
            init_config: false,
            show_config: false,
            config_cmd: None,
            remove: None,
            refresh: false,
            updates: false,
            orphans: false,
            stats: false,
            upgrade: false,
            clean: false,
            keep: 2,
            dry_run: false,
            readonly: false,
        };
        let app = App::new_with_cli("test".into(), &cli);
        assert_eq!(app.limit, 10);
        assert_eq!(app.source, crate::cli::Source::Aur);
        assert_eq!(app.aur_by, crate::cli::AurBy::Name);
        assert!(app.use_regex);
        assert!(app.installed_only);
        assert!(app.no_color);
    }

    #[test]
    fn list_state_none_on_empty() {
        let app = App::new("".into());
        assert_eq!(app.list_state.selected(), None);
        assert!(app.packages.is_empty());
    }

    #[test]
    fn tab_switches_to_installed_mode() {
        // Installed mode triggers a real localdb scan thread; skip it when
        // PAKMAP_SKIP_LIVE=1 is set for fully offline runs.
        if std::env::var("PAKMAP_SKIP_LIVE").is_ok() {
            return;
        }
        let mut app = App::new("".into());
        assert_eq!(app.mode, Mode::Search);
        app.switch_mode();
        assert_eq!(app.mode, Mode::Installed);
        // Empty filter in Installed lists all -> loading
        assert!(app.is_loading);
        app.switch_mode();
        assert_eq!(app.mode, Mode::Updates);
        app.switch_mode();
        assert_eq!(app.mode, Mode::Search);
    }

    #[test]
    fn tab_key_event_switches_mode() {
        let mut app = App::new("".into());
        app.focus = Focus::Search;
        let ev = Event::Key(crossterm::event::KeyEvent::new(
            KeyCode::Tab,
            KeyModifiers::empty(),
        ));
        let _ = app.handle_event(&ev, None);
        assert_eq!(app.mode, Mode::Installed);
        // Tab must not be typed into input
        assert!(!app.input.value().contains('\t'));
    }

    #[test]
    fn enter_in_installed_opens_confirm_remove() {
        let mut app = App::new("".into());
        app.mode = Mode::Installed;
        app.focus = Focus::List;
        app.packages = vec![crate::model::Package {
            name: "vim".into(),
            version: "1-1".into(),
            description: None,
            repo: "local".into(),
            arch: None,
            url: None,
            installed: true,
            votes: None,
            popularity: None,
            out_of_date: None,
            maintainer: None,
            num_votes: None,
            last_modified: None,
            depends: None,
            optdepends: None,
            required_by: None,
            optional_for: None,
            reason: None,
            orphan: false,
        }];
        app.list_state.select(Some(0));
        let ev = Event::Key(crossterm::event::KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::empty(),
        ));
        let _ = app.handle_event(&ev, None);
        assert!(matches!(app.popup, Popup::ConfirmRemove(_)));
    }

    #[test]
    fn d_in_installed_opens_confirm_remove() {
        let mut app = App::new("".into());
        app.mode = Mode::Installed;
        app.focus = Focus::List;
        app.packages = vec![crate::model::Package {
            name: "vim".into(),
            version: "1-1".into(),
            description: None,
            repo: "local".into(),
            arch: None,
            url: None,
            installed: true,
            votes: None,
            popularity: None,
            out_of_date: None,
            maintainer: None,
            num_votes: None,
            last_modified: None,
            depends: None,
            optdepends: None,
            required_by: None,
            optional_for: None,
            reason: None,
            orphan: false,
        }];
        app.list_state.select(Some(0));
        let ev = Event::Key(crossterm::event::KeyEvent::new(
            KeyCode::Char('d'),
            KeyModifiers::empty(),
        ));
        let _ = app.handle_event(&ev, None);
        assert!(matches!(app.popup, Popup::ConfirmRemove(_)));
    }

    #[test]
    fn cache_hit_serves_instantly() {
        let mut app = App::new("".into());
        app.input = Input::new("vim".into());
        let pkg = crate::model::Package {
            name: "vim".into(),
            version: "1-1".into(),
            description: None,
            repo: "local".into(),
            arch: None,
            url: None,
            installed: true,
            votes: None,
            popularity: None,
            out_of_date: None,
            maintainer: None,
            num_votes: None,
            last_modified: None,
            depends: None,
            optdepends: None,
            required_by: None,
            optional_for: None,
            reason: None,
            orphan: false,
        };
        let key = app.cache_key("vim");
        app.cache_put(key, vec![pkg]);
        app.trigger_search();
        // Cache hit: synchronous, no background thread, no loading spinner
        assert!(!app.is_loading);
        assert_eq!(app.packages.len(), 1);
        assert_eq!(app.packages[0].name, "vim");
    }

    #[test]
    fn cache_evicts_oldest_beyond_cap() {
        let mut app = App::new("".into());
        let mk = |n: &str| crate::model::Package {
            name: n.into(),
            version: "1-1".into(),
            description: None,
            repo: "local".into(),
            arch: None,
            url: None,
            installed: true,
            votes: None,
            popularity: None,
            out_of_date: None,
            maintainer: None,
            num_votes: None,
            last_modified: None,
            depends: None,
            optdepends: None,
            required_by: None,
            optional_for: None,
            reason: None,
            orphan: false,
        };
        for i in 0..(SEARCH_CACHE_CAP + 5) {
            app.cache_put(format!("k{i}"), vec![mk("x")]);
        }
        assert!(app.search_cache.len() <= SEARCH_CACHE_CAP);
        assert!(app.cache_get("k0").is_none());
    }

    #[test]
    fn question_in_list_opens_help() {
        let mut app = App::new("".into());
        app.focus = Focus::List;
        let ev = Event::Key(crossterm::event::KeyEvent::new(
            KeyCode::Char('?'),
            KeyModifiers::empty(),
        ));
        let _ = app.handle_event(&ev, None);
        assert_eq!(app.popup, Popup::Help);
    }

    #[test]
    fn r_in_list_opens_confirm_refresh() {
        let mut app = App::new("".into());
        app.focus = Focus::List;
        let ev = Event::Key(crossterm::event::KeyEvent::new(
            KeyCode::Char('r'),
            KeyModifiers::empty(),
        ));
        let _ = app.handle_event(&ev, None);
        assert_eq!(app.popup, Popup::ConfirmRefresh);
    }

    #[test]
    fn f5_in_list_opens_confirm_refresh() {
        let mut app = App::new("".into());
        app.focus = Focus::List;
        let ev = Event::Key(crossterm::event::KeyEvent::new(
            KeyCode::F(5),
            KeyModifiers::empty(),
        ));
        let _ = app.handle_event(&ev, None);
        assert_eq!(app.popup, Popup::ConfirmRefresh);
    }

    #[test]
    fn r_in_search_types_instead_of_refresh() {
        let mut app = App::new("".into());
        app.focus = Focus::Search;
        let ev = Event::Key(crossterm::event::KeyEvent::new(
            KeyCode::Char('r'),
            KeyModifiers::empty(),
        ));
        let _ = app.handle_event(&ev, None);
        assert_ne!(app.popup, Popup::ConfirmRefresh);
        assert_eq!(app.focus, Focus::Search);
    }

    #[test]
    fn f5_in_search_opens_confirm_refresh() {
        let mut app = App::new("".into());
        app.focus = Focus::Search;
        let ev = Event::Key(crossterm::event::KeyEvent::new(
            KeyCode::F(5),
            KeyModifiers::empty(),
        ));
        let _ = app.handle_event(&ev, None);
        assert_eq!(app.popup, Popup::ConfirmRefresh);
    }

    #[test]
    fn ctrl_r_in_search_opens_confirm_refresh_without_typing() {
        let mut app = App::new("".into());
        app.focus = Focus::Search;
        let ev = Event::Key(crossterm::event::KeyEvent::new(
            KeyCode::Char('r'),
            KeyModifiers::CONTROL,
        ));
        let _ = app.handle_event(&ev, None);
        assert_eq!(app.popup, Popup::ConfirmRefresh);
        assert!(app.input.value().is_empty());
    }

    #[test]
    fn ctrl_r_in_list_opens_confirm_refresh() {
        let mut app = App::new("".into());
        app.focus = Focus::List;
        let ev = Event::Key(crossterm::event::KeyEvent::new(
            KeyCode::Char('R'),
            KeyModifiers::CONTROL,
        ));
        let _ = app.handle_event(&ev, None);
        assert_eq!(app.popup, Popup::ConfirmRefresh);
    }

    #[test]
    fn u_in_updates_opens_confirm_upgrade() {
        // `U` fetches Arch news live for the inline warning; skip the fetch
        // when PAKMAP_SKIP_LIVE=1 (popup shape is snapshot-covered).
        if std::env::var("PAKMAP_SKIP_LIVE").is_ok() {
            return;
        }
        let mut app = App::new("".into());
        app.mode = Mode::Updates;
        app.focus = Focus::List;
        let ev = Event::Key(crossterm::event::KeyEvent::new(
            KeyCode::Char('U'),
            KeyModifiers::empty(),
        ));
        let _ = app.handle_event(&ev, None);
        assert_eq!(app.popup, Popup::ConfirmUpgrade);
    }

    #[test]
    fn u_in_search_types_instead_of_upgrade() {
        // 5.1 scoping: U outside Updates must not pop an upgrade prompt.
        let mut app = App::new("".into());
        app.mode = Mode::Search;
        app.focus = Focus::List;
        let ev = Event::Key(crossterm::event::KeyEvent::new(
            KeyCode::Char('U'),
            KeyModifiers::empty(),
        ));
        let _ = app.handle_event(&ev, None);
        assert_ne!(app.popup, Popup::ConfirmUpgrade);
        // Falls through to list-typing: focus jumps to search with U typed.
        assert_eq!(app.focus, Focus::Search);
    }

    #[test]
    fn backtab_reverses_tab_cycle() {
        let mut app = App::new("".into());
        assert_eq!(app.mode, Mode::Search);
        app.switch_mode_back();
        assert_eq!(app.mode, Mode::Updates);
        app.switch_mode_back();
        assert_eq!(app.mode, Mode::Installed);
        app.switch_mode_back();
        assert_eq!(app.mode, Mode::Search);
    }

    #[test]
    fn backtab_key_event_reverses_mode() {
        let mut app = App::new("".into());
        let ev = Event::Key(crossterm::event::KeyEvent::new(
            KeyCode::BackTab,
            KeyModifiers::empty(),
        ));
        let _ = app.handle_event(&ev, None);
        assert_eq!(app.mode, Mode::Updates);
    }

    #[test]
    fn c_in_installed_opens_clean_or_hint() {
        let mut app = App::new("".into());
        app.mode = Mode::Installed;
        app.focus = Focus::List;
        let ev = Event::Key(crossterm::event::KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::empty(),
        ));
        let _ = app.handle_event(&ev, None);
        // paccache present → ConfirmClean; absent (off-Arch CI) → Message hint.
        assert!(
            matches!(app.popup, Popup::ConfirmClean { .. } | Popup::Message(_)),
            "c should open clean confirm or a missing-tool hint"
        );
    }

    #[test]
    fn o_in_installed_toggles_orphans_only() {
        if std::env::var("PAKMAP_SKIP_LIVE").is_ok() {
            return;
        }
        let mut app = App::new("".into());
        app.mode = Mode::Installed;
        app.focus = Focus::List;
        assert!(!app.orphans_only);
        let ev = Event::Key(crossterm::event::KeyEvent::new(
            KeyCode::Char('o'),
            KeyModifiers::empty(),
        ));
        let _ = app.handle_event(&ev, None);
        assert!(app.orphans_only);
    }

    #[test]
    fn readonly_blocks_mutating_ops() {
        let mut app = App::new("".into());
        app.readonly = true;
        let backend = ratatui::backend::CrosstermBackend::new(std::io::stdout());
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let pkg = crate::model::Package::minimal("vim".into(), "1-1".into(), "extra".into());
        let _ = app.do_install(pkg.clone(), &mut terminal);
        assert!(matches!(app.popup, Popup::Message(_)));
        let _ = app.do_remove(pkg, &mut terminal);
        assert!(matches!(app.popup, Popup::Message(_)));
        // P3: flatpak rows refuse identically without spawning flatpak.
        let fp = crate::model::Package::minimal(
            "org.mozilla.firefox".into(),
            "158.0".into(),
            "flatpak".into(),
        );
        let _ = app.do_install(fp.clone(), &mut terminal);
        assert!(matches!(app.popup, Popup::Message(_)));
        let _ = app.do_remove(fp, &mut terminal);
        assert!(matches!(app.popup, Popup::Message(_)));
        let _ = app.do_upgrade(&mut terminal);
        assert!(matches!(app.popup, Popup::Message(_)));
    }

    #[test]
    fn three_tab_cycle_covers_updates() {
        let mut app = App::new("".into());
        assert_eq!(app.mode.tab_index(), 0);
        app.switch_mode();
        assert_eq!(app.mode, Mode::Installed);
        // Updates fetch spawns workers; skip live part offline.
        if std::env::var("PAKMAP_SKIP_LIVE").is_ok() {
            return;
        }
        app.switch_mode();
        assert_eq!(app.mode, Mode::Updates);
        assert_eq!(app.mode.tab_index(), 2);
    }
    // -------------------------------------------------------------------
    // Overhaul phase 3: the applied dispatch matrix.
    //
    // The same state × key grid `tui::keys::BINDINGS` declares, driven
    // through the REAL run-loop handlers (App × synthetic crossterm
    // events). Subprocess-bound rows (ConfirmActive beyond its state-set,
    // `U` upgrade-all's live news fetch) are resolved-only — the resolver
    // matrix in `tui::keys` owns those arms; these tests assert observable
    // state only, so the whole matrix runs hermetic (no terminal, no
    // network, no child processes).
    // -------------------------------------------------------------------

    /// A plain press exactly as crossterm delivers it.
    fn press_key(code: KeyCode) -> Event {
        Event::Key(crossterm::event::KeyEvent::new(code, KeyModifiers::empty()))
    }

    fn press_ctrl_key(code: KeyCode) -> Event {
        Event::Key(crossterm::event::KeyEvent::new(code, KeyModifiers::CONTROL))
    }

    /// Frame-layer drive: no popup, no modal, terminal-less (state-only
    /// arms — every frame body here touches no subprocess).
    fn frame(app: &mut App, code: KeyCode) {
        let _ = app.handle_event(&press_key(code), None);
    }

    fn frame_ctrl(app: &mut App, code: KeyCode) {
        let _ = app.handle_event(&press_ctrl_key(code), None);
    }

    /// Popup-layer drive: asserts the consume-everything contract on the way.
    fn popup(app: &mut App, code: KeyCode) {
        assert!(
            app.handle_popup_event(&press_key(code), None).unwrap(),
            "popup layer must consume"
        );
    }

    /// Modal-layer drive (settings open; typing-guard territory).
    fn modal(app: &mut App, code: KeyCode) {
        let _ = app.handle_settings_event(&press_key(code), None);
    }

    #[test]
    fn applied_frame_tab_backtab_and_esc_toggle() {
        let mut app = App::new("".into());
        for focus in [Focus::Search, Focus::List] {
            app.focus = focus;
            // Tab cycles from either focus (Guard::Any row).
            for want in [Mode::Installed, Mode::Updates, Mode::Search] {
                frame(&mut app, KeyCode::Tab);
                assert_eq!(app.mode, want);
            }
            // BackTab walks it backwards.
            frame(&mut app, KeyCode::BackTab);
            assert_eq!(app.mode, Mode::Updates);
            frame(&mut app, KeyCode::BackTab);
            assert_eq!(app.mode, Mode::Installed);
            frame(&mut app, KeyCode::BackTab);
            assert_eq!(app.mode, Mode::Search);
            // The cycle never touches focus.
            assert_eq!(app.focus, focus);
        }
        // Esc toggles focus one layer at a time — never the mode.
        app.focus = Focus::Search;
        frame(&mut app, KeyCode::Esc);
        assert_eq!(app.focus, Focus::List);
        frame(&mut app, KeyCode::Esc);
        assert_eq!(app.focus, Focus::Search);
        assert_eq!(app.mode, Mode::Search);
    }

    #[test]
    fn applied_esc_unwinds_exactly_one_layer() {
        let mut app = App::new("".into());
        app.focus = Focus::List;
        // Layer 1: a popup. Esc closes the popup and NOT the frame focus.
        // (Driven through the popup handler — the run-loop layer that owns
        // every press while a popup is open.)
        app.packages = vec![pkg("repo", "a")];
        app.list_state.select(Some(0));
        frame(&mut app, KeyCode::Char('i'));
        assert!(matches!(app.popup, Popup::Info(_)));
        assert!(
            app.handle_popup_event(&press_key(KeyCode::Esc), None)
                .unwrap()
        );
        assert!(matches!(app.popup, Popup::None), "popup closed");
        assert!(app.settings.is_none(), "frame state untouched");
        assert_eq!(app.focus, Focus::List);
        // Layer 2: the settings modal. Esc closes the modal, NOT the frame
        // (the frame's Esc toggle must not fire on the same press).
        frame(&mut app, KeyCode::Char('S'));
        assert!(app.settings.is_some());
        modal(&mut app, KeyCode::Esc);
        assert!(app.settings.is_none(), "modal closed");
        assert!(matches!(app.popup, Popup::None));
        assert_eq!(app.focus, Focus::List);
        // Layer 3: the frame Esc rows (Search↔List).
        app.focus = Focus::Search;
        frame(&mut app, KeyCode::Esc);
        assert_eq!(app.focus, Focus::List);
    }

    #[test]
    fn applied_enter_from_list_opens_the_mode_dialogs() {
        let mut app = App::new("".into());
        app.packages = vec![pkg("repo", "a")];
        app.list_state.select(Some(0));
        app.focus = Focus::List;
        frame(&mut app, KeyCode::Enter);
        assert!(
            matches!(app.popup, Popup::Confirm(_)),
            "Search row installs"
        );
        // y/Enter beyond this point are resolved-only (keys.rs matrix):
        // do_install runs a child process — the state set is asserted above.
        // Tab (switch_mode) clears the rows and the selection — the exact
        // v0.10 sweep — so each leg re-populates the list first.
        app.switch_mode();
        assert!(app.packages.is_empty(), "Tab sweeps the rows");
        let want = [Mode::Installed, Mode::Updates];
        for mode in want {
            app.packages = vec![pkg("local", "b")];
            app.list_state.select(Some(0));
            frame(&mut app, KeyCode::Enter);
            let ok = if mode == Mode::Installed {
                matches!(app.popup, Popup::ConfirmRemove(_))
            } else {
                matches!(app.popup, Popup::Confirm(_))
            };
            assert!(ok, "{mode:?} row dialog");
            app.switch_mode();
            assert!(matches!(app.popup, Popup::None), "Tab also closes dialogs");
        }
    }

    #[test]
    fn applied_letters_from_list_jump_to_search_and_type() {
        let mut app = App::new("".into());
        app.mode = Mode::Installed;
        app.focus = Focus::List;
        // An unclaimed letter (char-no-mods catch-all, doc-hidden).
        frame(&mut app, KeyCode::Char('z'));
        assert_eq!(app.focus, Focus::Search);
        assert_eq!(app.input.value(), "z");
        // The mode-scoped 'o' row hit Installed's ToggleOrphans instead —
        // but in the Search list it is a plain letter again.
        let mut app2 = App::new("".into());
        app2.focus = Focus::List;
        frame(&mut app2, KeyCode::Char('o'));
        assert_eq!(app2.focus, Focus::Search);
        assert_eq!(app2.input.value(), "o");
        assert!(!app2.should_quit);
    }

    #[test]
    fn applied_q_and_ctrl_c_quit_only_where_declared() {
        // `q` while typing types — never quits.
        let mut app = App::new("".into());
        app.focus = Focus::Search;
        frame(&mut app, KeyCode::Char('q'));
        assert!(!app.should_quit);
        assert_eq!(app.input.value(), "q");
        // `q` from list focus quits.
        let mut app = App::new("".into());
        app.focus = Focus::List;
        frame(&mut app, KeyCode::Char('q'));
        assert!(app.should_quit);
        // `q` inside a popup closes the popup — no surprise quit.
        let mut app = App::new("".into());
        app.upgrade_warn = Some("warn".into());
        app.popup = Popup::ConfirmUpgrade;
        popup(&mut app, KeyCode::Char('q'));
        assert!(matches!(app.popup, Popup::None));
        assert!(!app.should_quit, "popup q is a close, not a quit");
        // Ctrl+C is the only anywhere-quit (Search focus this time).
        let mut app = App::new("".into());
        app.focus = Focus::Search;
        frame_ctrl(&mut app, KeyCode::Char('c'));
        assert!(app.should_quit);
    }

    #[test]
    fn applied_popup_rows_keep_the_v010_warn_semantics() {
        let mut app = App::new("".into());
        app.upgrade_warn = Some("manual".into());
        app.popup = Popup::ConfirmUpgrade;
        // Unclaimed keys are consumed but leave everything unchanged.
        popup(&mut app, KeyCode::Char('x'));
        assert!(matches!(app.popup, Popup::ConfirmUpgrade));
        assert_eq!(app.upgrade_warn.as_deref(), Some("manual"));
        // n/N closes plainly — v0.10 exact: the warn copy survives for the
        // NEXT dialog open (only Esc/q dismiss it).
        popup(&mut app, KeyCode::Char('n'));
        assert!(matches!(app.popup, Popup::None));
        assert_eq!(app.upgrade_warn.as_deref(), Some("manual"));
        // Esc/q closes AND drops the warn copy (ClosePopupDismiss).
        app.popup = Popup::ConfirmUpgrade;
        popup(&mut app, KeyCode::Esc);
        assert!(matches!(app.popup, Popup::None));
        assert!(app.upgrade_warn.is_none(), "warn dismissed with the dialog");
        // Frame arms never fired for any popup press.
        assert_eq!(app.mode, Mode::Search);
        assert!(app.settings.is_none());
    }

    #[test]
    fn applied_help_toggle_and_closables_are_exact() {
        // Help: `?` toggles it closed again (the only popup ? touches).
        let mut app = App::new("".into());
        app.focus = Focus::List;
        frame(&mut app, KeyCode::Char('?'));
        assert!(matches!(app.popup, Popup::Help));
        popup(&mut app, KeyCode::Char('?'));
        assert!(matches!(app.popup, Popup::None));
        // The toggling row's guard: any other popup swallows `?` whole.
        app.packages = vec![pkg("repo", "a")];
        app.list_state.select(Some(0));
        frame(&mut app, KeyCode::Char('i'));
        assert!(matches!(app.popup, Popup::Info(_)));
        popup(&mut app, KeyCode::Char('?'));
        assert!(matches!(app.popup, Popup::Info(_)), "? is a no-op on info");
        popup(&mut app, KeyCode::Enter);
        assert!(matches!(app.popup, Popup::None), "info is Enter-closable");
    }

    #[test]
    fn applied_frame_search_typing_flags_are_unconditional() {
        let mut app = App::new("".into());
        app.focus = Focus::Search;
        // `i` in search focus: the TypeForced branch — flags set even with
        // no change (the old arm's shape).
        frame(&mut app, KeyCode::Char('i'));
        assert_eq!(app.input.value(), "i");
        assert!(app.needs_search);
        // Up/Down in search focus: same unconditional flags, value untouched
        // (movement is cursor-level in tui-input).
        app.input = Input::default();
        app.needs_search = false;
        frame(&mut app, KeyCode::Up);
        assert!(app.needs_search, "Up sets the debounced-search flag");
        assert_eq!(app.input.value(), "");
        app.needs_search = false;
        frame(&mut app, KeyCode::Down);
        assert!(app.needs_search);
    }

    #[test]
    fn applied_remove_dialogs_open_from_installed_list() {
        let mut app = App::new("".into());
        app.mode = Mode::Installed;
        app.packages = vec![pkg("local", "libold")];
        app.list_state.select(Some(0));
        app.focus = Focus::List;
        for code in [
            KeyCode::Char('d'),
            KeyCode::Char('D'),
            KeyCode::Char('x'),
            KeyCode::Char('X'),
            KeyCode::Delete,
            KeyCode::Backspace,
        ] {
            frame(&mut app, code);
            assert!(matches!(app.popup, Popup::ConfirmRemove(_)), "{code:?}");
            // The letters never leak into the search (the remove row owns
            // them; focus and input stay put).
            assert_eq!(app.focus, Focus::List);
            assert_eq!(app.input.value(), "");
            app.popup = Popup::None;
        }
    }

    #[test]
    fn applied_mode_scoped_rows_drive_their_actions() {
        // `r`/`R` open the refresh confirm (state-only here; the run path
        // needs the terminal and is covered by the readonly studio below).
        let mut app = App::new("".into());
        app.focus = Focus::List;
        frame(&mut app, KeyCode::Char('r'));
        assert!(matches!(app.popup, Popup::ConfirmRefresh));
        // A key the popup layer doesn't claim would be eaten by the run
        // loop's popup handler (the frame sees nothing while a popup is
        // open) — drive it through the popup layer to pin the contract.
        assert!(
            app.handle_popup_event(&press_ctrl_key(KeyCode::Char('r')), None)
                .unwrap()
        );
        assert!(
            matches!(app.popup, Popup::ConfirmRefresh),
            "Ctrl+R is consumed by the popup layer"
        );
        // `c` in Installed scrolls readonly refusal (no paccache spawn).
        let mut app = App::new("".into());
        app.readonly = true;
        app.mode = Mode::Installed;
        app.focus = Focus::List;
        frame(&mut app, KeyCode::Char('c'));
        assert!(matches!(app.popup, Popup::Message(_)));
        // `o` toggles orphans-only round-trip (local pacman search path).
        let mut app = App::new("".into());
        app.mode = Mode::Installed;
        app.focus = Focus::List;
        frame(&mut app, KeyCode::Char('o'));
        assert!(app.orphans_only);
        frame(&mut app, KeyCode::Char('o'));
        assert!(!app.orphans_only);
        // `U` types in the Updates search box (list-scoped row would run
        // the live news fetch — driven resolver-only in keys.rs).
        let mut app = App::new("".into());
        app.mode = Mode::Updates;
        app.focus = Focus::Search;
        frame(&mut app, KeyCode::Char('U'));
        assert_eq!(app.input.value(), "U");
        assert!(!app.should_quit);
    }

    #[test]
    fn applied_enter_in_search_submits_and_returns_to_list() {
        let mut app = App::new("".into());
        app.focus = Focus::Search;
        // Empty input: the trigger clears instantly (hermetic).
        frame(&mut app, KeyCode::Enter);
        assert_eq!(app.focus, Focus::List);
        assert!(!app.is_loading);
    }

    #[test]
    fn applied_popup_closed_reports_not_consumed() {
        let mut app = App::new("".into());
        assert!(
            !app.handle_popup_event(&press_key(KeyCode::Char('a')), None)
                .unwrap()
        );
        assert!(matches!(app.popup, Popup::None));
    }

    #[test]
    fn applied_modal_unwinds_one_box_at_a_time() {
        let mut app = App::new("".into());
        app.focus = Focus::List;
        frame(&mut app, KeyCode::Char('S'));
        assert!(app.settings.is_some());
        // '/' focuses the filter box.
        modal(&mut app, KeyCode::Char('/'));
        let m = app.settings.as_ref().unwrap();
        assert!(m.filter_focus);
        let s0 = m.selection;
        // Arrows still move the selection while the filter is focused
        // (the old `!editing` guard — not part of the box capture).
        modal(&mut app, KeyCode::Char('j'));
        assert_eq!(app.settings.as_ref().unwrap().selection, s0 + 1);
        // Typing goes to the filter only.
        modal(&mut app, KeyCode::Char('x'));
        let m = app.settings.as_ref().unwrap();
        assert_eq!(m.filter.value(), "x");
        assert!(m.filter_focus);
        // Enter leaves the box, keeping the filter text.
        modal(&mut app, KeyCode::Enter);
        let m = app.settings.as_ref().unwrap();
        assert!(!m.filter_focus);
        assert_eq!(m.filter.value(), "x");
        // At the plain level Esc closes the WHOLE modal (the box already
        // exited) — one layer per press.
        modal(&mut app, KeyCode::Esc);
        assert!(app.settings.is_none());
        assert_eq!(app.focus, Focus::List);
        // Esc *from inside* the box unwinds only the box: filter cleared,
        // modal stays.
        frame(&mut app, KeyCode::Char('S'));
        modal(&mut app, KeyCode::Char('/'));
        modal(&mut app, KeyCode::Char('x'));
        modal(&mut app, KeyCode::Esc);
        let m = app.settings.as_ref().unwrap();
        assert!(m.filter.value().is_empty(), "filter cleared, box only");
        assert!(!m.filter_focus);
        // Esc again closes the modal; the frame Esc toggle never fired.
        modal(&mut app, KeyCode::Esc);
        assert!(app.settings.is_none());
        assert_eq!(app.focus, Focus::List);
        // `S` also toggles the modal closed from its list level: re-open
        // through the frame row, close through the modal row.
        frame(&mut app, KeyCode::Char('S'));
        assert!(app.settings.is_some());
        modal(&mut app, KeyCode::Char('S'));
        assert!(app.settings.is_none());
    }

    #[test]
    fn applied_editor_typing_guard_swallows_strays() {
        let mut app = App::new("".into());
        app.open_settings();
        // A non-bool, non-enum row owns a capture box (tui.poll_ms = U64).
        assert!(app.settings.as_mut().unwrap().select_key("tui.poll_ms"));
        modal(&mut app, KeyCode::Enter);
        assert!(app.settings.as_ref().unwrap().editor.is_some());
        let before = app
            .settings
            .as_ref()
            .unwrap()
            .editor
            .as_ref()
            .unwrap()
            .buf
            .value()
            .to_string();
        // Strays type into the box; the search input and modal state hold.
        modal(&mut app, KeyCode::Char('a'));
        modal(&mut app, KeyCode::Char('q'));
        let m = app.settings.as_ref().unwrap();
        assert!(m.editor.is_some());
        let value = m.editor.as_ref().unwrap().buf.value().to_string();
        assert_eq!(value, format!("{before}aq"), "strays typed into the box");
        assert!(app.input.value().is_empty(), "the modal typing guard won");
        assert!(!app.should_quit);
        // Esc unwinds exactly one box: editor closes, modal (then frame) stay.
        modal(&mut app, KeyCode::Esc);
        let m = app.settings.as_ref().unwrap();
        assert!(m.editor.is_none());
        // And one more Esc closes the modal itself.
        modal(&mut app, KeyCode::Esc);
        assert!(app.settings.is_none());
    }

    #[test]
    fn applied_bool_rows_space_toggle_and_r_reset() {
        let mut app = App::new("".into());
        app.open_settings();
        assert!(app.settings.as_mut().unwrap().select_key("theme.no_color"));
        // Enter on a bool is inert (bools are Space-only per plan §4)...
        modal(&mut app, KeyCode::Enter);
        let m = app.settings.as_ref().unwrap();
        assert!(m.editor.is_none());
        let row = m
            .rows
            .iter()
            .find(|r| r.meta.key == "theme.no_color")
            .unwrap();
        assert!(row.draft.is_none());
        let stored = row.stored.clone();
        // ...Space flips it...
        modal(&mut app, KeyCode::Char(' '));
        let m = app.settings.as_ref().unwrap();
        let row = m
            .rows
            .iter()
            .find(|r| r.meta.key == "theme.no_color")
            .unwrap();
        assert_ne!(row.draft.as_deref(), Some(stored.as_str()), "flipped");
        assert!(row.draft.is_some());
        // ...and `r` resets to the opened-at value.
        modal(&mut app, KeyCode::Char('r'));
        let m = app.settings.as_ref().unwrap();
        let row = m
            .rows
            .iter()
            .find(|r| r.meta.key == "theme.no_color")
            .unwrap();
        assert!(row.draft.is_none(), "reset cleared the draft");
    }
}
