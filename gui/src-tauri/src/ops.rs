//! AD operations layer: install / remove / upgrade / clean / refresh + flatpak
//! user-scope ops, streamed over a typed `Channel` (one per operation).
//!
//! Privilege model (docs/gui-architecture.md):
//! - the GUI process stays unprivileged; pacman/paccache run under **pkexec**
//!   (polkit owns the auth UI) — `sudo` needs a TTY and a GUI has none, which
//!   is why the CLI's `install::*` TTY-inherit flows are not reused here;
//! - pacman/paccache/kexec'd commands in the GUI are **always
//!   non-interactive** (`--noconfirm`). The polkit dialog is the
//!   confirmation; pacman's own y/n prompt has no stdin to read. Divergence
//!   from the TUI is documented in the architecture doc;
//! - flatpak ops run user-scope, no escalation, `--noninteractive`;
//! - AUR builds follow aur-guides: never root, cloned into the same cache dir
//!   the TUI/diff module uses, built with makepkg unprivileged, installed via
//!   `pkexec pacman -U`.
//!
//! Failure states are classified from the stderr tail into the shared
//! `GuiError` envelope so the frontend error cards are actionable, never bare
//! exit codes. Cancellation: `cancel_operation` flips a flag that runners
//! check **only at phase boundaries** (clone→build→install). A pacman
//! transaction is never killed mid-flight; the UI copy says so.

use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;

use crate::GuiError;

/// Tagged union wire shape: `{"kind": "install_repo", "args": {"name": ...}}`.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", content = "args")]
pub enum OpRequest {
    #[serde(rename = "install_repo")]
    InstallRepo { name: String },
    #[serde(rename = "install_flatpak")]
    InstallFlatpak { app_id: String },
    #[serde(rename = "install_aur")]
    InstallAur { name: String },
    #[serde(rename = "remove")]
    Remove { name: String },
    #[serde(rename = "remove_flatpak")]
    RemoveFlatpak { app_id: String },
    #[serde(rename = "remove_orphans")]
    RemoveOrphans,
    #[serde(rename = "upgrade")]
    Upgrade,
    #[serde(rename = "refresh")]
    Refresh,
    #[serde(rename = "clean")]
    Clean { keep: u32 },
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OpMessage {
    /// one journal line from the running operation
    Progress {
        op_id: u64,
        phase: String,
        line: String,
    },
    /// terminal state; `ok=false` carries the classified envelope
    Done {
        op_id: u64,
        ok: bool,
        exit_code: Option<i32>,
        error: Option<GuiError>,
    },
}

/// Tracks op ids and per-op cancel flags. Runners read the flag only at
/// phase boundaries, so cancel applies between phases only — by construction.
pub struct OpsState {
    next_id: AtomicU64,
    cancel_flags: Mutex<HashMap<u64, Arc<AtomicBool>>>,
}

impl OpsState {
    pub fn new() -> Self {
        Self {
            next_id: AtomicU64::new(0),
            cancel_flags: Mutex::new(HashMap::new()),
        }
    }

    fn register(&self, op_id: u64, flag: Arc<AtomicBool>) {
        if let Ok(mut map) = self.cancel_flags.lock() {
            map.insert(op_id, flag);
        }
    }

    /// Flip the cancel flag. Returns false when the op is unknown/finished.
    pub fn cancel(&self, op_id: u64) -> bool {
        match self.cancel_flags.lock() {
            Ok(map) => match map.get(&op_id) {
                Some(flag) => {
                    flag.store(true, Ordering::Relaxed);
                    true
                }
                None => false,
            },
            Err(_) => false,
        }
    }

