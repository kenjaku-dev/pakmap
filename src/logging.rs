//! Logging config resolution (the Debug group's application layer).
//!
//! Layering (explicit beats ambient, ambient beats file): `RUST_LOG` env
//! wins when set, then the `--verbose` ladder (the CLI's explicit intent),
//! then `debug.log_level` (the config file). An unknown configured level
//! fails open to "warn" — a hand-edited config must never crash logging
//! (same contract as `parse_style`).
//!
//! `debug.log_file` is the append target for ALL surfaces; set, the TUI
//! can finally log — a file never touches the alt screen the way stderr
//! would. Unwritable paths degrade to `None` (each surface then uses its
//! default writer), never a boot failure.

use tracing_subscriber::{EnvFilter, fmt};

/// The `debug.log_level` spellings (mirrors the catalog's
/// `LOG_LEVEL_VALUES`; kept literal here so the resolver stays a pure
/// function with no catalog dependency).
const LOG_LEVELS: [&str; 6] = ["off", "error", "warn", "info", "debug", "trace"];

/// The resolved `EnvFilter` directive string: `RUST_LOG` when set (non-
/// blank), else the `--verbose` ladder (1=info, 2=debug, 3+=trace), else
/// the configured level (unknown → "warn", fail-open).
pub fn resolve_log_directive(configured: &str, verbose: u8, rust_log: Option<&str>) -> String {
    if let Some(rl) = rust_log.map(str::trim).filter(|s| !s.is_empty()) {
        return rl.to_string();
    }
    match verbose {
        0 => {
            let t = configured.trim();
            if LOG_LEVELS.contains(&t) {
                t.to_string()
            } else {
                "warn".to_string()
            }
        }
        1 => "info".to_string(),
        2 => "debug".to_string(),
        _ => "trace".to_string(),
    }
}

/// Open the configured log file for appending (create-along; empty/None =
/// no file). `None` = use the surface's default writer. An open failure
/// (missing parent, permissions) degrades to `None`, never a boot failure.
pub fn open_log_file(path: Option<&str>) -> Option<std::fs::File> {
    let p = path?.trim();
    if p.is_empty() {
        return None;
    }
    std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(p)
        .ok()
}

/// TUI-boot logging: NEVER stderr (the alt screen owns it). A subscriber
/// exists only when `debug.log_file` is set — appending to the file with
/// the resolved filter. Best-effort: open/init failures stay silent (a
/// TUI boot must not print). Returns whether a subscriber was installed
/// (global state: at most one per process, like every tracing init).
pub fn init_tui_logging(cfg: &crate::config::Config, verbose: u8, rust_log: Option<&str>) -> bool {
    let Some(file) = open_log_file(cfg.debug.log_file.as_deref()) else {
        return false;
    };
    let filter = EnvFilter::new(resolve_log_directive(
        &cfg.debug.log_level,
        verbose,
        rust_log,
    ));
    fmt()
        .with_env_filter(filter)
        .with_target(false)
        .without_time()
        .with_writer(file)
        .try_init()
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_log_env_wins_over_everything() {
        assert_eq!(resolve_log_directive("warn", 2, Some("trace")), "trace");
        assert_eq!(resolve_log_directive("debug", 0, Some("off")), "off");
    }

    #[test]
    fn empty_or_blank_env_is_ignored() {
        assert_eq!(resolve_log_directive("debug", 0, Some("")), "debug");
        assert_eq!(resolve_log_directive("debug", 0, Some("   ")), "debug");
    }

    #[test]
    fn verbose_ladder_overrides_config() {
        assert_eq!(resolve_log_directive("trace", 1, None), "info");
        assert_eq!(resolve_log_directive("trace", 2, None), "debug");
        assert_eq!(resolve_log_directive("trace", 3, None), "trace");
        assert_eq!(resolve_log_directive("trace", 9, None), "trace");
    }

    #[test]
    fn config_value_is_the_default_layer() {
        assert_eq!(resolve_log_directive("debug", 0, None), "debug");
        assert_eq!(resolve_log_directive("off", 0, None), "off");
        assert_eq!(resolve_log_directive("warn", 0, None), "warn");
    }

    #[test]
    fn unknown_config_level_fails_open_to_warn() {
        assert_eq!(resolve_log_directive("banana", 0, None), "warn");
        assert_eq!(resolve_log_directive("", 0, None), "warn");
        assert_eq!(resolve_log_directive("INFO", 0, None), "warn");
    }

    #[test]
    fn log_file_opens_append_and_none_when_unset() {
        assert!(open_log_file(None).is_none());
        assert!(open_log_file(Some("")).is_none());
        assert!(open_log_file(Some("   ")).is_none());
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pakmap.log");
        drop(open_log_file(Some(path.to_str().unwrap())).unwrap());
        assert!(path.exists());
        // append semantics: a second session keeps prior content
        std::fs::write(&path, b"old").unwrap();
        drop(open_log_file(Some(path.to_str().unwrap())).unwrap());
        assert_eq!(std::fs::read(&path).unwrap(), b"old");
    }

    #[test]
    fn log_file_unwritable_parent_degrades_to_none() {
        assert!(open_log_file(Some("/nonexistent-dir-xyz/pakmap.log")).is_none());
    }
}
