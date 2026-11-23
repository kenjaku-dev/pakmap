#![allow(clippy::unwrap_used, clippy::expect_used)]
//! TUI golden snapshots (0.12, TUI phase 1 — accessibility/ui-design).
//!
//! Each screen is rendered through `TestBackend` and dumped as a trimmed text
//! grid plus an FNV-1a hash over every cell's symbol + fg/bg/underline/modifier
//! encoding (the same fingerprint vocabulary as the settings-modal pin). The
//! `.txt` golden therefore guards BOTH layout/copy and styling. Regenerate
//! deliberately with:
//!
//! ```text
//! UPDATE_TUI_SNAPSHOTS=1 cargo test --test tui_golden
//! ```
//!
//! Screens are hermetic: `no_color` is set explicitly, no staged legs (the
//! wall-clock spinner never enters a golden), and `is_loading` is false.

use pakmap::config::Config;
use pakmap::model::Package;
use pakmap::tui::app::{App, Focus, Mode, Popup};
use pakmap::tui::settings::SettingsModal;
use ratatui::{Terminal, backend::TestBackend};
use std::fs;
use std::path::PathBuf;

fn fnv1a(data: &[u8], mut hash: u64) -> u64 {
    for b in data {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    hash
}

fn sample_packages() -> Vec<Package> {
    vec![
        Package {
            name: "firefox".into(),
            version: "123.0-1".into(),
            description: Some("Standalone web browser".into()),
            repo: "extra".into(),
            arch: Some("x86_64".into()),
            url: Some("https://firefox.example".into()),
            installed: false,
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
        },
        Package {
            name: "firefox-bin".into(),
            version: "123.0-1".into(),
            description: Some("Binary Firefox".into()),
            repo: "aur".into(),
            arch: None,
            url: Some("https://aur.example".into()),
            installed: false,
            votes: Some(1200),
            popularity: Some(5.2),
            out_of_date: None,
            maintainer: Some("someone".into()),
            num_votes: Some(1200),
            last_modified: Some(1700000000),
            depends: None,
            optdepends: None,
            required_by: None,
            optional_for: None,
            reason: None,
            orphan: false,
        },
        Package::minimal(
            "org.mozilla.firefox".into(),
            "158.0".into(),
            "flatpak".into(),
        ),
    ]
}

fn sample_installed() -> Vec<Package> {
    vec![Package {
        name: "vim".into(),
        version: "9.1-1".into(),
        description: Some("Vi improved".into()),
        repo: "local".into(),
        arch: Some("x86_64".into()),
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
        reason: Some("explicit".into()),
        orphan: false,
    }]
}

fn base(query: &str) -> App {
    let mut app = App::new(query.into());
    app.no_color = false; // hermetic: NO_COLOR env must not shift a golden
    app.is_loading = false;
    app.legs = None;
    app
}

fn dump(app: &App, w: u16, h: u16) -> (String, u64) {
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal.draw(|f| pakmap::tui::ui::draw(f, app)).unwrap();
    let buf = terminal.backend().buffer();
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    let mut text = String::new();
    for y in 0..h {
        let mut line = String::new();
        for x in 0..w {
            let c = &buf[(x, y)];
            let encoding = format!(
                "{:?}|{:?}|{:?}|{:?}|{:?}",
                c.symbol(),
                c.fg,
                c.bg,
                c.underline_color,
                c.modifier
            );
            hash = fnv1a(encoding.as_bytes(), hash);
            line.push_str(c.symbol());
        }
        text.push_str(line.trim_end());
        text.push('\n');
    }
    (text, hash)
}

fn golden(name: &str, app: &App, w: u16, h: u16) {
    let (text, hash) = dump(app, w, h);
    let want = format!("# pakmap-tui-golden v1 hash={hash:016x}\n{text}");
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/snapshots/tui")
        .join(format!("{name}.txt"));
    if std::env::var_os("UPDATE_TUI_SNAPSHOTS").is_some() {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, &want).unwrap();
        return;
    }
    let got = fs::read_to_string(&path).unwrap_or_else(|_| {
        panic!(
            "missing golden `{name}` — generate with \
             UPDATE_TUI_SNAPSHOTS=1 cargo test --test tui_golden"
        )
    });
    assert_eq!(
        got, want,
        "golden `{name}` drifted (layout/copy or styling). If intentional, \
         re-pin with UPDATE_TUI_SNAPSHOTS=1 cargo test --test tui_golden"
    );
}