    fn unregister(&self, op_id: u64) {
        if let Ok(mut map) = self.cancel_flags.lock() {
            map.remove(&op_id);
        }
    }
}

// ---------------------------------------------------------------------------
// spawn plumbing
// ---------------------------------------------------------------------------

struct SpawnResult {
    ok: bool,
    exit_code: Option<i32>,
    stderr: String,
}

enum SpawnError {
    /// pkexec is not installed — pacman/paccache ops cannot escalate
    PkexecMissing,
    /// the process itself could not be started
    Io(String),
}

impl SpawnError {
    fn into_error(self) -> GuiError {
        match self {
            SpawnError::PkexecMissing => GuiError {
                code: "pkexec_missing".to_string(),
                message: "pkexec not found — PolicyKit is required for pacman operations"
                    .to_string(),
                hint: Some(
                    "install polkit (pacman -S polkit) and run a polkit authentication agent"
                        .to_string(),
                ),
            },
            SpawnError::Io(msg) => GuiError {
                code: "op_failed".to_string(),
                message: format!("failed to spawn the operation process: {msg}"),
                hint: None,
            },
        }
    }
}

/// Stream one child process to completion: stdout/stderr lines become
/// `Progress` events on the channel; the exit status is returned to the
/// caller, which decides when to emit the single terminal `Done` event.
///
/// Interactive by design: unbounded wait, like the TUI's mutate runs — a
/// polkit dialog can sit open while the user thinks about it.
fn spawn_streamed_once(
    program: &str,
    args: &[String],
    cwd: Option<&std::path::Path>,
    op_id: u64,
    phase: &str,
    channel: &Channel<OpMessage>,
) -> Result<SpawnResult, SpawnError> {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return Err(SpawnError::Io(e.to_string())),
    };

    let Some(stdout) = child.stdout.take() else {
        let err = SpawnError::Io("stdout was not piped".to_string());
        let status = child.wait();
        let _ = status;
        return Err(err);
    };
    let Some(stderr) = child.stderr.take() else {
        let err = SpawnError::Io("stderr was not piped".to_string());
        let status = child.wait();
        let _ = status;
        return Err(err);
    };

    let out_channel = channel.clone();
    let out_phase = phase.to_string();
    let out_reader = std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let _ = out_channel.send(OpMessage::Progress {
                op_id,
                phase: out_phase.clone(),
                line,
            });
        }
    });

    let err_channel = channel.clone();
    let err_phase = phase.to_string();
    let err_reader = std::thread::spawn(move || {
        // stderr flows to the journal too (pacman reports there) and the
        // bounded tail is the failure-classification input.
        let mut tail: VecDeque<String> = VecDeque::new();
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            tail.push_back(line.clone());
            if tail.len() > 40 {
                tail.pop_front();
            }
            let _ = err_channel.send(OpMessage::Progress {
                op_id,
                phase: err_phase.clone(),
                line,
            });
        }
        tail.into_iter().collect::<Vec<_>>().join("\n")
    });

    let status = child.wait();
    let _ = out_reader.join();
    let stderr_text = (err_reader.join()).unwrap_or_default();

    Ok(match status {
        Ok(s) => SpawnResult {
            ok: s.success(),
            exit_code: s.code(),
            stderr: stderr_text,
        },
        Err(e) => SpawnResult {
            ok: false,
            exit_code: None,
            stderr: e.to_string(),
        },
    })
}

fn escalated(
    args: Vec<String>,
    op_id: u64,
    phase: &str,
    channel: &Channel<OpMessage>,
) -> Result<SpawnResult, SpawnError> {
    if !pakmap::process::has_tool("pkexec") {
        return Err(SpawnError::PkexecMissing);
    }
    spawn_streamed_once("pkexec", &args, None, op_id, phase, channel)
}

fn unprivileged(
    program: &str,
    args: Vec<String>,
    cwd: Option<&std::path::Path>,
    op_id: u64,
    phase: &str,
    channel: &Channel<OpMessage>,
) -> Result<SpawnResult, SpawnError> {
    spawn_streamed_once(program, &args, cwd, op_id, phase, channel)
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn done_ok(op_id: u64, exit_code: Option<i32>, channel: &Channel<OpMessage>) {
    let _ = channel.send(OpMessage::Done {
        op_id,
        ok: true,
        exit_code,
        error: None,
    });
}

fn done_err(op_id: u64, exit_code: Option<i32>, error: GuiError, channel: &Channel<OpMessage>) {
    let _ = channel.send(OpMessage::Done {
        op_id,
        ok: false,
        exit_code,
        error: Some(error),
    });
}

fn progress(op_id: u64, phase: &str, line: &str, channel: &Channel<OpMessage>) {
    let _ = channel.send(OpMessage::Progress {
        op_id,
        phase: phase.to_string(),
        line: line.to_string(),
    });
}

fn canceled_error() -> GuiError {
    GuiError {
        code: "op_canceled".to_string(),
        message: "operation canceled before the next phase".to_string(),
        hint: Some("a running transaction is never killed mid-flight, by design".to_string()),
    }
}

fn readonly_error(op: &str) -> GuiError {
    GuiError {
        code: "readonly_mode".to_string(),
        message: format!("refusing {op}: readonly mode is enabled"),
        hint: Some(
            "set behavior.readonly = false in ~/.config/pakmap/config.toml to allow mutations"
                .to_string(),
        ),
    }
}

pub(crate) fn invalid_name_error(name: &str) -> GuiError {
    GuiError {
        code: "invalid_name".to_string(),
        message: format!("invalid package name: {name:?}"),
        hint: None,
    }
}

/// Same policy as the CLI's install/remove guards (no empty/traversal names).
/// argv passing is already shell-safe; this is consistency, not security.
/// Reused by the v0.9 pkgbuild review (same untrusted-name boundary).
pub(crate) fn validate_name(name: &str) -> Result<(), GuiError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(GuiError {
            code: "invalid_name".to_string(),
            message: "empty package name".to_string(),
            hint: None,
        });
    }
    if trimmed.contains('/')
        || trimmed.contains('\\')
        || trimmed.contains("..")
        || trimmed.contains('\0')
    {
        return Err(invalid_name_error(trimmed));
    }
    Ok(())
}

