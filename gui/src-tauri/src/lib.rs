//! pakmap GUI backend: Tauri commands over the pakmap core library.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
//!
//! AC phase: staged streaming search mirroring the TUI orchestrator
//! (`src/tui/app.rs` — per-leg threads, sequential joins, generation guard),
//! full-field package DTO, detail resolution for installed packages. The IPC
//! contract source of truth is `docs/gui-architecture.md`.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use pakmap::model::Package;
use serde::{Deserialize, Serialize};
use tauri::State;
use tauri::ipc::Channel;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager};

mod ops;
mod pkgbuild;

/// Global state: the `active_search` generation guard — same semantics as the
/// TUI (`App::active_search`): every new search supersedes in-flight legs
/// incrementally, never silently mixing generations in the UI.
pub struct AppState {
    active_search: Arc<AtomicU64>,
    ops: ops::OpsState,
}

#[derive(Serialize)]
pub struct ConfigSummary {
    /// GUI package version
    pub app_version: &'static str,
    /// Configured default result limit (`search.limit`)
    pub limit: usize,
    /// Configured default source precedence (`search.source`)
    pub source: String,
    /// Configured search timeout seconds (`search.timeout_secs`)
    pub timeout_secs: u64,
    /// `behavior.flatpak_enabled`
    pub flatpak_enabled: bool,
    /// `behavior.aur_rpc` overridden (custom mirror)
    pub custom_aur_rpc: bool,
    /// `tui.debounce_ms` clamped the way the TUI clamps it (50..=5000)
    pub debounce_ms: u64,
}

/// Full-field package DTO — every `Package` field the detail pane needs, plus
/// the list essentials. Field names mirror the Rust struct 1:1 (serde).
#[derive(Serialize, Clone)]
pub struct PackageDto {
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub repo: String,
    pub arch: Option<String>,
    pub url: Option<String>,
    pub installed: bool,
    pub votes: Option<i32>,
    pub popularity: Option<f64>,
    pub out_of_date: Option<i64>,
    pub maintainer: Option<String>,
    pub last_modified: Option<i64>,
    pub depends: Option<Vec<String>>,
    pub optdepends: Option<Vec<String>>,
    pub required_by: Option<Vec<String>>,
    pub optional_for: Option<Vec<String>>,
    pub reason: Option<String>,
    pub orphan: bool,
}

impl From<&Package> for PackageDto {
    fn from(p: &Package) -> Self {
        Self {
            name: p.name.clone(),
            version: p.version.clone(),
            description: p.description.clone(),
            repo: p.repo.clone(),
            arch: p.arch.clone(),
            url: p.url.clone(),
            installed: p.installed,
            votes: p.votes,
            popularity: p.popularity,
            out_of_date: p.out_of_date,
            maintainer: p.maintainer.clone(),
            last_modified: p.last_modified,
            depends: p.depends.clone(),
            optdepends: p.optdepends.clone(),
            required_by: p.required_by.clone(),
            optional_for: p.optional_for.clone(),
            reason: p.reason.clone(),
            orphan: p.orphan,
        }
    }
}

impl From<Package> for PackageDto {
    fn from(p: Package) -> Self {
        Self::from(&p)
    }
}

/// IPC contract: `{ code, message, hint }` — mirrors the CLI's stderr hints
/// so the frontend never invents its own copy of recovery text.
#[derive(Debug, Serialize, Clone)]
pub struct GuiError {
    pub code: String,
    pub message: String,
    pub hint: Option<String>,
}

fn gui_error(code: &str, message: String, hint: Option<String>) -> GuiError {
    GuiError {
        code: code.to_string(),
        message,
        hint,
    }
}

/// Mirror of `cli::Source` as a wire enum ("all" | "repo" | "aur" | "flatpak").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SourceKind {
    All,
    Repo,
    Aur,
    Flatpak,
}

impl SourceKind {
    fn parse(s: &str) -> Self {
        match s {
            "repo" => Self::Repo,
            "aur" => Self::Aur,
            "flatpak" => Self::Flatpak,
            _ => Self::All,
        }
    }
}

#[derive(Deserialize, Clone)]
pub struct SearchFilters {
    /// "all" | "repo" | "aur" | "flatpak" (missing / unknown → all)
    pub source: Option<String>,
    /// AUR RPC `by=` value (defaults to the lib's "name-desc")
    pub aur_by: Option<String>,
    pub use_regex: Option<bool>,
    pub installed_only: Option<bool>,
    pub orphans_only: Option<bool>,
    /// Overrides `config.search.limit`
    pub limit: Option<usize>,
}

