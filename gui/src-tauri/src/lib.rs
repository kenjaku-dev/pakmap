//! pakmap GUI backend: Tauri commands over the pakmap core library.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
//!
//! AC phase: staged streaming search mirroring the TUI orchestrator
//! (`src/tui/app.rs` — per-leg threads, sequential joins, generation guard),
//! full-field package DTO, detail resolution for installed packages. The IPC
//! contract source of truth is `docs/gui-architecture.md`.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use pakmap::model::Package;
use serde::{Deserialize, Serialize};
use tauri::State;
use tauri::ipc::Channel;

mod ops;

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
fn get_updates() -> UpdatesResult {
    let cfg = pakmap::config::Config::load();
    let (repo, repo_err) = match pakmap::updates::check_repo_updates() {
        Ok(v) => (v, None),
        Err(e) => (Vec::new(), Some(format!("{e:#}"))),
    };
    let aur = pakmap::updates::check_aur_updates_blocking_cached(&cfg);
    let orphans = pakmap::orphans::list_orphans()
        .or_else(|_| pakmap::orphans::list_orphans_fallback())
        .unwrap_or_default()
        .into_iter()
        .map(|p| p.name)
        .collect();
    UpdatesResult {
        repo,
        aur,
        repo_err,
        aur_err: None,
        orphans,
    }
}

#[tauri::command]
fn get_news() -> Vec<pakmap::news::NewsItem> {
    let cfg = pakmap::config::Config::load();
    pakmap::news::fetch_arch_news(&cfg)
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
}

#[tauri::command]
fn get_stats() -> GuiStats {
    let cfg = pakmap::config::Config::load();
    let mut s = pakmap::stats::gather_local_stats();
    s.updates_aur = pakmap::updates::check_aur_updates_blocking_cached(&cfg).len();
    s.flatpak_installed = pakmap::sources::flatpak::list_installed_cached()
        .map(|v| v.len())
        .unwrap_or(0);
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
    }
}

/// Detail resolution for a selected row: for installed packages this resolves
/// the dependency/reason metadata through the localdb fast path; remote rows
/// carry their info in the streamed `Package` already (votes, popularity,
/// maintainer, ood) so the frontend passes those straight to the pane.
#[tauri::command]
fn package_info(name: &str) -> Result<PackageDto, GuiError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(gui_error("empty_query", "empty query".to_string(), None));
    }
    let found = pakmap::search::repo::search_local(name, 1, false)
        .or_else(|_| pakmap::search::repo::search_local_fallback(name, 1, false))
        .unwrap_or_default();
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
            get_stats
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
}