/// Post-op memo invalidation — mirrors the phase-Z audit finding: stale update
/// memos after mutations lie to the user.
fn invalidate_memos() {
    pakmap::updates::invalidate_aur_memo();
}

/// Classify a failed op from its stderr tail — mapped to actionable codes.
/// Keep this list in sync with the failure matrix in docs/gui-architecture.md.
pub fn classify_op_failure(exit_code: Option<i32>, stderr: &str) -> GuiError {
    let s = stderr.to_lowercase();
    let failure_from_tail = |code: &str, hint: Option<String>| GuiError {
        code: code.to_string(),
        message: stderr
            .trim()
            .lines()
            .last()
            .unwrap_or("operation failed")
            .to_string(),
        hint,
    };

    if s.contains("disapproved") || s.contains("request dismissed") || s.contains("not authorized")
    {
        return GuiError {
            code: "auth_dismissed".to_string(),
            message: "authentication was dismissed or failed".to_string(),
            hint: Some(
                "policykit denied the request — retry and answer the polkit dialog".to_string(),
            ),
        };
    }
    if s.contains("database is locked") || s.contains("unable to lock database") {
        return GuiError {
            code: "db_locked".to_string(),
            message: "pacman database is locked".to_string(),
            hint: Some(
                "wait for the running pacman to finish, or remove a stale /var/lib/pacman/db.lck"
                    .to_string(),
            ),
        };
    }
    if s.contains("failed to synchronize")
        || s.contains("failed to download")
        || s.contains("temporary failure")
        || s.contains("timed out")
    {
        return GuiError {
            code: "network_failure".to_string(),
            message: "network failure while running the operation".to_string(),
            hint: Some("check connectivity / mirror reachability and retry".to_string()),
        };
    }
    if s.contains("could not satisfy dependencies") || s.contains("unresolvable package conflicts")
    {
        return GuiError {
            code: "unresolvable_deps".to_string(),
            message: "dependency resolution failed".to_string(),
            hint: Some("install or update the listed dependencies first, then retry".to_string()),
        };
    }
    if s.contains("target not found") || s.contains("was not found") {
        return GuiError {
            code: "package_not_found".to_string(),
            message: "package not found in the configured repositories".to_string(),
            hint: Some(
                "check the spelling; the repositories you have synced do not carry it".to_string(),
            ),
        };
    }
    failure_from_tail(
        "op_failed",
        exit_code.map(|c| format!("exit code {c} — see the operation journal")),
    )
}

fn pacman_args(op: &str, mut opts: Vec<String>, targets: Vec<String>) -> Vec<String> {
    let mut v = vec!["pacman".to_string(), op.to_string()];
    v.append(&mut opts);
    v.extend(targets);
    v
}

// ---------------------------------------------------------------------------
// flows
// ---------------------------------------------------------------------------