#[derive(Serialize, Clone)]
pub struct SearchEvent {
    pub generation: u64,
    /// "repo" | "aur" | "flatpak" | "done" — mirrors `tui::SearchStage`
    pub stage: String,
    pub packages: Vec<PackageDto>,
}

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {name}! You've been greeted from pakmap-gui!")
}

/// Resolved-effective config summary (user file with fail-open defaults).
#[tauri::command]
fn get_config_summary() -> ConfigSummary {
    let cfg = pakmap::config::Config::load();
    ConfigSummary {
        app_version: env!("CARGO_PKG_VERSION"),
        limit: cfg.search.limit,
        source: cfg.search.source.clone(),
        timeout_secs: cfg.search.timeout_secs,
        flatpak_enabled: cfg.behavior.flatpak_enabled,
        custom_aur_rpc: cfg.behavior.aur_rpc.is_some(),
        debounce_ms: cfg.tui.debounce_ms.clamp(50, 5000),
    }
}

#[tauri::command]
fn cancel_search(state: State<'_, AppState>) -> u64 {
    state.active_search.fetch_add(1, Ordering::Relaxed) + 1
}

// ---------------------------------------------------------------------------
// AD operations surface
// ---------------------------------------------------------------------------

/// Wire shape of `get_updates`: both legs degrade independently — an error in
/// one leg is an explicit field, never a global command failure (resilience:
/// a broken AUR mirror must not hide repo updates). Orphan names ride along
/// because the Updates screen's orphans tool needs the same localdb scan.
#[derive(Serialize)]
pub struct UpdatesResult {
    pub repo: Vec<pakmap::updates::PkgUpdate>,
    pub aur: Vec<pakmap::updates::PkgUpdate>,
    pub repo_err: Option<String>,
    pub aur_err: Option<String>,
    pub orphans: Vec<String>,
}

#[derive(Serialize)]
pub struct ConfigFlags {
    /// `behavior.readonly` — the frontend disables all mutating actions
    pub readonly: bool,
    pub flatpak_enabled: bool,
}

#[tauri::command]
fn run_op(
    op: ops::OpRequest,
    on_event: Channel<ops::OpMessage>,
    state: State<'_, AppState>,
) -> u64 {
    ops::dispatch_op(op, on_event, &state.ops)
}

/// Phase-boundary cancellation. Returns false for unknown/finished ops.
#[tauri::command]
fn cancel_operation(op_id: u64, state: State<'_, AppState>) -> bool {
    state.ops.cancel(op_id)
}

/// Free the cancel-flag entry after the UI consumed the terminal event.
#[tauri::command]
fn drop_operation(op_id: u64, state: State<'_, AppState>) {
    ops::drop_op(&state.ops, op_id);
}

#[tauri::command]
fn get_config_flags(guest: State<'_, SmokeGuest>) -> ConfigFlags {
    // Set BEFORE any work: the flag proves "webview runs app JS", which is
    // all the smoke harness needs — a config-load hiccup must not turn a
    // booted webview into a misleading smoke exit 3.
    guest.0.store(true, Ordering::Relaxed);
    let cfg = pakmap::config::Config::load();
    ConfigFlags {
        readonly: cfg.is_readonly(),
        flatpak_enabled: cfg.behavior.flatpak_enabled,
    }
}

#[tauri::command]
fn get_updates(app: tauri::AppHandle) -> UpdatesResult {
    let cfg = pakmap::config::Config::load();
    let (repo, repo_err) = match pakmap::updates::check_repo_updates() {
        Ok(v) => (v, None),
        Err(e) => (Vec::new(), Some(format!("{e:#}"))),
    };
    let aur_report = pakmap::updates::check_aur_updates_blocking_report(&cfg);
    let aur = aur_report.updates;
    let orphans = pakmap::orphans::list_orphans()
        .or_else(|_| pakmap::orphans::list_orphans_fallback())
        .unwrap_or_default()
        .into_iter()
        .map(|p| p.name)
        .collect();
    // Event-driven tray badge (v0.9): the tooltip refresh rides on data this
    // command computed anyway — zero extra network, no TTL thread. The memo's
    // staleness is the pre-existing 5-min semantics, not a new check.
    refresh_tray_tooltip(&app, repo.len() + aur.len());
    UpdatesResult {
        repo,
        aur,
        repo_err,
        // B5 (v0.9.1): the lib's cached report now carries the failure reason
        // (fail-open contract unchanged — updates stay best-effort).
        aur_err: aur_report.aur_err,
        orphans,
    }
}

