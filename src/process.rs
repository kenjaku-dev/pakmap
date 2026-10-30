//! Bounded subprocess runner (perf P2: bound every background wait).
//!
//! `resilience-failure`: an unbounded `Command::wait` is how a stuck child
//! hangs the UI/CLI forever. Every background spawn in pakmap goes through
//! here: poll `try_wait()` and `kill()` on expiry so no caller blocks past
//! its budget. Returns `None` on timeout/spawn failure — callers map that to
//! their fail-open empty or install-hint error, never a panic.
//!
//! Deliberately unbounded (documented, user present with Ctrl-C):
//! interactive mutate runs with inherited stdio (`sudo pacman -S/-R/-Sy/-Syu`,
//! `makepkg -si`, `flatpak install/remove/update`) and pager/display tools.
//!
//! No `expect()`/`unwrap()`.

use std::io::Read;
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::time::{Duration, Instant};

/// Local probes and queries (pacman -Q, -Qi, -Qdt, git diff, flatpak info).
pub const QUICK_TIMEOUT: Duration = Duration::from_secs(15);
/// Network-backed scans (pacman -Ss, paccache -d, flatpak search/list).
pub const NETWORK_TIMEOUT: Duration = Duration::from_secs(30);
/// Bulk transfers with user-visible progress (git clone/pull --ff-only).
pub const CLONE_TIMEOUT: Duration = Duration::from_secs(300);

/// Poll interval for `try_wait`. 50ms keeps timeout precision tight without
/// spin cost (20 wakeups/sec worst case, only while a child runs).
const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Per-stream retained bytes (resilience-failure bulkhead): a runaway child
/// (`journalctl`, a chatty build) can't balloon RSS. Past the cap, draining
/// CONTINUES but bytes are discarded — the child's pipe stays unblocked, only
/// the returned snapshot is truncated.
pub const PIPE_CAP_BYTES: usize = 8 * 1024 * 1024;

/// Grace for drain threads after the child exits: a fast child whose last
/// pipe bytes are still in flight gets this window to finish; a stalled
/// one is abandoned (its thread exits at EOF — replaying `wait_bounded`'s
/// kill-and-reap means EOF always comes). Bounded below by `wait` itself.
const DRAIN_GRACE: Duration = Duration::from_secs(2);

/// Drain read size — pipe-friendly, no partial-buffer churn.
const DRAIN_CHUNK: usize = 32 * 1024;

/// True when `name` resolves to an executable file on PATH.
/// Pure-Rust `which`: never shells out to an external `which` binary, which
/// is absent from minimal containers (and would make every probe a false
/// negative there). Rejects empty names, paths and NULs; never panics.
pub fn has_tool(name: &str) -> bool {
    let name = name.trim();
    if name.is_empty() || name.contains('/') || name.contains('\0') {
        return false;
    }
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&paths).any(|dir| is_executable_file(&dir.join(name)))
}