fn install_repo_flow(op_id: u64, name: &str, channel: &Channel<OpMessage>) {
    let cfg = pakmap::config::Config::load();
    if cfg.is_readonly() {
        return done_err(op_id, None, readonly_error("install"), channel);
    }
    if let Err(e) = validate_name(name) {
        return done_err(op_id, None, e, channel);
    }
    progress(
        op_id,
        "prepare",
        &format!("installing {name} via pkexec pacman -S --needed --noconfirm"),
        channel,
    );
    let args = pacman_args(
        "-S",
        vec!["--needed".into(), "--noconfirm".into()],
        vec![name.trim().to_string()],
    );
    match escalated(args, op_id, "install", channel) {
        Ok(r) if r.ok => {
            invalidate_memos();
            done_ok(op_id, r.exit_code, channel);
        }
        Ok(r) => done_err(
            op_id,
            r.exit_code,
            classify_op_failure(r.exit_code, &r.stderr),
            channel,
        ),
        Err(e) => done_err(op_id, None, e.into_error(), channel),
    }
}

fn remove_flow(op_id: u64, name: &str, channel: &Channel<OpMessage>) {
    let cfg = pakmap::config::Config::load();
    if cfg.is_readonly() {
        return done_err(op_id, None, readonly_error("remove"), channel);
    }
    if let Err(e) = validate_name(name) {
        return done_err(op_id, None, e, channel);
    }
    progress(op_id, "prepare", &format!("removing {name}"), channel);
    let args = pacman_args(
        "-Rs",
        vec!["--noconfirm".into()],
        vec![name.trim().to_string()],
    );
    match escalated(args, op_id, "remove", channel) {
        Ok(r) if r.ok => {
            invalidate_memos();
            done_ok(op_id, r.exit_code, channel);
        }
        Ok(r) => done_err(
            op_id,
            r.exit_code,
            classify_op_failure(r.exit_code, &r.stderr),
            channel,
        ),
        Err(e) => done_err(op_id, None, e.into_error(), channel),
    }
}

fn remove_orphans_flow(op_id: u64, channel: &Channel<OpMessage>) {
    let cfg = pakmap::config::Config::load();
    if cfg.is_readonly() {
        return done_err(op_id, None, readonly_error("remove orphans"), channel);
    }
    let orphans = pakmap::orphans::list_orphans()
        .or_else(|_| pakmap::orphans::list_orphans_fallback())
        .unwrap_or_default();
    if orphans.is_empty() {
        progress(
            op_id,
            "prepare",
            "no orphans found — nothing to remove",
            channel,
        );
        return done_ok(op_id, None, channel);
    }
    let names: Vec<String> = orphans.iter().map(|p| p.name.clone()).collect();
    progress(
        op_id,
        "prepare",
        &format!("removing {} orphan(s): {}", names.len(), names.join(", ")),
        channel,
    );
    let mut args = vec![
        "pacman".to_string(),
        "-Rns".to_string(),
        "--noconfirm".to_string(),
    ];
    args.extend(names);
    match escalated(args, op_id, "remove", channel) {
        Ok(r) if r.ok => {
            invalidate_memos();
            done_ok(op_id, r.exit_code, channel);
        }
        Ok(r) => done_err(
            op_id,
            r.exit_code,
            classify_op_failure(r.exit_code, &r.stderr),
            channel,
        ),
        Err(e) => done_err(op_id, None, e.into_error(), channel),
    }
}

fn upgrade_flow(op_id: u64, channel: &Channel<OpMessage>) {
    let cfg = pakmap::config::Config::load();
    if cfg.is_readonly() {
        return done_err(op_id, None, readonly_error("upgrade"), channel);
    }
    progress(
        op_id,
        "prepare",
        "full system upgrade: pkexec pacman -Syu --noconfirm",
        channel,
    );
    let args = pacman_args("-Syu", vec!["--noconfirm".into()], vec![]);
    match escalated(args, op_id, "upgrade", channel) {
        Ok(r) if r.ok => {
            invalidate_memos();
            done_ok(op_id, r.exit_code, channel);
        }
        Ok(r) => done_err(
            op_id,
            r.exit_code,
            classify_op_failure(r.exit_code, &r.stderr),
            channel,
        ),
        Err(e) => done_err(op_id, None, e.into_error(), channel),
    }
}

fn refresh_flow(op_id: u64, channel: &Channel<OpMessage>) {
    let cfg = pakmap::config::Config::load();
    if cfg.is_readonly() {
        return done_err(op_id, None, readonly_error("refresh"), channel);
    }
    progress(
        op_id,
        "prepare",
        "refreshing sync databases: pkexec pacman -Sy",
        channel,
    );
    let args = pacman_args("-Sy", vec![], vec![]);
    match escalated(args, op_id, "refresh", channel) {
        Ok(r) if r.ok => {
            invalidate_memos();
            done_ok(op_id, r.exit_code, channel);
        }
        Ok(r) => done_err(
            op_id,
            r.exit_code,
            classify_op_failure(r.exit_code, &r.stderr),
            channel,
        ),
        Err(e) => done_err(op_id, None, e.into_error(), channel),
    }
}