#[tauri::command]
fn get_news() -> Vec<pakmap::news::NewsItem> {
    let cfg = pakmap::config::Config::load();
    pakmap::news::fetch_arch_news(&cfg)
}

// ---------------------------------------------------------------------------
// v0.9: package-URL opener (DetailPane homepage field)
// ---------------------------------------------------------------------------

/// **Standing deviation from the plan (§2.1, recorded in the plan file):** no
/// *host* allowlist. A allowlist would reject every legitimate upstream
/// homepage ( mozilla.org …) that untrusted AUR metadata legitimately carries.
/// The security boundary is the scheme gate + browser isolation instead: only
/// https pages (http allowed for loopback dev targets) are handed to the OS
/// browser, which is itself a separate process — the webview never loads any
/// of this. Everything still validates in Rust: the frontend has no bypass,
/// embedded credentials and control characters are rejected outright, and
/// opens are audit-logged (host only) for post-incident review.
pub(crate) const MAX_URL_BYTES: usize = 2048;

fn invalid_url_error(message: String) -> GuiError {
    gui_error(
        "invalid_url",
        message,
        Some("copy the URL and open it in your browser instead".to_string()),
    )
}

/// Loopback-only exception for http (dev servers on package pages).
fn is_loopback_host(host: &str) -> bool {
    matches!(
        host.to_ascii_lowercase().as_str(),
        "localhost" | "::1" | "[::1]"
    ) || host.starts_with("127.")
}

/// Validate a package-provided URL before handing it to the OS browser.
/// Returns the host on success (audit logging uses the host, never the path
/// or query — those can carry per-user tracking).
pub(crate) fn validate_package_url(raw: &str) -> Result<String, GuiError> {
    if raw.is_empty() {
        return Err(invalid_url_error("empty URL".to_string()));
    }
    if raw.len() > MAX_URL_BYTES {
        return Err(invalid_url_error(format!(
            "URL too long ({MAX_URL_BYTES} byte cap)"
        )));
    }
    // The WHATWG parser strips TAB/CR/LF — reject them up front so no scheme
    // injection can survive the round-trip.
    if raw.chars().any(|c| c.is_control()) {
        return Err(invalid_url_error(
            "URL contains control characters".to_string(),
        ));
    }
    let parsed =
        url::Url::parse(raw).map_err(|_| invalid_url_error("not a parseable URL".to_string()))?;
    match parsed.scheme() {
        "https" => {}
        "http" if is_loopback_host(parsed.host_str().unwrap_or_default()) => {}
        _ => {
            return Err(invalid_url_error(format!(
                "scheme '{}' is not allowed — only https pages are opened",
                parsed.scheme()
            )));
        }
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(invalid_url_error(
            "URL contains embedded credentials".to_string(),
        ));
    }
    let host = parsed
        .host_str()
        .filter(|h| !h.is_empty())
        .ok_or_else(|| invalid_url_error("URL has no host".to_string()))?;
    Ok(host.to_string())
}

/// Open a package homepage in the system browser, after Rust-side validation.
/// The capability model does not change (`opener:allow-open-url` stays); this
/// command adds the trust boundary where package metadata is concerned —
/// news links keep their direct `openExternal` path (AF-tested).
#[tauri::command]
fn open_package_url(name: &str, url: &str) -> Result<(), GuiError> {
    if name.trim().is_empty() {
        return Err(gui_error(
            "invalid_name",
            "empty package name".to_string(),
            None,
        ));
    }
    let host = validate_package_url(url)?;
    tracing::info!(target: "pakmap_gui::opener", name = %name.trim(), host = %host, "opening package URL");
    tauri_plugin_opener::open_url(url, None::<&str>).map_err(|e| {
        gui_error(
            "open_failed",
            format!("failed to open in the system browser: {e}"),
            Some("copy the URL and open it manually".to_string()),
        )
    })?;
    Ok(())
}

// ---------------------------------------------------------------------------
// AE surface
// ---------------------------------------------------------------------------