fn is_executable_file(path: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.is_file()
        && path
            .metadata()
            .map(|m| m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
}

/// Wait up to `timeout`, killing + reaping the child on expiry (no zombies,
/// no orphaned writers). `Some(status)` also covers immediate spawn errors
/// as `None` via the caller.
fn wait_bounded(child: &mut Child, timeout: Duration) -> Option<ExitStatus> {
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status),
            Ok(None) => {}
            Err(_) => return None,
        }
        if start.elapsed() >= timeout {
            // Best-effort kill, then reap so no zombie remains.
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

/// Pump one captured stream to EOF into the drain channel. Runs on its own
/// thread for the whole child lifetime — this is what unblocks writers. The
/// old post-exit drain let any child emitting more than the ~64KiB pipe
/// buffer deadlock against `write()` until its timeout killed it.
fn drain_stream<R: Read>(
    name: &'static str,
    mut stream: R,
    done: std::sync::mpsc::Sender<(&'static str, Vec<u8>)>,
) {
    let mut buf = Vec::new();
    let mut chunk = [0u8; DRAIN_CHUNK];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break, // EOF
            Ok(n) => {
                if buf.len() < PIPE_CAP_BYTES {
                    let keep = n.min(PIPE_CAP_BYTES - buf.len());
                    buf.extend_from_slice(&chunk[..keep]);
                } // past the cap: keep the pipe drained, drop the bytes
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            // Read error (rare): ship what we have — captured output is
            // best-effort by contract (the old code ignored it entirely).
            Err(_) => break,
        }
    }
    // The receiver may be gone (drain grace expired upstream): sending into
    // a closed channel is how abandon-first drain threads exit cleanly.
    let _ = done.send((name, buf));
}

/// Captured-output spawn with stdin nulled (background child must never block
/// on input). `None` = spawn failure or timeout (child killed + reaped).
pub fn output_bounded(cmd: &str, args: &[&str], timeout: Duration) -> Option<Output> {
    let mut child = Command::new(cmd)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    // Drain while we wait (resilience-failure): the child must never block on
    // a full pipe because the parent is waiting on its exit — that turned a
    // query into the full timeout. Each captured stream gets one drainer
    // thread; only successfully started drainers are awaited below (spawn
    // failure degrades to losing that stream's capture, never to a hang).
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let mut drainers = 0usize;
    let tx_err = done_tx.clone();
    if let Some(err) = child.stderr.take()
        && std::thread::Builder::new()
            .name("pakmap-drain-stderr".to_string())
            .spawn(move || drain_stream("stderr", err, tx_err))
            .is_ok()
    {
        drainers += 1;
    }
    if let Some(out) = child.stdout.take()
        && std::thread::Builder::new()
            .name("pakmap-drain-stdout".to_string())
            .spawn({
                let tx_out = done_tx.clone();
                move || drain_stream("stdout", out, tx_out)
            })
            .is_ok()
    {
        drainers += 1;
    }
    drop(done_tx);
    let status = wait_bounded(&mut child, timeout)?;
    // EOF may lag the child's exit by a heartbeat; grant the drain threads a
    // bounded grace, then take whatever arrived. Worst case: a truncated
    // snapshot — strictly better than the old wait-deadlift (and unknown to
    // callers, who already treat output as best-effort).
    let deadline = Instant::now() + DRAIN_GRACE;
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    for _ in 0..drainers {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        match done_rx.recv_timeout(left) {
            Ok(("stdout", buf)) => stdout = buf,
            Ok(("stderr", buf)) => stderr = buf,
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(_) => break,
        }
    }
    Some(Output {
        status,
        stdout,
        stderr,
    })
}

/// Inherited-stdio spawn (user sees progress / password prompts).
/// `None` = spawn failure or timeout (child killed + reaped).
pub fn status_inherit_bounded(cmd: &str, args: &[&str], timeout: Duration) -> Option<ExitStatus> {
    let mut child = Command::new(cmd)
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .ok()?;
    wait_bounded(&mut child, timeout)
}

/// Silent spawn returning only the exit status (background probes like
/// `git fetch --quiet`, `flatpak info`). `None` = spawn failure or timeout.
pub fn status_null_bounded(cmd: &str, args: &[&str], timeout: Duration) -> Option<ExitStatus> {
    let mut child = Command::new(cmd)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    wait_bounded(&mut child, timeout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn true_is_some_success() {
        let out = output_bounded("true", &[], Duration::from_secs(5)).unwrap();
        assert!(out.status.success());
    }

    #[test]
    fn false_is_some_failure() {
        let out = output_bounded("false", &[], Duration::from_secs(5)).unwrap();
        assert!(!out.status.success());
    }

    #[test]
    fn missing_binary_is_none() {
        assert!(
            output_bounded("definitely-not-a-binary-xyz", &[], Duration::from_secs(5)).is_none()
        );
        assert!(
            status_inherit_bounded("definitely-not-a-binary-xyz", &[], Duration::from_secs(5))
                .is_none()
        );
    }

    #[test]
    fn sleep_hits_timeout_fast() {
        // 30s sleep with 200ms budget must return None in well under 5s —
        // and reap the child (no hang, no zombie).
        let start = Instant::now();
        assert!(output_bounded("sleep", &["30"], Duration::from_millis(200)).is_none());
        assert!(start.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn stdout_is_captured() {
        let out = output_bounded("echo", &["hello"], Duration::from_secs(5)).unwrap();
        assert!(String::from_utf8_lossy(&out.stdout).contains("hello"));
    }

    #[test]
    fn inherit_true_is_some() {
        let st = status_inherit_bounded("true", &[], Duration::from_secs(5)).unwrap();
        assert!(st.success());
    }

    #[test]
    fn has_tool_finds_shell_rejects_garbage() {
        assert!(has_tool("sh"));
        assert!(!has_tool("definitely-not-a-binary-xyz"));
        assert!(!has_tool(""));
        assert!(!has_tool("   "));
        assert!(!has_tool("a/b"));
        assert!(!has_tool("foo\0bar"));
    }

    #[test]
    fn big_stdout_is_drained_not_deadlocked() {
        // 300 KiB through the pipe — the exact shape that deadlocked the
        // pre-drain code (>64KiB pipe buffer: child blocked on write() while
        // the parent waited for its exit, so every such probe ate its full
        // timeout before the kill).
        let start = Instant::now();
        let out = output_bounded(
            "sh",
            &[
                "-c",
                "dd if=/dev/zero bs=1024 count=300 2>/dev/null; echo MARK",
            ],
            Duration::from_secs(10),
        )
        .unwrap();
        assert_eq!(out.stdout.len(), 300 * 1024 + 5);
        assert!(String::from_utf8_lossy(&out.stdout).ends_with("MARK\n"));
        assert!(out.status.success());
        assert!(start.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn big_stderr_is_drained_not_deadlocked() {
        let out = output_bounded(
            "sh",
            &["-c", "head -c 300000 /dev/zero 1>&2; echo ERRMARK 1>&2"],
            Duration::from_secs(10),
        )
        .unwrap();
        assert_eq!(out.stderr.len(), 300_000 + 8);
        assert!(out.stdout.is_empty());
        assert!(String::from_utf8_lossy(&out.stderr).ends_with("ERRMARK\n"));
    }

    #[test]
    fn runaway_stdout_is_capped_but_child_completes() {
        // 20 MiB against the 8 MiB retention cap: draining must CONTINUE past
        // the cap so the child finishes on its own (exit 0), with only the
        // capped window retained.
        let out = output_bounded(
            "dd",
            &["if=/dev/zero", "bs=1M", "count=20"],
            Duration::from_secs(30),
        )
        .unwrap();
        assert_eq!(out.stdout.len(), PIPE_CAP_BYTES);
        assert!(out.status.success());
    }
}
