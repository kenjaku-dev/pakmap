//! Command execution-context drift gate (v0.11.0).
//!
//! Tauri runs synchronous commands on the main thread. The Updates / News /
//! Stats commands do network + libalpm + flatpak work, so they must be async
//! and move the blocking body onto the blocking pool — otherwise the webview
//! freezes for the duration of the AUR timeout.
//!
//! The mock-gateway e2e cannot observe the real Tauri main thread (it runs the
//! frontend in a plain browser against `mockIpc.ts`), so this source-level gate
//! is the regression lock. It follows the core crate's `include_str!` drift-gate
//! pattern: red on the old shape, green on the fixed one.

const LIB: &str = include_str!("../src/lib.rs");
const PKGBUILD: &str = include_str!("../src/pkgbuild.rs");

/// The command body from its declaration to the next `#[tauri::command]`.
fn body_of<'a>(src: &'a str, marker: &str) -> &'a str {
    let start = src
        .find(marker)
        .unwrap_or_else(|| panic!("missing `{marker}` in the source"));
    let rest = &src[start + marker.len()..];
    let end = rest.find("#[tauri::command]").unwrap_or(rest.len());
    &rest[..end]
}

#[test]
fn updates_news_stats_are_async_and_offload_blocking_work() {
    for name in ["get_updates", "get_stats", "get_news", "package_info"] {
        let marker = format!("async fn {name}");
        let body = body_of(LIB, &marker);
        assert!(
            body.contains("spawn_blocking"),
            "{name} must offload its blocking body with spawn_blocking"
        );
    }
}

#[test]
fn pkgbuild_reference_pattern_stays_async_and_offloads() {
    let body = body_of(PKGBUILD, "async fn get_pkgbuild");
    assert!(body.contains("spawn_blocking"));
}

#[test]
fn boot_and_dispatch_commands_stay_fast_sync() {
    for name in [
        "greet",
        "get_config_summary",
        "get_config_flags",
        "search",
        "run_op",
        "main_window_ready",
        "maintenance_note",
        "open_package_url",
        "cancel_search",
        "cancel_operation",
        "drop_operation",
    ] {
        let marker = format!("fn {name}");
        let idx = LIB
            .find(&marker)
            .unwrap_or_else(|| panic!("missing `{marker}` in lib.rs"));
        let before = &LIB[..idx];
        let attr = before
            .rfind("#[tauri::command]")
            .unwrap_or_else(|| panic!("{name} is not a tauri command"));
        let between = &before[attr + "#[tauri::command]".len()..];
        assert!(
            !between.contains("async"),
            "{name} is a fast/boot/dispatch command and must stay synchronous"
        );
    }
}