/// Status-bar counters for the Updates tab header (TUI `--stats` parity).
/// Local-only + the AUR memo (5 min) — no extra network chatty.
#[derive(Serialize)]
pub struct GuiStats {
    pub explicit: usize,
    pub dependencies: usize,
    pub orphans: usize,
    pub updates_repo: usize,
    pub updates_aur: usize,
    pub updates_flatpak: usize,
    pub flatpak_installed: usize,
    pub cache_pacman_bytes: u64,
    pub cache_pakmap_bytes: u64,
    /// pre-formatted via the lib's `human_bytes` (no float noise ported)
    pub cache_pacman_fmt: String,
    pub cache_pakmap_fmt: String,
    /// B6 (v0.9.1): wall-clock epoch secs of the last executed AUR check
    /// backing `updates_aur`; `None` = never checked (chip hidden).
    pub aur_checked_at: Option<u64>,
}

#[tauri::command]
fn get_stats(app: tauri::AppHandle) -> GuiStats {
    let cfg = pakmap::config::Config::load();
    let mut s = pakmap::stats::gather_local_stats();
    // B6 (v0.9.1): the same report feeds the count and the "checked Xm ago"
    // timestamp — the chip states how stale the number may be (≤5-min memo).
    let aur_report = pakmap::updates::check_aur_updates_blocking_report(&cfg);
    s.updates_aur = aur_report.updates.len();
    s.flatpak_installed = pakmap::sources::flatpak::list_installed_cached()
        .map(|v| v.len())
        .unwrap_or(0);
    // Event-driven tray badge refresh, same contract as get_updates.
    refresh_tray_tooltip(&app, s.updates_repo + s.updates_aur + s.updates_flatpak);
    GuiStats {
        explicit: s.explicit,
        dependencies: s.dependencies,
        orphans: s.orphans,
        updates_repo: s.updates_repo,
        updates_aur: s.updates_aur,
        updates_flatpak: s.updates_flatpak,
        flatpak_installed: s.flatpak_installed,
        cache_pacman_bytes: s.cache_pacman_bytes,
        cache_pakmap_bytes: s.cache_pakmap_bytes,
        cache_pacman_fmt: pakmap::stats::human_bytes(s.cache_pacman_bytes),
        cache_pakmap_fmt: pakmap::stats::human_bytes(s.cache_pakmap_bytes),
        aur_checked_at: aur_report.checked_at_epoch,
    }
}

/// Detail resolution for a selected row: for installed packages this resolves
/// the dependency/reason metadata through the localdb fast path; remote rows
/// carry their info in the streamed `Package` already (votes, popularity,
/// maintainer, ood) so the frontend passes those straight to the pane.
///
/// B4 fix (v0.9): search failure and "not found" are distinct conclusions —
/// an unreadable localdb (`localdb_unreadable`) propagates with the lib error
/// instead of masquerading as an empty result, while a clean-but-empty localdb
/// still maps to `package_not_found` (the row simply is not installed).
#[tauri::command]
fn package_info(name: &str) -> Result<PackageDto, GuiError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(gui_error("empty_query", "empty query".to_string(), None));
    }
    let found = pakmap::search::repo::search_local(name, 1, false)
        .or_else(|_| pakmap::search::repo::search_local_fallback(name, 1, false));
    let found = found.map_err(|e| {
        gui_error(
            "localdb_unreadable",
            format!("could not read the local pacman database: {e:#}"),
            Some("check that pacman's db (/var/lib/pacman) is readable — remote search rows still carry their metadata in the results list".to_string()),
        )
    })?;
    found
        .first()
        .map(PackageDto::from)
        .ok_or_else(|| {
            gui_error(
                "package_not_found",
                format!("installed package '{name}' not found in localdb"),
                Some("the package may not be installed — check the search results row for remote metadata".to_string()),
            )
        })
}