fn clean_flow(op_id: u64, keep: u32, channel: &Channel<OpMessage>) {
    let cfg = pakmap::config::Config::load();
    if cfg.is_readonly() {
        return done_err(op_id, None, readonly_error("clean"), channel);
    }
    if !pakmap::install::clean::has_paccache() {
        return done_err(
            op_id,
            None,
            GuiError {
                code: "paccache_missing".to_string(),
                message: "paccache not found".to_string(),
                hint: Some("install pacman-contrib to clean the package cache".to_string()),
            },
            channel,
        );
    }
    let keep = keep.max(1);
    progress(
        op_id,
        "prepare",
        &format!("cleaning cache: pkexec paccache -r -k{keep}"),
        channel,
    );
    let args = vec![
        "paccache".to_string(),
        "-r".to_string(),
        format!("-k{keep}"),
    ];
    match escalated(args, op_id, "clean", channel) {
        Ok(r) if r.ok => done_ok(op_id, r.exit_code, channel),
        Ok(r) => done_err(
            op_id,
            r.exit_code,
            classify_op_failure(r.exit_code, &r.stderr),
            channel,
        ),
        Err(e) => done_err(op_id, None, e.into_error(), channel),
    }
}

fn flatpak_flow(op_id: u64, install: bool, app_id: &str, channel: &Channel<OpMessage>) {
    let cfg = pakmap::config::Config::load();
    if cfg.is_readonly() {
        let op = if install {
            "flatpak install"
        } else {
            "flatpak remove"
        };
        return done_err(op_id, None, readonly_error(op), channel);
    }
    if !pakmap::process::has_tool("flatpak") {
        return done_err(
            op_id,
            None,
            GuiError {
                code: "flatpak_missing".to_string(),
                message: "flatpak not found".to_string(),
                hint: Some(
                    "install the flatpak package, or disable behavior.flatpak_enabled".to_string(),
                ),
            },
            channel,
        );
    }
    if let Err(e) = validate_name(app_id) {
        return done_err(op_id, None, e, channel);
    }
    let verb = if install { "install" } else { "remove" };
    progress(
        op_id,
        "flatpak",
        &format!("flatpak {verb} {app_id} (user scope)"),
        channel,
    );
    let args = vec![
        verb.to_string(),
        "--user".to_string(),
        "--noninteractive".to_string(),
        app_id.trim().to_string(),
    ];
    // flatpak is unprivileged — no pkexec, no polkit dialog.
    match unprivileged("flatpak", args, None, op_id, "flatpak", channel) {
        Ok(r) if r.ok => done_ok(op_id, r.exit_code, channel),
        Ok(r) => done_err(
            op_id,
            r.exit_code,
            classify_op_failure(r.exit_code, &r.stderr),
            channel,
        ),
        Err(e) => done_err(op_id, None, e.into_error(), channel),
    }
}

