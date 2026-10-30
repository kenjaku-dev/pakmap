use std::io::IsTerminal;

use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use tokio::task;
use tracing_subscriber::{EnvFilter, fmt};

use pakmap::cli::{Cli, Source};
use pakmap::config::Config;
use pakmap::confirm::{confirm_prompt, should_confirm};
use pakmap::output::{print_json_error, print_packages};
use pakmap::search::{search_aur_with_config, search_repo, search_repo_fallback};
use pakmap::sources::search_flatpak_with_config;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut cli = Cli::parse();

    // --init-config / --show-config early exit (no TUI, no search)
    if cli.init_config {
        match Config::init_example() {
            Ok(p) => {
                println!("Created {}", p.display());
                return Ok(());
            }
            Err(e) => {
                eprintln!("init-config: {}", e);
                std::process::exit(1);
            }
        }
    }
    if cli.show_config {
        if let Some(p) = cli.config.clone().or_else(Config::default_path) {
            println!("{}", p.display());
        } else {
            println!("(no config dir)");
        }
        if let Some(p) = Config::project_path() {
            println!("project: {}", p.display());
        }
        return Ok(());
    }

    // Load config: --config PATH > project-local > XDG
    let cfg = if let Some(ref p) = cli.config {
        Config::load_from(p).unwrap_or_default()
    } else {
        Config::load()
    };

    // Merge config into cli where cli is still at default (so config becomes default)
    // This keeps CLI as override — user intent preserved per precedence: defaults < config < CLI
    if cli.limit == 50 && cfg.search.limit != 50 {
        cli.limit = cfg.search.limit;
    }
    if cli.source == Source::All && cfg.effective_source() != Source::All {
        cli.source = cfg.effective_source();
    }
    if cli.by == pakmap::cli::AurBy::NameDesc
        && cfg.effective_aur_by() != pakmap::cli::AurBy::NameDesc
    {
        cli.by = cfg.effective_aur_by();
    }
    if !cli.regex && cfg.search.regex {
        cli.regex = true;
    }
    if !cli.installed_only && cfg.search.installed_only {
        cli.installed_only = true;
    }
    if !cli.bottom_up && (cfg.search.bottom_up || cfg.behavior.bottom_up) {
        cli.bottom_up = true;
    }
    if !cli.no_color && cfg.theme.no_color {
        cli.no_color = true;
    }
    // Determine TUI mode before logging init: the TUI owns the terminal and
    // must stay log-silent (stderr writes would scribble over alt-screen).
    // --tui forces TUI, --no-tui forces plain, --json forces plain, otherwise auto.
    let is_tty = std::io::stdout().is_terminal() && std::io::stdin().is_terminal();
    let should_tui = if cli.no_tui || cli.json {
        false
    } else if cli.tui {
        true
    } else if cli.query.is_none() {
        // No query and TTY -> TUI (your drawing: term window with search bar)
        is_tty
    } else {
        false
    };
    if should_tui {
        if !is_tty {
            eprintln!("Error: --tui requires a terminal (TTY).");
            eprintln!("Try: pakmap <query> --no-tui   or   pakmap --help");
            eprintln!("Tip: In headless/CI, use --json or --no-tui");
            std::process::exit(1);
        }
        let initial = cli.query.clone().unwrap_or_default();
        // TUI owns terminal lifecycle — color_eyre installed inside tui::run per ratatui skill
        return pakmap::tui::run_with_config(initial, &cli, &cfg)
            .map_err(|e| anyhow::anyhow!("{e}"));
    }
    // Tracing init: all remaining paths are one-shot CLI. Logs go to stderr:
    // stdout is the data pipe (--json must stay pure).
    let filter = match cli.verbose {
        0 => EnvFilter::new("warn"),
        1 => EnvFilter::new("info"),
        2 => EnvFilter::new("debug"),
        _ => EnvFilter::new("trace"),
    };
    fmt()
        .with_env_filter(filter)
        .with_target(false)
        .without_time()
        .with_writer(std::io::stderr)
        .init();

    let readonly = cli.readonly || cfg.is_readonly();

    // 0.5.0 read-only fast paths: --updates/--orphans/--stats are safe, always allowed.
    if cli.updates {
        if cli.no_color || std::env::var("NO_COLOR").is_ok() {
            colored::control::set_override(false);
        }
        // Perf P3: repo check joins the async fan-out instead of blocking it.
        let repo_fut = async {
            let timer = pakmap::timing::start("repo-updates");
            let out =
                task::spawn_blocking(|| pakmap::updates::check_repo_updates().unwrap_or_default())
                    .await
                    .unwrap_or_default();
            timer.finish(out.len());
            out
        };
        // AUR check needs async client; reuse timeout from config. P3: tuned
        // builder + warm-up overlapping the repo probe below.
        let client = pakmap::search::aur::client_builder(cfg.search.timeout_secs).build();
        if let Ok(ref client) = client {
            let warm_client = client.clone();
            let warm_rpc = cfg.aur_rpc_url();
            tokio::spawn(async move {
                pakmap::search::aur::warm_async(&warm_client, &warm_rpc).await;
            });
        }
        let mut all = Vec::new();
        if let Ok(client) = client {
            let aur_fut = async {
                let timer = pakmap::timing::start("aur-updates");
                let aur = pakmap::updates::check_aur_updates(&client, &cfg).await;
                timer.finish(aur.len());
                aur
            };
            // Flatpak updates via remote-info (fail-open empty when disabled/offline).
            let cfg_fp = cfg.clone();
            let fp_fut = async {
                let timer = pakmap::timing::start("flatpak-updates");
                let out = task::spawn_blocking(move || {
                    pakmap::sources::check_flatpak_updates_cached(&cfg_fp)
                })
                .await
                .unwrap_or_default();
                timer.finish(out.len());
                out
            };
            let (repo, aur, flatpak) = tokio::join!(repo_fut, aur_fut, fp_fut);
            all.extend(repo);
            all.extend(aur);
            all.extend(flatpak);
        } else {
            // No HTTP client (TLS init failure): still report repo results.
            all.extend(repo_fut.await);
        }
        all.sort_by(|a, b| a.repo.cmp(&b.repo).then(a.name.cmp(&b.name)));
        pakmap::output::print_updates(&all, cli.json, cli.no_color)?;
        return Ok(());
    }
    if cli.orphans {
        if cli.no_color || std::env::var("NO_COLOR").is_ok() {
            colored::control::set_override(false);
        }
        let orphans = pakmap::orphans::list_orphans()
            .or_else(|_| pakmap::orphans::list_orphans_fallback())
            .unwrap_or_default();
        pakmap::output::print_orphans(&orphans, cli.json, cli.no_color)?;
        return Ok(());
    }
    if cli.stats {
        if cli.no_color || std::env::var("NO_COLOR").is_ok() {
            colored::control::set_override(false);
        }
        // Perf P3: all four probes run concurrently; the flatpak branch lists
        // once and reuses the list for the updates check (single scan).
        let client = pakmap::search::aur::client_builder(cfg.search.timeout_secs).build();
        if let Ok(ref client) = client {
            let warm_client = client.clone();
            let warm_rpc = cfg.aur_rpc_url();
            tokio::spawn(async move {
                pakmap::search::aur::warm_async(&warm_client, &warm_rpc).await;
            });
        }
        let local_fut = async {
            let timer = pakmap::timing::start("local-stats");
            let out = match task::spawn_blocking(pakmap::stats::gather_local_stats).await {
                Ok(s) => s,
                Err(e) => {
                    tracing::error!(err=?e, "spawn_blocking JoinError (local stats panicked)");
                    pakmap::stats::SysStats::default()
                }
            };
            timer.finish(out.explicit.saturating_add(out.dependencies));
            out
        };
        let aur_fut = async {
            let timer = pakmap::timing::start("aur-updates");
            let out = match client {
                Ok(client) => pakmap::updates::check_aur_updates(&client, &cfg).await,
                Err(_) => vec![],
            };
            timer.finish(out.len());
            out.len()
        };
        let cfg_fp = cfg.clone();
        let fp_fut = async move {
            if !cfg_fp.flatpak_enabled() {
                return (0usize, 0usize);
            }
            let timer = pakmap::timing::start("flatpak-list");
            // Perf P4: cached inventory shared with the updates check below.
            let listed = match task::spawn_blocking(pakmap::sources::list_installed_cached).await {
                Ok(Ok(v)) => v,
                Ok(Err(e)) => {
                    tracing::warn!(err=?e, "flatpak list failed");
                    vec![]
                }
                Err(e) => {
                    tracing::error!(err=?e, "spawn_blocking JoinError (flatpak list panicked)");
                    vec![]
                }
            };
            timer.finish(listed.len());
            let n_listed = listed.len();
            let timer = pakmap::timing::start("flatpak-updates");
            let up = task::spawn_blocking(move || {
                pakmap::sources::check_flatpak_updates_cached_for(&listed, &cfg_fp)
            })
            .await
            .unwrap_or_default();
            timer.finish(up.len());
            (n_listed, up.len())
        };
        let (mut stats, aur_n, (fp_n, fp_up)) = tokio::join!(local_fut, aur_fut, fp_fut);
        stats.updates_aur = aur_n;
        stats.flatpak_installed = fp_n;
        stats.updates_flatpak = fp_up;
        pakmap::output::print_stats(&stats, cli.json, cli.no_color)?;
        return Ok(());
    }

    // 0.5.0 --upgrade with Arch-news guard (haj/pacsea pattern).
    if cli.upgrade {
        if cli.no_color || std::env::var("NO_COLOR").is_ok() {
            colored::control::set_override(false);
        }
        if readonly {
            eprintln!(
                "refusing to upgrade: readonly mode is enabled (--readonly / [behavior] readonly)"
            );
            std::process::exit(2);
        }
        // News guard unless explicitly non-interactive.
        let noconfirm_env = std::env::var("PAKMAP_NOCONFIRM").is_ok();
        if !noconfirm_env {
            let news = pakmap::news::fetch_arch_news(&cfg);
            if pakmap::news::has_intervention(&news, true) {
                eprintln!("⚠ Arch news reports manual intervention may be required:");
                for item in news.iter().filter(|i| i.needs_action).take(3) {
                    // Titles/links were already filtered at the news source
                    // (Phase 3); blank link = untrusted URL — the print falls
                    // back to the hub instead of echoing it.
                    let title = pakmap::safety::text::sanitize_terminal(&item.title);
                    if item.link.is_empty() {
                        eprintln!("  • {title} (see https://archlinux.org/news/)");
                    } else {
                        eprintln!("  • {title} ({})", item.link);
                    }
                }
                eprintln!("Read https://archlinux.org/news/ before upgrading.");
                if should_confirm(cfg.upgrade_noconfirm(), std::io::stdin().is_terminal()) {
                    let stdin = std::io::stdin();
                    let mut input = stdin.lock();
                    let mut output = std::io::stderr();
                    if !confirm_prompt(
                        "Proceed with system upgrade anyway?",
                        &mut input,
                        &mut output,
                    ) {
                        eprintln!("cancelled");
                        return Ok(());
                    }
                } else if !cfg.upgrade_noconfirm() {
                    eprintln!(
                        "refusing to upgrade without confirmation (no TTY); set PAKMAP_NOCONFIRM=1 to force"
                    );
                    std::process::exit(2);
                }
            }
        }
        let noconfirm = cfg.upgrade_noconfirm();
        if should_confirm(noconfirm, std::io::stdin().is_terminal()) {
            let stdin = std::io::stdin();
            let mut input = stdin.lock();
            let mut output = std::io::stderr();
            if !confirm_prompt(
                "Upgrade system (sudo pacman -Syu + flatpak update)?",
                &mut input,
                &mut output,
            ) {
                eprintln!("cancelled");
                return Ok(());
            }
        } else if !noconfirm {
            eprintln!(
                "refusing to upgrade without confirmation (no TTY); set PAKMAP_NOCONFIRM=1 for scripts"
            );
            std::process::exit(2);
        }
        match pakmap::install::upgrade::upgrade_system(&cfg) {
            Ok(()) => {
                // Flatpak refs upgrade alongside (fail-open when absent).
                // Best-effort: a flatpak failure warns but keeps the pacman
                // upgrade's success (scripts see exit 0 + stderr note).
                // Refresh update memos so the Updates tab is current.
                pakmap::updates::invalidate_aur_memo();
                pakmap::sources::invalidate_flatpak_memo();
                pakmap::sources::invalidate_flatpak_list_memo();
                if let Err(e) = pakmap::sources::update_all_flatpak(&cfg) {
                    eprintln!("warning: flatpak update failed: {}", e);
                }
                println!("System upgraded");
                return Ok(());
            }
            Err(e) => {
                eprintln!("upgrade failed: {}", e);
                std::process::exit(1);
            }
        }
    }

    // 5.1 --clean fast path: live paccache preview, then confirm + run.
    // Preview is always live (never cached); missing paccache exits 1 with
    // an install hint, refusals exit 2 like the other mutating paths.
    if cli.clean {
        if readonly {
            eprintln!(
                "refusing to clean: readonly mode is enabled (--readonly / [behavior] readonly)"
            );
            std::process::exit(2);
        }
        if cli.no_color || std::env::var("NO_COLOR").is_ok() {
            colored::control::set_override(false);
        }
        let keep = cfg.effective_keep(cli.keep);
        let preview = match pakmap::install::clean::preview_clean(keep) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("clean failed: {}", e);
                std::process::exit(1);
            }
        };
        if cli.dry_run || cli.json {
            pakmap::output::print_clean(&preview, true, cli.json, cli.no_color)?;
            // Flatpak has no byte preview; say so instead of silence.
            if !cli.json
                && cfg.flatpak_enabled()
                && pakmap::sources::has_flatpak()
                && preview.files > 0
            {
                eprintln!("+ unused flatpak runtimes (no preview available)");
            }
            return Ok(());
        }
        // P5: unused flatpak runtimes clean alongside paccache when available.
        let flatpak_clean = cfg.flatpak_enabled() && pakmap::sources::has_flatpak();
        if preview.files == 0 && !flatpak_clean {
            pakmap::output::print_clean(&preview, true, cli.json, cli.no_color)?;
            return Ok(());
        }
        let noconfirm = cfg.clean_noconfirm();
        if should_confirm(noconfirm, std::io::stdin().is_terminal()) {
            let stdin = std::io::stdin();
            let mut input = stdin.lock();
            let mut output = std::io::stderr();
            let size = pakmap::stats::human_bytes(preview.bytes);
            let prompt = if preview.files == 0 {
                "Pacman cache already clean. Remove unused flatpak runtimes?".to_string()
            } else if flatpak_clean {
                format!(
                    "Remove {} cached files ({})? [keep {}] + unused flatpak runtimes",
                    preview.files, size, keep
                )
            } else {
                format!(
                    "Remove {} cached files ({})? [keep {}]",
                    preview.files, size, keep
                )
            };
            if !confirm_prompt(&prompt, &mut input, &mut output) {
                eprintln!("cancelled");
                return Ok(());
            }
        } else if !noconfirm {
            eprintln!(
                "refusing to clean without confirmation (no TTY); set PAKMAP_NOCONFIRM=1 for scripts"
            );
            std::process::exit(2);
        }
        // The confirm above covers both stages: force -y downstream so neither
        // tool re-prompts after the user already said yes.
        if preview.files > 0 {
            match pakmap::install::clean::run_clean(&cfg, keep) {
                Ok(()) => {
                    pakmap::output::print_clean(&preview, false, cli.json, cli.no_color)?;
                }
                Err(e) => {
                    eprintln!("clean failed: {}", e);
                    std::process::exit(1);
                }
            }
        }
        if flatpak_clean {
            let mut cfg_fp = cfg.clone();
            cfg_fp.behavior.flatpak_noconfirm = true;
            match pakmap::sources::clean_unused_flatpak(&cfg_fp) {
                Ok(()) => {
                    println!("✓ Removed unused flatpak runtimes");
                }
                Err(e) => {
                    eprintln!("flatpak clean failed: {}", e);
                    std::process::exit(1);
                }
            }
        }
        return Ok(());
    }

    // --remove fast path (no TUI, no search) — mirrors remover mode confirm in TUI.
    // Destructive: confirm on TTY unless noconfirm; refuse silently-piped runs.
    if let Some(name) = cli.remove.clone() {
        if readonly {
            eprintln!(
                "refusing to remove: readonly mode is enabled (--readonly / [behavior] readonly)"
            );
            std::process::exit(2);
        }
        if cli.no_color || std::env::var("NO_COLOR").is_ok() {
            colored::control::set_override(false);
        }
        // P3: flatpak app-IDs (reverse-DNS, installed via flatpak) uninstall
        // without sudo. Shape + installed-state gate avoids misrouting pacman
        // names; readonly already refused above.
        let is_flatpak = cfg.flatpak_enabled()
            && pakmap::sources::validate_app_id(&name).is_ok()
            && pakmap::sources::is_flatpak_installed(&name);
        // Either noconfirm flag covers --remove end-to-end (flatpak -y and
        // pacman --noconfirm alike) so scripts need only one knob.
        let noconfirm = cfg.remove_noconfirm() || (is_flatpak && cfg.flatpak_noconfirm());
        if should_confirm(noconfirm, std::io::stdin().is_terminal()) {
            let stdin = std::io::stdin();
            let mut input = stdin.lock();
            let mut output = std::io::stderr();
            let prompt = if is_flatpak {
                format!("Remove flatpak '{}'?", name.trim())
            } else {
                format!("Remove '{}'?", name.trim())
            };
            if !confirm_prompt(&prompt, &mut input, &mut output) {
                eprintln!("cancelled");
                return Ok(());
            }
        } else if !noconfirm {
            eprintln!(
                "refusing to remove without confirmation (no TTY); set PAKMAP_NOCONFIRM=1 for scripts"
            );
            std::process::exit(2);
        }
        // flatpak -y follows the same noconfirm so scripts behave identically.
        let res = if is_flatpak {
            let mut cfg_fp = cfg.clone();
            if noconfirm {
                cfg_fp.behavior.flatpak_noconfirm = true;
            }
            pakmap::sources::remove_flatpak_package(&name, &cfg_fp)
        } else {
            pakmap::install::remove::remove_package_with_config(&name, &cfg)
        };
        match res {
            Ok(()) => {
                // Installed set changed — updates + inventory memos are stale.
                pakmap::sources::invalidate_flatpak_memo();
                pakmap::sources::invalidate_flatpak_list_memo();
                println!("Removed {}", name.trim());
                return Ok(());
            }
            Err(e) => {
                eprintln!("remove failed: {}", e);
                std::process::exit(1);
            }
        }
    }

    // --refresh fast path (no TUI, no search) — mirrors TUI r/F5 confirm in TUI.
    // Runs sudo: confirm on TTY unless noconfirm; refuse silently-piped runs.
    if cli.refresh {
        if readonly {
            eprintln!(
                "refusing to refresh: readonly mode is enabled (--readonly / [behavior] readonly)"
            );
            std::process::exit(2);
        }
        if cli.no_color || std::env::var("NO_COLOR").is_ok() {
            colored::control::set_override(false);
        }
        let noconfirm = cfg.refresh_noconfirm();
        if should_confirm(noconfirm, std::io::stdin().is_terminal()) {
            let stdin = std::io::stdin();
            let mut input = stdin.lock();
            let mut output = std::io::stderr();
            if !confirm_prompt(
                "Refresh sync databases (sudo pacman -Sy)?",
                &mut input,
                &mut output,
            ) {
                eprintln!("cancelled");
                return Ok(());
            }
        } else if !noconfirm {
            eprintln!(
                "refusing to refresh without confirmation (no TTY); set PAKMAP_NOCONFIRM=1 for scripts"
            );
            std::process::exit(2);
        }
        match pakmap::install::refresh::refresh_sync_db(&cfg) {
            Ok(()) => {
                println!("Sync databases refreshed");
                return Ok(());
            }
            Err(e) => {
                eprintln!("refresh failed: {}", e);
                std::process::exit(1);
            }
        }
    }

    // --- CLI one-shot mode (existing): TUI already returned above. ---
    // Need query now
    let query = match cli.query {
        Some(q) if !q.trim().is_empty() => q,
        _ => {
            eprintln!("Usage: pakmap <query>  or  pakmap --tui");
            eprintln!("Try 'pakmap --help' for more information.");
            std::process::exit(2);
        }
    };

    if cli.no_color || std::env::var("NO_COLOR").is_ok() {
        colored::control::set_override(false);
    }

    // P3 perf: tuned client (gzip/connect-bound/nodelay) built once for the
    // whole fan-out.
    let client = pakmap::search::aur::client_builder(cfg.search.timeout_secs).build()?;

    // P3 perf: warm the RPC pool while the repo probe runs — the handshake
    // overlaps the repo/flatpak legs, so the AUR request rides an established
    // connection (one-shot per process; skipped under PAKMAP_SKIP_LIVE).
    {
        let warm_client = client.clone();
        let warm_rpc = cfg.aur_rpc_url();
        tokio::spawn(async move {
            pakmap::search::aur::warm_async(&warm_client, &warm_rpc).await;
        });
    }

    let query_repo = query.clone();
    let query_aur = query.clone();
    let query_flatpak = query.clone();
    let limit_repo = cli.limit;
    let limit_aur = cli.limit;
    let limit_flatpak = cli.limit;
    let regex_repo = cli.regex;
    let regex_aur = cli.regex;
    let regex_flatpak = cli.regex;
    let installed_only = cli.installed_only;
    let by_str = cli.by.as_str().to_string();
    let cfg_flatpak = cfg.clone();

    let repo_enabled = matches!(cli.source, Source::Repo | Source::All);
    let aur_enabled = matches!(cli.source, Source::Aur | Source::All) && !cli.installed_only;
    let flatpak_enabled = matches!(cli.source, Source::Flatpak | Source::All)
        && !cli.installed_only
        && cfg.flatpak_enabled();

    let repo_future = async {
        if !repo_enabled {
            Vec::new()
        } else {
            // P1 perf: time the backend (warns over budget at default verbosity).
            let timer = pakmap::timing::start("repo-search");
            let res = task::spawn_blocking(move || {
                match search_repo(&query_repo, limit_repo, regex_repo, installed_only) {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::warn!(err=?e, "alpm search failed, falling back to pacman -Ss");
                        match search_repo_fallback(&query_repo, limit_repo) {
                            Ok(v) => v,
                            Err(e2) => {
                                tracing::error!(err=?e2, "fallback also failed");
                                vec![]
                            }
                        }
                    }
                }
            })
            .await;
            let out = match res {
                Ok(v) => v,
                Err(e) => {
                    tracing::error!(err=?e, "spawn_blocking JoinError (repo search panicked)");
                    vec![]
                }
            };
            timer.finish(out.len());
            out
        }
    };

    if cli.installed_only && !matches!(cli.source, Source::Repo) && !cli.json {
        eprintln!(
            "Note: --installed-only only applies to repo packages, AUR/flatpak results hidden"
        );
    }

    let aur_future = async {
        if !aur_enabled {
            Vec::new()
        } else {
            let timer = pakmap::timing::start("aur-search");
            let out = match search_aur_with_config(
                &client, &query_aur, &by_str, limit_aur, regex_aur, &cfg,
            )
            .await
            {
                Ok(v) => v,
                Err(e) => {
                    tracing::error!(err=?e, "AUR search failed");
                    if cli.json {
                        print_json_error(&format!("AUR error: {e}"));
                    } else {
                        eprintln!("AUR search error: {}", e);
                    }
                    Vec::new()
                }
            };
            timer.finish(out.len());
            out
        }
    };

    let flatpak_future = async {
        if !flatpak_enabled {
            Vec::new()
        } else {
            let timer = pakmap::timing::start("flatpak-search");
            let q = query_flatpak.clone();
            let res = task::spawn_blocking(move || {
                search_flatpak_with_config(&q, limit_flatpak, regex_flatpak, &cfg_flatpak)
                    .unwrap_or_else(|e| {
                        tracing::warn!(err=?e, "flatpak search failed");
                        vec![]
                    })
            })
            .await;
            let out = match res {
                Ok(v) => v,
                Err(e) => {
                    tracing::error!(err=?e, "spawn_blocking JoinError (flatpak search panicked)");
                    vec![]
                }
            };
            timer.finish(out.len());
            out
        }
    };

    let spinner = if !cli.json && cli.verbose == 0 {
        let pb = ProgressBar::new_spinner();
        let tick = cfg
            .tui
            .tick_chars
            .clone()
            .unwrap_or_else(|| "⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏ ".into());
        let tmpl = cfg
            .tui
            .spinner_template
            .clone()
            .unwrap_or_else(|| "{spinner:.cyan} {msg}".into());
        let style = ProgressStyle::default_spinner()
            .tick_chars(&tick)
            .template(&tmpl)
            .unwrap_or_else(|_| ProgressStyle::default_spinner());
        pb.set_style(style);
        pb.set_message(format!("Searching for '{}'...", query));
        pb.enable_steady_tick(std::time::Duration::from_millis(80));
        Some(pb)
    } else {
        None
    };

    let total_timer = pakmap::timing::start("search-total");
    let (repo_packages, aur_packages, flatpak_packages) =
        tokio::join!(repo_future, aur_future, flatpak_future);
    total_timer.finish(
        repo_packages
            .len()
            .saturating_add(aur_packages.len())
            .saturating_add(flatpak_packages.len()),
    );

    if let Some(pb) = spinner {
        pb.finish_and_clear();
    }

    print_packages(
        &repo_packages,
        &aur_packages,
        &flatpak_packages,
        cli.source,
        cli.bottom_up,
        cli.json,
        cli.no_color,
    )?;

    Ok(())
}