/// Worker mirroring `tui::App::trigger_search`: one thread per enabled leg,
/// sequential joins, staged sends behind the generation guard, then `done`.
///
/// TUI semantics preserved exactly:
/// - installed/orphans-only queries become a LOCAL search (search_local +
///   orphans intersection via `list_orphans`), carried by a payload `done`
///   event like the TUI's carry-payload `Done`;
/// - AUR/flatpak legs are skipped when `installed_only` is set; flatpak also
///   requires `config.flatpak_enabled()` (fail-open empty when missing);
/// - a one-shot AUR warm (HEAD) overlaps the repo leg of the same query.
fn spawn_search(
    query: String,
    filters: SearchFilters,
    generation: u64,
    active: Arc<AtomicU64>,
    channel: Channel<SearchEvent>,
) {
    std::thread::spawn(move || {
        let cfg = pakmap::config::Config::load();
        let src = SourceKind::parse(filters.source.as_deref().unwrap_or("all"));
        let use_regex = filters.use_regex.unwrap_or(false);
        let installed_only = filters.installed_only.unwrap_or(false);
        let orphans_only = filters.orphans_only.unwrap_or(false);
        let limit = filters.limit.unwrap_or(50);
        let aur_by = filters.aur_by.unwrap_or_else(|| "name-desc".to_string());

        let send = |stage: &str, packages: Vec<Package>| {
            if active.load(Ordering::Relaxed) == generation {
                let _ = channel.send(SearchEvent {
                    generation,
                    stage: stage.to_string(),
                    packages: packages.iter().map(PackageDto::from).collect(),
                });
            }
        };

        // Local path: installed-only and orphans-only are single local legs
        // (mirrors the TUI installed-mode branch, orphans intersect + enrich).
        if installed_only || orphans_only {
            let mut res = pakmap::search::repo::search_local(&query, limit, use_regex)
                .or_else(|_| pakmap::search::repo::search_local_fallback(&query, limit, use_regex))
                .unwrap_or_default();
            if orphans_only {
                let orphans = pakmap::orphans::list_orphans()
                    .or_else(|_| pakmap::orphans::list_orphans_fallback())
                    .unwrap_or_default();
                let names: std::collections::HashSet<String> =
                    orphans.into_iter().map(|p| p.name).collect();
                res.retain(|p| names.contains(&p.name));
                // Enrich orphan flag for badges (same as the TUI).
                for p in &mut res {
                    p.orphan = true;
                    p.reason = Some("dependency".to_string());
                }
            }
            if active.load(Ordering::Relaxed) == generation {
                let _ = channel.send(SearchEvent {
                    generation,
                    stage: "done".to_string(),
                    packages: res.iter().map(PackageDto::from).collect(),
                });
            }
            return;
        }

        // One-shot RPC warm (P3) on its own thread — overlaps the repo leg.
        {
            let cfg_w = cfg.clone();
            std::thread::spawn(move || pakmap::search::aur::warm_blocking_aur(&cfg_w));
        }

        let q1 = query.clone();
        let repo_handle = if matches!(src, SourceKind::Aur | SourceKind::Flatpak) {
            None
        } else {
            Some(std::thread::spawn(move || {
                let timer = pakmap::timing::start("gui-repo-search");
                let out: Vec<Package> =
                    pakmap::search::repo::search_repo(&q1, limit, use_regex, false)
                        .or_else(|_| pakmap::search::repo::search_repo_fallback(&q1, limit))
                        .unwrap_or_default();
                timer.finish(out.len());
                out
            }))
        };
        let q2 = query.clone();
        let cfg2 = cfg.clone();
        let aur_by_c = aur_by.clone();
        let aur_handle = if matches!(src, SourceKind::Repo | SourceKind::Flatpak) {
            None
        } else {
            Some(std::thread::spawn(move || {
                let timer = pakmap::timing::start("gui-aur-search");
                let out: Vec<Package> = pakmap::search::aur::search_aur_blocking_with_config(
                    &q2, &aur_by_c, limit, use_regex, &cfg2,
                )
                .unwrap_or_default();
                timer.finish(out.len());
                out
            }))
        };
        let q3 = query.clone();
        let cfg3 = cfg.clone();
        let flatpak_handle =
            if matches!(src, SourceKind::Repo | SourceKind::Aur) || !cfg.flatpak_enabled() {
                None
            } else {
                Some(std::thread::spawn(move || {
                    let timer = pakmap::timing::start("gui-flatpak-search");
                    let out: Vec<Package> =
                        pakmap::sources::search_flatpak_with_config(&q3, limit, use_regex, &cfg3)
                            .unwrap_or_default();
                    timer.finish(out.len());
                    out
                }))
            };

        // Sequential joins, staged sends behind the guard (TUI mirrors this;
        // a panicked worker yields an empty leg — TUI's warn+empty behaviour).
        let repo_res: Vec<Package> = repo_handle.and_then(|h| h.join().ok()).unwrap_or_default();
        let aur_res: Vec<Package> = aur_handle.and_then(|h| h.join().ok()).unwrap_or_default();
        let flatpak_res: Vec<Package> = flatpak_handle
            .and_then(|h| h.join().ok())
            .unwrap_or_default();

        // Empty query is rejected at the command boundary; empty results are
        // legitimate (send the legs so totals tick to 0).
        send("repo", repo_res);
        send("aur", aur_res);
        send("flatpak", flatpak_res);
        send("done", Vec::new());
    });
}