/// AUR build: clone/pull (bounded) → makepkg build (streamed, unprivileged,
/// cwd = clone dir) → `pkexec pacman -U --noconfirm` of every built package
/// file. Cancel checks sit at the phase boundaries only.
fn install_aur_flow(op_id: u64, name: &str, cancel: &AtomicBool, channel: &Channel<OpMessage>) {
    let cfg = pakmap::config::Config::load();
    if cfg.is_readonly() {
        return done_err(op_id, None, readonly_error("AUR install"), channel);
    }
    if let Err(e) = validate_name(name) {
        return done_err(op_id, None, e, channel);
    }

    // ---- phase 1: clone or pull (bounded, same dir as the TUI/diff module)
    if cancel.load(Ordering::Relaxed) {
        return done_err(op_id, None, canceled_error(), channel);
    }
    let Some(clone_dir) = pakmap::diff::clone_dir_for(&cfg, name.trim()) else {
        return done_err(
            op_id,
            None,
            GuiError {
                code: "aur_clone_failed".to_string(),
                message: format!("could not resolve the cache dir for clone of '{name}'"),
                hint: Some("set behavior.cache_dir in ~/.config/pakmap/config.toml".to_string()),
            },
            channel,
        );
    };
    progress(
        op_id,
        "clone",
        &format!("preparing clone of {name}"),
        channel,
    );
    let cloned = if clone_dir.exists() {
        pakmap::process::status_null_bounded(
            "git",
            &["-C", &clone_dir.to_string_lossy(), "pull", "--ff-only"],
            pakmap::process::CLONE_TIMEOUT,
        )
        .map(|s| s.success())
        .unwrap_or(false)
    } else {
        let url = format!("https://aur.archlinux.org/{}.git", name.trim());
        pakmap::process::status_null_bounded(
            "git",
            &["clone", &url, &clone_dir.to_string_lossy()],
            pakmap::process::CLONE_TIMEOUT,
        )
        .map(|s| s.success())
        .unwrap_or(false)
    };
    if !cloned {
        return done_err(
            op_id,
            None,
            GuiError {
                code: "aur_clone_failed".to_string(),
                message: format!("git clone/pull failed for '{name}'"),
                hint: Some("check connectivity to aur.archlinux.org and retry".to_string()),
            },
            channel,
        );
    }

    // ---- phase 2: build (unprivileged, streamed, cwd = clone dir)
    if cancel.load(Ordering::Relaxed) {
        return done_err(op_id, None, canceled_error(), channel);
    }
    progress(
        op_id,
        "build",
        "makepkg -srf --noconfirm (unprivileged)",
        channel,
    );
    let build_args = vec!["-srf".to_string(), "--noconfirm".to_string()];
    let build = match unprivileged(
        "makepkg",
        build_args,
        Some(&clone_dir),
        op_id,
        "build",
        channel,
    ) {
        Ok(b) => b,
        Err(e) => return done_err(op_id, None, e.into_error(), channel),
    };
    if !build.ok {
        return done_err(
            op_id,
            build.exit_code,
            classify_op_failure(build.exit_code, &build.stderr),
            channel,
        );
    }
    let built = find_built_packages(&clone_dir);
    if built.is_empty() {
        return done_err(
            op_id,
            None,
            GuiError {
                code: "aur_build_empty".to_string(),
                message: "makepkg produced no package file".to_string(),
                hint: Some(
                    "read the build journal — bad checksums and failing PKGBUILDs are common"
                        .to_string(),
                ),
            },
            channel,
        );
    }
    let built_names: Vec<String> = built
        .iter()
        .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
        .collect();
    progress(
        op_id,
        "build",
        &format!("built: {}", built_names.join(", ")),
        channel,
    );

    // ---- phase 3: install the built packages (privilege crosses here)
    if cancel.load(Ordering::Relaxed) {
        return done_err(op_id, None, canceled_error(), channel);
    }
    progress(
        op_id,
        "install",
        "pkexec pacman -U --noconfirm (built packages)",
        channel,
    );
    let args = pacman_args(
        "-U",
        vec!["--noconfirm".into()],
        built
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect(),
    );
    match escalated(args, op_id, "install", channel) {
        Ok(r) if r.ok => {
            invalidate_memos();
            done_ok(op_id, r.exit_code, channel);
        }
        Ok(r) => done_err(
            op_id,
            r.exit_code,
            classify_op_failure(r.exit_code, &r.stderr),
            channel,
        ),
        Err(e) => done_err(op_id, None, e.into_error(), channel),
    }
}