#[test]
fn golden_search_empty_60x14() {
    let mut app = base("");
    app.focus = Focus::Search;
    golden("search_empty_60x14", &app, 60, 14);
}

#[test]
fn golden_search_results_80x24() {
    let mut app = base("firefox");
    app.packages = sample_packages();
    app.list_state.select(Some(0));
    app.focus = Focus::List;
    app.status = "Found 3 (repo 1 aur 1 flatpak 1)".into();
    golden("search_results_80x24", &app, 80, 24);
}

#[test]
fn golden_installed_80x24() {
    let mut app = base("");
    app.mode = Mode::Installed;
    app.packages = sample_installed();
    app.list_state.select(Some(0));
    app.focus = Focus::List;
    app.status = "Installed 1 (local 1)".into();
    golden("installed_80x24", &app, 80, 24);
}

#[test]
fn golden_updates_80x24() {
    let mut app = base("");
    app.mode = Mode::Updates;
    app.packages = sample_packages();
    app.list_state.select(Some(0));
    app.focus = Focus::List;
    app.status = "Updates 3 (repo 1 aur 1 flatpak 1)".into();
    golden("updates_80x24", &app, 80, 24);
}

#[test]
fn golden_search_preview_120x30() {
    let mut app = base("firefox");
    app.packages = sample_packages();
    app.list_state.select(Some(0));
    app.focus = Focus::List;
    app.config.tui.preview_pane = true;
    app.status = "Found 3 (repo 1 aur 1 flatpak 1)".into();
    golden("search_preview_120x30", &app, 120, 30);
}

#[test]
fn golden_search_wide_200x50() {
    let mut app = base("firefox");
    app.packages = sample_packages();
    app.list_state.select(Some(0));
    app.focus = Focus::List;
    app.status = "Found 3 (repo 1 aur 1 flatpak 1)".into();
    golden("search_wide_200x50", &app, 200, 50);
}

#[test]
fn golden_help_80x24() {
    let mut app = base("");
    app.focus = Focus::List;
    app.popup = Popup::Help;
    golden("help_80x24", &app, 80, 24);
}

#[test]
fn golden_info_80x24() {
    let mut app = base("firefox");
    app.packages = sample_packages();
    app.popup = Popup::Info(sample_packages()[0].clone());
    golden("info_80x24", &app, 80, 24);
}

#[test]
fn golden_confirm_80x24() {
    let mut app = base("firefox");
    app.packages = sample_packages();
    app.popup = Popup::Confirm(sample_packages()[0].clone());
    golden("confirm_80x24", &app, 80, 24);
}

#[test]
fn golden_message_error_80x24() {
    let mut app = base("firefox");
    app.popup = Popup::Message(
        "Install failed: target not found: firefox-nonexistent — check the name or refresh the sync DBs (F5)".into(),
    );
    golden("message_error_80x24", &app, 80, 24);
}

#[test]
fn golden_settings_60x14() {
    let mut app = base("");
    app.focus = Focus::List;
    app.settings = Some(SettingsModal::new(Config::default(), Vec::new(), None));
    golden("settings_60x14", &app, 60, 14);
}

#[test]
fn golden_no_color_ascii_80x24() {
    let mut app = base("");
    app.no_color = true;
    app.config.tui.icons = "ascii".into();
    app.mode = Mode::Installed;
    app.packages = sample_installed();
    app.list_state.select(Some(0));
    app.focus = Focus::List;
    app.status = "Installed 1 (local 1)".into();
    golden("no_color_ascii_80x24", &app, 80, 24);
}