/// Kick off a staged search: returns the generation immediately; results
/// stream through the typed `Channel` (one per operation, per the contract).
#[tauri::command]
fn search(
    query: &str,
    filters: SearchFilters,
    on_event: Channel<SearchEvent>,
    state: State<'_, AppState>,
) -> Result<u64, GuiError> {
    let query = query.trim();
    if query.is_empty() {
        return Err(gui_error(
            "empty_query",
            "empty query".to_string(),
            Some("type a package name — Enter searches immediately".to_string()),
        ));
    }
    let generation = state.active_search.fetch_add(1, Ordering::Relaxed) + 1;
    spawn_search(
        query.to_string(),
        filters,
        generation,
        state.active_search.clone(),
        on_event,
    );
    Ok(generation)
}

// ---------------------------------------------------------------------------
// v0.9: system tray — passive updates awareness
// ---------------------------------------------------------------------------

/// Tray identity for `tray_by_id` lookups (tooltip refresh from commands).
const TRAY_ID: &str = "pakmap-tray";

/// Tray menu — deliberately minimal (no "install from tray" shortcuts): show
/// the window, jump to the Updates tab, quit. The actual upgrade always flows
/// through the GUI's confirm dialog.
fn setup_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "tray-show", "Show pakmap", true, None::<&str>)?;
    let updates = MenuItem::with_id(app, "tray-updates", "Check for updates", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "tray-quit", "Quit pakmap", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &show,
            &PredefinedMenuItem::separator(app)?,
            &updates,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;

    let mut tray = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .tooltip("pakmap")
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "tray-show" => show_main_window(app),
            "tray-updates" => {
                show_main_window(app);
                // The real refresh is delegated to the frontend: switching to
                // the Updates tab re-mounts UpdatesScreen, which fetches.
                let _ = app.emit("tray:check-updates", ());
            }
            "tray-quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        });
    // Icon is optional — some environments report no default icon; the tray
    // still works (a blank icon is better than refusing passive awareness).
    if let Some(icon) = app.default_window_icon().cloned() {
        tray = tray.icon(icon.to_owned());
    }
    // tray-icon's appindicator backend loads libayatana-appindicator3 lazily
    // during build — and the dynamic loader `panic!`s instead of erroring when
    // the library is absent (v0.9.0 release-run canary: exit 101 in a bare
    // Arch container; every lib-less desktop would have crashed on boot).
    // The loader panics in pure Rust frames (before any C state we own), so
    // the unwind is caught here and the failure is logged-and-absorbed: the
    // app degrades to tray-less (same contract as the SNI-less graceful fail
    // at the call site).
    match catch_unwind(AssertUnwindSafe(|| tray.build(app))) {
        // Fallible (non-panic) build error — the call site logs and continues.
        Ok(build) => {
            build?;
        }
        Err(payload) => {
            let reason = payload
                .downcast_ref::<&str>()
                .map(|s| (*s).to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "appindicator backend failed to load".to_string());
            tracing::warn!(
                "system tray unavailable: {reason} — running without the passive update badge"
            );
        }
    }
    Ok(())
}

/// Raise the main window from tray interactions (click / "Show pakmap").
fn show_main_window(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// Event-driven badge refresh: called from `get_updates`/`get_stats` with the
/// counts those commands computed anyway. No polling thread, no network from
/// the tray — the memo TTL (5 min) stays the only refresh source. No-op when
/// the tray was refused (e.g. headless/SNI-less sessions).
fn refresh_tray_tooltip(app: &tauri::AppHandle, updates: usize) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let text = match updates {
            0 => "pakmap — no updates".to_string(),
            1 => "pakmap — 1 update available".to_string(),
            n => format!("pakmap — {n} updates available"),
        };
        let _ = tray.set_tooltip(Some(text));
    }
}

