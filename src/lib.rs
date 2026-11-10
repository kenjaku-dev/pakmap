#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
//! pakmap library: search backends, install runners, TUI state.

/// Version single-source mirror: the published version lives in the root
/// `[package]` (AGENTS.md's single-source rule) — this const is how OTHER
/// workspace members (pakmap-gui) display it without carrying their own
/// stale crate field around.
pub const PAKMAP_VERSION: &str = env!("CARGO_PKG_VERSION");

pub mod cli;
pub mod config;
pub mod confirm;
pub mod diff;
pub mod error;
pub mod info;
pub mod install;
pub mod model;
pub mod news;
pub mod orphans;
pub mod output;
pub mod process;
pub mod safety;
pub mod search;
pub mod settings;
pub mod sources;
pub mod stats;
pub mod timing;
pub mod tui;
pub mod updates;

/// House dead-code gate (v0.10.2 phase 1): `#[allow(dead_code)]` is banned
/// as an escape hatch for prod code. Exactly ONE escape exists on purpose —
/// the documented test-only const in `ui/chrome.rs` whose value rides the
/// frame cost calc and therefore cannot travel to a test file. Any second
/// escape must come with an honest edit to this gate, never silently.
#[cfg(test)]
mod dead_code_gate {
    use std::path::{Path, PathBuf};

    fn rs_files_under(dir: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(d) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&d) else {
                continue;
            };
            for e in entries.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().is_some_and(|x| x == "rs") {
                    out.push(p);
                }
            }
        }
        out
    }

    /// The banned shape is COMPOSED at runtime so this gate never matches
    /// its own comment/source text (house pattern from the spawn-guard gate).
    #[test]
    fn allow_dead_code_escapes_stay_at_exactly_one() {
        let pattern = format!("allow({}_code)", "dead");
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut total = 0usize;
        let mut hits: Vec<PathBuf> = Vec::new();
        for f in rs_files_under(&root) {
            let text = std::fs::read_to_string(&f).unwrap();
            let n = text
                .lines()
                .filter(|l| {
                    let t = l.trim_start();
                    !t.starts_with("//")
                })
                .filter(|l| l.contains(&pattern))
                .count();
            if n > 0 {
                hits.push(f);
                total += n;
            }
        }
        assert_eq!(total, 1, "{pattern} escape count moved: {hits:?}");
        assert!(
            hits[0].ends_with("src/tui/ui/chrome.rs"),
            "the one escape must remain the documented chrome.rs const, got {hits:?}"
        );
    }
}