/// makepkg artifacts in the clone dir (stable sort for a deterministic
/// install list; pacman -U consumes all bundled packages at once).
fn find_built_packages(dir: &std::path::Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            let p = e.path();
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let archive = name.ends_with(".pkg.tar.zst")
                || name.ends_with(".pkg.tar.xz")
                || name.ends_with(".pkg.tar.gz");
            if archive && !name.ends_with(".sig") {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

// ---------------------------------------------------------------------------
// dispatch
// ---------------------------------------------------------------------------

/// Kick an operation off in the background; ownership of the channel passes to
/// the runner thread. Returns the op id immediately; a terminal `Done` event
/// is guaranteed exactly once per op.
pub fn dispatch_op(req: OpRequest, channel: Channel<OpMessage>, state: &OpsState) -> u64 {
    let op_id = state.next_id.fetch_add(1, Ordering::Relaxed) + 1;
    let cancel = Arc::new(AtomicBool::new(false));
    state.register(op_id, cancel.clone());

    let thread_cancel = cancel;
    std::thread::spawn(move || {
        run_op_with_flag(req, &thread_cancel, &channel, op_id);
        // terminal event sent → the flag entry is dead weight
    });
    op_id
}

fn run_op_with_flag(req: OpRequest, cancel: &AtomicBool, channel: &Channel<OpMessage>, op_id: u64) {
    match req {
        OpRequest::InstallRepo { name } => install_repo_flow(op_id, &name, channel),
        OpRequest::InstallAur { name } => install_aur_flow(op_id, &name, cancel, channel),
        OpRequest::Remove { name } => remove_flow(op_id, &name, channel),
        OpRequest::RemoveOrphans => remove_orphans_flow(op_id, channel),
        OpRequest::Upgrade => upgrade_flow(op_id, channel),
        OpRequest::Refresh => refresh_flow(op_id, channel),
        OpRequest::Clean { keep } => clean_flow(op_id, keep, channel),
        OpRequest::InstallFlatpak { app_id } => flatpak_flow(op_id, true, &app_id, channel),
        OpRequest::RemoveFlatpak { app_id } => flatpak_flow(op_id, false, &app_id, channel),
    }
}

/// Remove the cancel-flag entry once the UI has consumed the terminal event.
pub fn drop_op(state: &OpsState, op_id: u64) {
    state.unregister(op_id);
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_maps_pkexec_auth_and_lock() {
        let g = classify_op_failure(
            Some(255),
            "Sorry, request dismissed by user authentication\n",
        );
        assert_eq!(g.code, "auth_dismissed");
        assert!(g.hint.is_some());

        let lock = classify_op_failure(
            Some(255),
            "error: could not lock database: database is locked\n",
        );
        assert_eq!(lock.code, "db_locked");

        let net = classify_op_failure(Some(255), "failed to synchronize database 'core'\n");
        assert_eq!(net.code, "network_failure");

        let deps = classify_op_failure(Some(255), "error: could not satisfy dependencies\n");
        assert_eq!(deps.code, "unresolvable_deps");

        let missing = classify_op_failure(Some(255), "error: target not found: nope-pkg\n");
        assert_eq!(missing.code, "package_not_found");
    }

    #[test]
    fn classify_generic_failure_carries_last_line() {
        let g = classify_op_failure(Some(1), "warning: x\nerror: y-thing happened\n");
        assert_eq!(g.code, "op_failed");
        assert_eq!(g.message, "error: y-thing happened");
        assert_eq!(
            g.hint.as_deref(),
            Some("exit code 1 — see the operation journal")
        );
    }

    #[test]
    fn op_request_deserialises_wire_tags() {
        let req: OpRequest =
            serde_json::from_str(r#"{"kind":"install_repo","args":{"name":"htop"}}"#).unwrap();
        assert!(matches!(req, OpRequest::InstallRepo { name } if name == "htop"));

        let req: OpRequest = serde_json::from_str(r#"{"kind":"clean","args":{"keep":2}}"#).unwrap();
        assert!(matches!(req, OpRequest::Clean { keep: 2 }));

        let req: OpRequest = serde_json::from_str(r#"{"kind":"upgrade"}"#).unwrap();
        assert!(matches!(req, OpRequest::Upgrade));

        let req: OpRequest =
            serde_json::from_str(r#"{"kind":"remove","args":{"name":"vim"}}"#).unwrap();
        assert!(matches!(req, OpRequest::Remove { name } if name == "vim"));
    }

    #[test]
    fn validate_name_rejects_traversal_and_empty() {
        assert!(validate_name("").is_err());
        assert!(validate_name("  ").is_err());
        assert!(validate_name("a/b").is_err());
        assert!(validate_name("..").is_err());
        assert!(validate_name("htop").is_ok());
    }

    #[test]
    fn find_built_packages_filters_archives_only() {
        let dir = std::env::temp_dir().join(format!("pakmap-ops-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("foo-1.0-1-x86_64.pkg.tar.zst"), b"x").unwrap();
        std::fs::write(dir.join("foo-1.0-1-x86_64.pkg.tar.zst.sig"), b"x").unwrap();
        std::fs::write(dir.join("PKGBUILD"), b"x").unwrap();
        let found = find_built_packages(&dir);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(found.len(), 1);
        assert!(
            found[0]
                .file_name()
                .unwrap()
                .to_string_lossy()
                .contains(".pkg.tar.zst")
        );
    }
}