/// Spike smoke hook: `PAKMAP_GUI_SMOKE_MS=<ms>` auto-quits after the delay
/// so release artifacts can be launched in scripts/xvfb CI without a human
/// closing the window. Same pattern as `PAKMAP_SKIP_LIVE`. The quit goes
/// through `AppHandle::exit` (graceful run-loop shutdown) — a bare
/// `std::process::exit` from the timer thread races the webkit atexit path
/// and can segfault, which would fail the smoke's exit-code contract.
///
/// Exit codes: **0** = the timer fired AND the frontend actually issued IPC
/// (the shell's mount-time `get_config_flags` marks the guest), **3** = the
/// timer fired but NO IPC ever arrived — that is a webview stuck on a
/// browser error page (e.g. a release build without the `custom-protocol`
/// feature pointing at the missing dev server), not a healthy app.
fn arm_smoke_timer(app: tauri::AppHandle, ms: u64, guest: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(ms));
        if guest.load(Ordering::Relaxed) {
            app.exit(0);
        } else {
            eprintln!(
                "SMOKE: frontend never issued IPC within {ms}ms — webview did not \
                 load the app (embedded assets missing? release build without the \
                 tauri/custom-protocol feature?)"
            );
            app.exit(3);
        }
    });
}

fn build_smoke_trigger() -> Option<u64> {
    std::env::var("PAKMAP_GUI_SMOKE_MS")
        .ok()
        .and_then(|ms| ms.parse::<u64>().ok())
}

/// Set by the shell's first IPC call (`get_config_flags`, which the React
/// shell issues on mount): any successful round-trip proves the webview is
/// running the app's JS, not displaying a browser error page. Owned by the
/// smoke harness — the managed copy and the timer thread share the same
/// allocation. Cheap; ignored entirely in normal (non-smoke) runs.
#[derive(Debug, Default)]
struct SmokeGuest(Arc<AtomicBool>);

/// App entry: no unwrap/expect — a failed startup prints and exits 1
/// (the run loop's error is terminal anyway).
pub fn run() {
    // Audit/observability channel (opener + pkgbuild logging): opt-in via
    // RUST_LOG, everything else stays silent on stdout like before.
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .try_init();
    let smoke = build_smoke_trigger();
    let guest = Arc::new(AtomicBool::new(false));
    let guest_for_timer = guest.clone();
    if let Err(e) = tauri::Builder::default()
        // AF: real link opening (news → system browser via xdg-open); the
        // capability grants only `opener:allow-open-url`, not paths.
        .plugin(tauri_plugin_opener::init())
        .setup(move |app| {
            if let Some(ms) = smoke {
                arm_smoke_timer(app.handle().clone(), ms, guest_for_timer);
            }
            // v0.9 tray: graceful fail — a missing SNI host (Wayland without
            // StatusNotifier, containers, headless) or an absent
            // libayatana-appindicator (caught inside setup_tray) must not kill
            // the app.
            if let Err(e) = setup_tray(app.handle()) {
                tracing::warn!(
                    "system tray unavailable: {e} — running without the passive update badge"
                );
            }
            Ok(())
        })
        .manage(AppState {
            active_search: Arc::new(AtomicU64::new(0)),
            ops: ops::OpsState::new(),
        })
        .manage(SmokeGuest(guest))
        .invoke_handler(tauri::generate_handler![
            greet,
            get_config_summary,
            search,
            cancel_search,
            package_info,
            run_op,
            cancel_operation,
            drop_operation,
            get_config_flags,
            get_updates,
            get_news,
            get_stats,
            pkgbuild::get_pkgbuild,
            open_package_url
        ])
        .run(tauri::generate_context!())
    {
        eprintln!("pakmap-gui exited: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_package() -> Package {
        let mut p = Package::minimal(
            "firefox".to_string(),
            "139.0-1".to_string(),
            "extra".to_string(),
        );
        p.description = Some("web browser".to_string());
        p.installed = true;
        p.votes = Some(42);
        p.popularity = Some(1.25);
        p.out_of_date = Some(1000);
        p.maintainer = Some("someone".to_string());
        p.last_modified = Some(999);
        p.depends = Some(vec!["gtk3".to_string()]);
        p.optdepends = Some(vec!["ffmpeg".to_string()]);
        p.required_by = Some(vec!["xulrunner".to_string()]);
        p.optional_for = Some(vec!["other".to_string()]);
        p.reason = Some("explicit".to_string());
        p.orphan = false;
        p
    }

    #[test]
    fn dto_maps_every_field() {
        let dto = PackageDto::from(sample_package());
        assert_eq!(dto.name, "firefox");
        assert_eq!(dto.version, "139.0-1");
        assert_eq!(dto.repo, "extra");
        assert_eq!(dto.description.as_deref(), Some("web browser"));
        assert!(dto.installed);
        assert_eq!(dto.votes, Some(42));
        assert_eq!(dto.popularity, Some(1.25));
        assert!(dto.out_of_date.is_some());
        assert_eq!(dto.maintainer.as_deref(), Some("someone"));
        assert_eq!(
            dto.depends.as_deref(),
            Some(["gtk3".to_string()].as_slice())
        );
        assert_eq!(
            dto.required_by.as_deref(),
            Some(["xulrunner".to_string()].as_slice())
        );
        assert_eq!(dto.reason.as_deref(), Some("explicit"));
    }

    #[test]
    fn search_event_stage_names_mirror_tui() {
        let ev = SearchEvent {
            generation: 7,
            stage: "repo".to_string(),
            packages: vec![PackageDto::from(sample_package())],
        };
        assert_eq!(ev.stage, "repo");
        let done = SearchEvent {
            generation: 7,
            stage: "done".to_string(),
            packages: Vec::new(),
        };
        assert_eq!(done.stage, "done");
        assert!(done.packages.is_empty());
    }

    #[test]
    fn source_parse_fail_open_to_all() {
        assert_eq!(SourceKind::parse("aur"), SourceKind::Aur);
        assert_eq!(SourceKind::parse("nonsense"), SourceKind::All);
        assert_eq!(SourceKind::parse(""), SourceKind::All);
    }

    // --- v0.9 package-URL opener validation matrix --------------------------

    #[test]
    fn package_url_accepts_https_and_loopback_http() {
        assert_eq!(
            validate_package_url("https://aur.archlinux.org/packages/yay").unwrap(),
            "aur.archlinux.org"
        );
        assert_eq!(
            validate_package_url("https://github.com/kenjaku-dev/pakmap").unwrap(),
            "github.com"
        );
        assert_eq!(
            validate_package_url("https://mozilla.org/?query=a/b#c").unwrap(),
            "mozilla.org"
        );
        // http survives only for loopback dev targets.
        assert_eq!(
            validate_package_url("http://localhost:8080/pkg").unwrap(),
            "localhost"
        );
        assert_eq!(
            validate_package_url("http://127.0.0.1:3000/").unwrap(),
            "127.0.0.1"
        );
    }

    #[test]
    fn package_url_rejects_bad_schemes_targets() {
        for bad in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "javascript:%20void(0)",
            "data:text/html,<script>x</script>",
            "vbscript:msgbox",
            "chrome://settings",
            "ftp://aur.archlinux.org/x",
            // http to a non-loopback host is not a package-homepage channel
            "http://evil.example.com/phish",
            "not a url at all",
            "",
        ] {
            let res = validate_package_url(bad);
            assert!(res.is_err(), "expected reject for {bad:?}");
            assert_eq!(
                res.err().map(|e| e.code),
                Some("invalid_url".to_string()),
                "wrong code for {bad:?}"
            );
        }
    }

    #[test]
    fn package_url_rejects_credentials_controls_and_oversized() {
        // userinfo (credential phishing)
        assert!(validate_package_url("https://user:pass@github.com/x").is_err());
        assert!(validate_package_url("https://user@github.com/").is_err());
        // control characters (WHATWG parser strips CR/LF/TAB — block up front)
        assert!(validate_package_url("https://a.b/\r\nset-cookie:x").is_err());
        assert!(validate_package_url("https://a.b/\n").is_err());
        assert!(validate_package_url("https://a.b/\u{7}").is_err());
        // oversized
        let long = format!("https://a.b/{}", "x".repeat(MAX_URL_BYTES));
        assert!(validate_package_url(&long).is_err());
        // exactly at the cap parses fine
        let at_cap = format!("https://a.b/{}", "x".repeat(MAX_URL_BYTES - 13));
        assert!(validate_package_url(&at_cap).is_ok());
    }
}
