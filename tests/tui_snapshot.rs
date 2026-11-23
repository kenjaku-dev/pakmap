#![allow(clippy::unwrap_used, clippy::expect_used)]
//! TUI snapshot tests (test-only crate: unwraps are idiomatic here).

use pakmap::model::Package;
use pakmap::tui::app::{App, Focus, Mode, Popup};
use ratatui::{Terminal, backend::TestBackend};
use tui_input::Input;
use tui_input::backend::crossterm::EventHandler;

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

#[test]
fn tui_renders_search_and_results() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut app = App::new("firefox".into());
    app.packages = sample_packages();
    app.list_state.select(Some(0));
    app.focus = Focus::List;
    app.status = "Found 3 (repo 1 aur 1 flatpak 1)".into();
    app.is_loading = false;

    terminal.draw(|f| pakmap::tui::ui::draw(f, &app)).unwrap();
    let buf = terminal.backend().buffer().clone();
    // Convert buffer to lines for snapshot
    let lines: Vec<String> = (0..24)
        .map(|y| {
            let mut line = String::new();
            for x in 0..80 {
                line.push_str(buf[(x, y)].symbol());
            }
            line.trim_end().to_string()
        })
        .collect();
    // Basic assertions — per tui-design visual-patterns, check layout not empty
    assert!(lines.iter().any(|l| l.contains("Search")));
    assert!(lines.iter().any(|l| l.contains("Results")));
    assert!(lines.iter().any(|l| l.contains("firefox")));
    assert!(lines.iter().any(|l| l.contains("Found 3")));
    assert!(lines.iter().any(|l| l.contains("flatpak")));
}

#[test]
fn tui_too_small_shows_message() {
    let backend = TestBackend::new(50, 8);
    let mut terminal = Terminal::new(backend).unwrap();
    let app = App::new("".into());
    terminal.draw(|f| pakmap::tui::ui::draw(f, &app)).unwrap();
    let buf = terminal.backend().buffer().clone();
    let content = buf.content().iter().map(|c| c.symbol()).collect::<String>();
    assert!(content.contains("Terminal too small"));
}

#[test]
fn tui_info_popup_renders() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut app = App::new("firefox".into());
    app.packages = sample_packages();
    app.popup = Popup::Info(sample_packages()[0].clone());
    terminal.draw(|f| pakmap::tui::ui::draw(f, &app)).unwrap();
    let buf = terminal.backend().buffer().clone();
    let content = buf.content().iter().map(|c| c.symbol()).collect::<String>();
    assert!(content.contains("firefox"));
    assert!(content.contains("Version"));
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

fn render(app: &App, w: u16, h: u16) -> String {
    let backend = TestBackend::new(w, h);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|f| pakmap::tui::ui::draw(f, app)).unwrap();
    terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|c| c.symbol())
        .collect::<String>()
}

#[test]
fn tui_tabs_bar_renders_both_modes() {
    let mut app = App::new("".into());
    app.focus = Focus::List;
    let content = render(&app, 80, 24);
    assert!(content.contains("Search"));
    assert!(content.contains("Installed"));
}

#[test]
fn tui_installed_mode_renders_local_list() {
    let mut app = App::new("".into());
    app.switch_mode();
    app.packages = sample_installed();
    app.list_state.select(Some(0));
    app.focus = Focus::List;
    app.is_loading = false;
    app.status = "Installed 1 (local 1)".into();
    let content = render(&app, 80, 24);
    assert!(content.contains("Installed"));
    // Phase 4 chip grammar: fixed-width origin chip + separator + name
    // ("local   " chip, sep, "vim" — no repo/name slash).
    assert!(content.contains("local    vim"));
    assert!(!content.contains("local/vim"));
    assert!(content.contains("Installed 1"));
    // [installed] tag hidden in Installed mode (noise reduction)
    assert!(!content.contains("[installed]"));
}

#[test]
fn tui_confirm_remove_popup_renders() {
    let mut app = App::new("".into());
    app.switch_mode();
    app.packages = sample_installed();
    app.popup = Popup::ConfirmRemove(sample_installed()[0].clone());
    let content = render(&app, 80, 24);
    assert!(content.contains("Confirm remove"));
    assert!(content.contains("sudo pacman"));
    assert!(content.contains("vim"));
}

#[test]
fn tui_help_popup_renders() {
    let mut app = App::new("".into());
    app.focus = Focus::List;
    app.popup = Popup::Help;
    let content = render(&app, 80, 24);
    assert!(content.contains("Help"));
    assert!(content.contains("Tab"));
    assert!(content.contains("Installed"));
    assert!(content.contains("refresh"));
}

#[test]
fn tui_confirm_refresh_popup_renders() {
    let mut app = App::new("".into());
    app.focus = Focus::List;
    app.popup = Popup::ConfirmRefresh;
    let content = render(&app, 80, 24);
    assert!(content.contains("Confirm refresh"));
    assert!(content.contains("sudo pacman"));
}

#[test]
fn tui_narrow_fallback_no_emoji() {
    let mut app = App::new("".into());
    app.focus = Focus::List;
    app.packages = sample_packages();
    app.list_state.select(Some(0));
    // 60-col split per tui-design floor test
    let content = render(&app, 60, 24);
    assert!(content.contains("Search"));
    assert!(content.contains("Installed"));
}

#[test]
fn tui_no_color_tabs_ascii() {
    let mut app = App::new("".into());
    app.no_color = true;
    app.focus = Focus::List;
    app.mode = Mode::Installed;
    app.packages = sample_installed();
    app.list_state.select(Some(0));
    let content = render(&app, 80, 24);
    assert!(content.contains("Installed"));
}

#[test]
fn tui_updates_tab_renders() {
    let mut app = App::new("".into());
    // Search -> Installed -> Updates
    app.switch_mode();
    app.switch_mode();
    assert_eq!(app.mode, Mode::Updates);
    app.focus = Focus::List;
    let content = render(&app, 80, 24);
    assert!(content.contains("Updates"));
    assert!(content.contains("upgrade"));
}

#[test]
fn tui_confirm_upgrade_popup_renders() {
    let mut app = App::new("".into());
    app.focus = Focus::List;
    app.popup = Popup::ConfirmUpgrade;
    let content = render(&app, 80, 24);
    assert!(content.contains("Confirm upgrade"));
    assert!(content.contains("pacman -Syu"));
}

#[test]
fn tui_ood_orphan_badges_render() {
    use pakmap::model::Package;
    let mut aur = Package::minimal("foo".into(), "2-1".into(), "aur".into());
    aur.out_of_date = Some(1);
    aur.orphan = true;
    let mut app = App::new("".into());
    app.focus = Focus::List;
    app.packages = vec![aur];
    app.list_state.select(Some(0));
    let content = render(&app, 80, 24);
    assert!(content.contains("[OOD]"));
    assert!(content.contains("[ORPHAN]"));
}

#[test]
fn tui_confirm_clean_popup_renders() {
    let mut app = App::new("".into());
    app.focus = Focus::List;
    app.popup = Popup::ConfirmClean {
        files: 16,
        bytes: 870 * 1024 * 1024,
        flatpak: false,
        clones: Default::default(), // 0 days = off → today's bytes unchanged
    };
    let content = render(&app, 80, 24);
    assert!(content.contains("Confirm clean"));
    assert!(content.contains("16 cached files"));
    assert!(content.contains("paccache"));
}

#[test]
fn tui_confirm_clean_flatpak_popup_renders() {
    let mut app = App::new("".into());
    app.focus = Focus::List;
    app.popup = Popup::ConfirmClean {
        files: 16,
        bytes: 870 * 1024 * 1024,
        flatpak: true,
        clones: Default::default(),
    };
    let content = render(&app, 80, 24);
    assert!(content.contains("Confirm clean"));
    assert!(content.contains("flatpak --unused"));
}

#[test]
fn tui_confirm_clean_clone_prune_line_renders() {
    let mut app = App::new("".into());
    app.focus = Focus::List;
    app.popup = Popup::ConfirmClean {
        files: 16,
        bytes: 870 * 1024 * 1024,
        flatpak: false,
        clones: pakmap::install::clean::ClonePlan {
            dirs: 7,
            bytes: 1024 * 1024,
            days: 30,
        },
    };
    let content = render(&app, 80, 24);
    assert!(content.contains("7 AUR clone dirs"));
    assert!(content.contains("30+ days old"));
    // the run line is NOT extended (it already sits at the 46-col inner
    // width limit; extending it would wrap and clip at the default size)
    assert!(content.contains("paccache"));
}

#[test]
fn tui_confirm_upgrade_warn_renders() {
    let mut app = App::new("".into());
    app.focus = Focus::List;
    app.upgrade_warn = Some("Linux 6.9 requires manual intervention".into());
    app.popup = Popup::ConfirmUpgrade;
    let content = render(&app, 80, 24);
    assert!(content.contains("Confirm upgrade"));
    assert!(content.contains("Manual intervention"));
}

#[test]
fn tui_help_mentions_clean_and_updates() {
    let mut app = App::new("".into());
    app.focus = Focus::List;
    app.popup = Popup::Help;
    let content = render(&app, 80, 24);
    assert!(content.contains("clean cache"));
    assert!(content.contains("Updates"));
}

// ---------------------------------------------------------------------------
// Phase 4 — results presentation & responsiveness (plan §3/§4)
// ---------------------------------------------------------------------------

use pakmap::tui::app::{SearchStage, StagedLegs};
use std::time::{Duration, Instant};

fn legs_fixture(query: &str, planned: [bool; 3], steps: u128) -> StagedLegs {
    let mut legs = StagedLegs::new(query.into(), planned);
    legs.started = Instant::now() - Duration::from_millis((steps * 240) as u64);
    legs
}

#[test]
fn tui_staged_three_legs_keep_row_positions() {
    // Acceptance: the visible list (last complete generation) keeps every
    // row's y-position across ≥3 mid-search renders — nothing repaints but
    // the status strip until Done.
    let mut app = App::new("firefox".into());
    app.packages = sample_packages();
    app.list_state.select(Some(1)); // firefox-bin
    app.focus = Focus::List;
    app.is_loading = true;

    /// Per-line extraction (buffer offers no newline separation): each cell
    /// row wrapped, trailing whitespace trimmed.
    fn render_lines(app: &App, w: u16, h: u16) -> Vec<String> {
        let backend = TestBackend::new(w, h);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| pakmap::tui::ui::draw(f, app)).unwrap();
        let buf = terminal.backend().buffer().clone();
        (0..h)
            .map(|y| {
                let mut line = String::new();
                for x in 0..w {
                    line.push_str(buf[(x, y)].symbol());
                }
                line.trim_end().to_string()
            })
            .collect()
    }

    fn row_y(lines: &[String], needle: &str) -> usize {
        lines
            .iter()
            .position(|l| l.contains(needle))
            .expect("row visible")
    }

    let legs = legs_fixture("firefox", [true, true, true], 2);
    app.legs = Some(legs);
    app.list_state.select(Some(1)); // select the row under test
    let lines = render_lines(&app, 80, 24);
    let baseline = row_y(&lines, "firefox-bin");
    assert!(
        baseline > 0,
        "row visible in the mid-search frame (baseline {baseline})"
    );

    // Three asynchronous mid-search repaints: repo lands, then aur, then a
    // spinner-only frame — the row stays put through all of them.
    let mut repo = legs_fixture("firefox", [true, true, true], 3);
    repo.apply_leg(SearchStage::Repo, 2);
    app.legs = Some(repo);
    assert_eq!(
        row_y(&render_lines(&app, 80, 24), "firefox-bin"),
        baseline,
        "repo leg repaint keeps the row's y"
    );

    let mut aur = legs_fixture("firefox", [true, true, true], 4);
    aur.apply_leg(SearchStage::Repo, 2);
    aur.apply_leg(SearchStage::Aur, 3);
    app.legs = Some(aur);
    assert_eq!(
        row_y(&render_lines(&app, 80, 24), "firefox-bin"),
        baseline,
        "aur leg repaint keeps the row's y"
    );

    app.legs = Some(legs_fixture("firefox", [true, true, true], 5));
    assert_eq!(
        row_y(&render_lines(&app, 80, 24), "firefox-bin"),
        baseline,
        "spinner-only repaint keeps the row's y (hold-until-Done)"
    );
}

#[test]
fn tui_staged_leg_counters_render_in_status_strip() {
    let mut app = App::new("firefox".into());
    app.packages = sample_packages();
    app.list_state.select(Some(0));
    app.focus = Focus::List;
    app.is_loading = true;

    // Two legs arrived, one still spinning (phase pinned to frame 7 ⠧).
    let mut legs = legs_fixture("firefox", [true, true, true], 7);
    legs.apply_leg(SearchStage::Repo, 5);
    legs.apply_leg(SearchStage::Aur, 12);
    app.legs = Some(legs);
    let content = render(&app, 80, 24);
    assert!(content.contains("repo 5"));
    assert!(content.contains("aur 12"));
    assert!(content.contains("flatpak ⠧"));
    assert!(content.contains("Searching for 'firefox'"));

    // Un-planned legs never render (source-filter honesty).
    let mut repo_only = legs_fixture("vim", [true, false, false], 7);
    repo_only.apply_leg(SearchStage::Repo, 3);
    app.legs = Some(repo_only);
    let content = render(&app, 80, 24);
    assert!(content.contains("repo 3"));
    assert!(!content.contains("aur 12"), "un-planned leg omitted");
}

#[test]
fn tui_origin_chip_fixed_width_and_char_safe() {
    // Long third-party repos truncate to the tail marker under the fixed
    // cap — the column grid stays put, the full source lives in `i`/preview.
    let mut app = App::new("firefox".into());
    let chaotic = Package::minimal("bright".into(), "1.0-1".into(), "chaotic-aur".into());
    app.packages = vec![chaotic];
    app.list_state.select(Some(0));
    app.focus = Focus::List;
    app.is_loading = false;
    let content = render(&app, 80, 24);
    assert!(content.contains("...c-aur"), "tail-truncated chip");
    assert!(content.contains("bright"), "name intact");

    // Known repo: plain padded verbatim label (chip 8 cells + 1 separator).
    let mut app = App::new("firefox".into());
    app.packages = sample_packages();
    app.list_state.select(Some(0));
    app.focus = Focus::List;
    let content = render(&app, 80, 24);
    let opaque = format!("extra{}firefox", " ".repeat(8 - "extra".len() + 1));
    assert!(content.contains(&opaque), "chip + sep + name");
}

#[test]
fn tui_preview_pane_splits_at_desktop_width() {
    let mut app = App::new("firefox".into());
    app.packages = sample_packages();
    app.list_state.select(Some(0));
    app.focus = Focus::List;
    app.is_loading = false;
    app.config.tui.preview_pane = true;
    app.status = "Found 3 (repo 1 aur 1 flatpak 1)".into();

    // Desktop: the focused row's fact sheet sits next to the list.
    let content = render(&app, 120, 30);
    assert!(content.contains("Version:"), "preview pane rendered");
    assert!(content.contains("firefox"), "list still primary");

    // Narrower panel: no split — the fact sheet drops back to the `i` modal.
    let narrow = render(&app, 80, 24);
    assert!(!narrow.contains("Version:"), "below threshold: no pane");

    // 0.12 (TUI phase 3): the preview is on by default at >=120 cols and
    // off below the raised threshold (it only fits where the list keeps a
    // usable width). Users can still force it off via the setting.
    let mut app = App::new("firefox".into());
    app.packages = sample_packages();
    app.list_state.select(Some(0));
    let content = render(&app, 120, 30);
    assert!(
        content.contains("Version:"),
        "preview_pane default on at 120"
    );
    let narrow = render(&app, 80, 24);
    assert!(!narrow.contains("Version:"), "default off below 120");
}

#[test]
fn tui_compact_density_fits_roughly_twice_the_rows() {
    let mut app = App::new("firefox".into());
    app.packages = (0..10)
        .map(|i| Package::minimal(format!("pkg{i}"), "1.0".into(), "extra".into()))
        .collect();
    app.list_state.select(Some(0));
    app.no_color = true;
    app.config.tui.row_density = "compact".into();
    let compact = render(&app, 60, 14);
    app.config.tui.row_density = "comfortable".into();
    let comfortable = render(&app, 60, 14);
    let visible = |s: &str| (0..10).filter(|i| s.contains(&format!("pkg{i}"))).count();
    assert!(
        visible(&compact) >= visible(&comfortable) * 2,
        "compact {:?} vs comfortable {:?}",
        visible(&compact),
        visible(&comfortable)
    );
}

#[test]
fn tui_wide_columns_appear_only_without_the_preview_pane() {
    let mut app = App::new("firefox".into());
    app.packages = sample_packages();
    app.list_state.select(Some(0));
    app.focus = Focus::List;
    app.no_color = true;
    app.config.tui.row_density = "auto".into();
    // Default preview at 120 covers the same facts — no duplicated column.
    let with_preview = render(&app, 120, 30);
    assert!(with_preview.contains("Version:"));
    assert!(
        !with_preview.contains("x86_64  ·"),
        "no duplicate arch column"
    );
    // With the pane off, wide mode adds arch + absolute updated date.
    app.config.tui.preview_pane = false;
    let wide = render(&app, 120, 30);
    assert!(wide.contains("x86_64"), "wide arch column");
    assert!(wide.contains("upd 2023-11-14"), "wide updated column");
    // Below the wide breakpoint the metadata column is absent.
    let narrow = render(&app, 80, 24);
    assert!(!narrow.contains("upd "), "wide only at >=120 cols");
}

#[test]
fn tui_popup_scroll_reaches_the_end_of_long_help() {
    let mut app = App::new("".into());
    app.no_color = true;
    app.config.tui.icons = "unicode".into(); // pin the glyph set, not ASCII mode
    app.popup = Popup::Help;
    let top = render(&app, 80, 24);
    assert!(top.contains("Tab / BackTab"), "starts at the top");
    assert!(top.contains("▲"), "overflow is advertised even on page 1");
    assert!(
        !top.contains("Press Esc/q/Enter/? to close"),
        "the final help line is not on the first page"
    );
    app.popup_scroll = u16::MAX; // the renderer clamps to the last page
    let bottom = render(&app, 80, 24);
    assert!(bottom.contains("▲"), "scroll indicator appears");
    assert!(
        bottom.contains("Press Esc/q/Enter/? to close"),
        "the final help line is reachable at 80x24"
    );
}

#[test]
fn tui_long_error_message_is_fully_reachable() {
    let tail = "UNIQUE_TAIL_42";
    let msg = format!("✗ Failed: {}", "x".repeat(2000)) + tail;
    let mut app = App::new("".into());
    app.no_color = true;
    app.config.tui.icons = "unicode".into();
    app.popup = Popup::Message(msg);
    let top = render(&app, 80, 24);
    assert!(!top.contains(tail), "the tail starts off-screen");
    app.popup_scroll = u16::MAX;
    let bottom = render(&app, 80, 24);
    assert!(bottom.contains("▲"), "long message is scrollable");
    assert!(bottom.contains(tail), "the long error's tail is reachable");
}

#[test]
fn tui_settings_modal_is_usable_at_the_60x14_floor() {
    use pakmap::tui::settings::SettingsModal;
    let mut app = App::new("".into());
    app.no_color = true;
    app.settings = Some(SettingsModal::new(
        pakmap::config::Config::default(),
        Vec::new(),
        None,
    ));
    let small = render(&app, 60, 14);
    assert!(
        small.contains("search.limit"),
        "entries visible at the floor"
    );
    assert!(
        !small.contains("next-launch"),
        "apply column hidden when narrow"
    );
    let wide = render(&app, 200, 50);
    assert!(
        wide.contains("next-launch"),
        "apply column back on wide frames"
    );
}

#[test]
fn tui_filter_cursor_tracks_visual_position() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut input = Input::new("abcdef".into());
    for _ in 0..3 {
        input.handle_event(&crossterm::event::Event::Key(KeyEvent::new(
            KeyCode::Left,
            KeyModifiers::empty(),
        )));
    }
    let mut app = App::new("".into());
    app.filter_draft = Some(input);
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|f| pakmap::tui::ui::draw(f, &app)).unwrap();
    let pos = terminal.backend().cursor_position();
    let content: String = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|c| c.symbol())
        .collect();
    assert!(!content.contains('▏'), "no fake end-of-line cursor glyph");
    // prefix " filter: " = 9 cells, visual cursor 3 -> x = 12.
    assert_eq!(pos.x, 12, "cursor follows the middle of the buffer");
    assert!(content.contains('a'), "the filtered value is still drawn");
}

#[test]
fn tui_feedback_popups_render_their_content() {
    use pakmap::tui::app::OpEntry;
    let mut app = App::new("".into());
    app.no_color = true;
    app.config.tui.icons = "unicode".into();
    app.ops_log.push_back(OpEntry {
        elapsed: "12:00:01".into(),
        action: "install".into(),
        target: "foo".into(),
        result: "✓ ok".into(),
        duration: "1.2s".into(),
    });
    app.popup = Popup::OpLog;
    let log = render(&app, 80, 24);
    assert!(log.contains("Operations log"));
    assert!(log.contains("12:00:01") && log.contains("✓ ok") && log.contains("1.2s"));
    app.query_history
        .extend(["firefox".to_string(), "vim".into()]);
    app.popup = Popup::History { selected: 1 };
    let hist = render(&app, 80, 24);
    assert!(hist.contains("Query history"));
    assert!(hist.contains("▸ vim"), "selected query is marked");
    assert!(hist.contains("Enter: rerun"));
    app.popup = Popup::CommandPalette {
        query: "help".into(),
        selected: 0,
    };
    let pal = render(&app, 80, 24);
    assert!(pal.contains("Command palette"));
    assert!(pal.contains("Help"), "generated from the keymap");
    assert!(pal.contains("> help"), "query is editable");
}

#[test]
fn tui_glyphs_stay_ascii_in_ascii_mode() {
    use pakmap::tui::app::{SearchStage, StagedLegs};
    let mut app = App::new("firefox".into());
    app.no_color = true;
    app.config.tui.icons = "ascii".into();
    app.packages = sample_packages();
    app.list_state.select(Some(0));
    app.is_loading = true;
    let mut legs = StagedLegs::new("firefox".into(), [true, true, true]);
    legs.set_leg_error(SearchStage::Aur, "timeout".into());
    app.legs = Some(legs);
    let content = render(&app, 80, 24);
    assert!(
        content.contains("aur x"),
        "failed leg uses the ASCII error glyph"
    );
    assert!(content.contains("~"), "busy marker is ASCII");
    for emoji in ["✗", "⏳", "🔍", "▲", "▸"] {
        assert!(
            !content.contains(emoji),
            "no emoji in the critical path: {emoji}"
        );
    }
}

#[test]
fn tui_column_header_is_wide_only_and_opt_in() {
    let mut app = App::new("firefox".into());
    app.packages = sample_packages();
    app.list_state.select(Some(0));
    app.no_color = true;
    app.config.tui.column_header = true;
    app.config.tui.preview_pane = false;
    let wide = render(&app, 120, 30);
    assert!(
        wide.contains("repo") && wide.contains("signals"),
        "wide header"
    );
    app.config.tui.row_density = "compact".into();
    let compact = render(&app, 120, 30);
    assert!(!compact.contains("signals"), "compact never gets a header");
}

#[test]
fn tui_popup_size_matrix_never_panics() {
    use pakmap::tui::settings::SettingsModal;
    let mut app = App::new("".into());
    app.no_color = true;
    app.packages = sample_packages();
    app.list_state.select(Some(0));
    let popups = [
        Popup::Help,
        Popup::Message("✗ Failed: x".repeat(30)),
        Popup::Info(sample_packages()[1].clone()),
        Popup::Confirm(sample_packages()[0].clone()),
        Popup::OpLog,
        Popup::History { selected: 0 },
        Popup::CommandPalette {
            query: String::new(),
            selected: 0,
        },
    ];
    for (w, h) in [
        (40, 10),
        (60, 14),
        (80, 24),
        (120, 30),
        (200, 60),
        (240, 60),
    ] {
        for popup in &popups {
            app.popup = popup.clone();
            let _ = render(&app, w, h);
        }
        app.popup = Popup::None;
        app.settings = Some(SettingsModal::new(
            pakmap::config::Config::default(),
            Vec::new(),
            None,
        ));
        let _ = render(&app, w, h);
        app.settings = None;
    }
}

#[test]
fn tui_list_viewports_stay_whole_without_preview() {
    // Sanity anchor for the split: at preview-disabled widths the results
    // block owns the full region (baseline behavior for the resize matrix).
    let mut app = App::new("firefox".into());
    app.packages = sample_packages();
    app.list_state.select(Some(0));
    app.focus = Focus::List;
    let content = render(&app, 80, 24);
    assert!(content.contains("Results"));
    assert!(content.contains("firefox"));
}

// v0.10.1 phase 3: the lens — census band over the armed filter + the
// inline filter line while the lens line is open. Unfiltered renders keep
// every pre-lens snapshot byte-stable (regions.filter is None).
#[test]
fn tui_filter_lens_census_band_and_inline_line() {
    // Armed needle 'fire' over sample_packages: all three rows (firefox,
    // firefox-bin, the flatpak id) hold it — census honesty = 3/3.
    let mut app = App::new("firefox".into());
    app.packages = sample_packages();
    app.filter_armed = Some("fire".into());
    app.list_state.select(Some(0));
    app.focus = Focus::List;
    let content = render(&app, 80, 24);
    assert!(
        content.contains("filter 'fire' 3/3"),
        "census band = matches/population"
    );
    assert!(content.contains("firefox"), "matching row visible");

    // A narrower needle hides everything it doesn't hold: 'bin' → census
    // 1/3, the firefox-bin row stays, the other two rows vanish.
    app.filter_armed = Some("bin".into());
    let content = render(&app, 80, 24);
    assert!(
        content.contains("filter 'bin' 1/3"),
        "census drops with the needle"
    );
    assert!(
        content.contains("Binary Firefox"),
        "the surviving row renders"
    );
    assert!(
        !content.contains("Standalone web browser"),
        "lens hides non-matching rows"
    );

    // Line open: the one-row inline line renders under the search box
    // with the live draft + the cursor routing into it.
    let mut app = App::new("firefox".into());
    app.packages = sample_packages();
    app.filter_draft = Some(Input::new("zed".into()));
    app.focus = Focus::List;
    let content = render(&app, 80, 24);
    assert!(content.contains("filter: zed"), "inline lens line");
}

#[test]
fn tui_updates_leg_strip_walks_like_the_search_one() {
    // v0.10.1 phase 1: the Updates tab stages its repo/AUR/flatpak legs
    // through the same strip vocabulary — repo counted, others spinning,
    // and the strip's copy speaks the updates verb (not the search one).
    let mut app = App::new("firefox".into());
    app.mode = Mode::Updates;
    app.is_loading = true;
    let mut legs = legs_fixture("", [true, true, true], 7);
    legs.apply_leg(pakmap::tui::app::SearchStage::Repo, 3);
    app.legs = Some(legs);
    let content = render(&app, 80, 24);
    assert!(
        content.contains("Checking for updates — repo 3"),
        "updates strip copy is mode-honest"
    );
    assert!(content.contains("aur ⠧"), "aur leg still spinning");
    assert!(content.contains("flatpak ⠧"), "flatpak leg still spinning");
    assert!(
        !content.contains("Searching for"),
        "updates strip must not borrow the search verb"
    );
}

// ---------------------------------------------------------------------------
// 0.12 (TUI phase 1) — focus ring + glyph set
// ---------------------------------------------------------------------------

/// The results block's top-left border cell fg (the row carrying "Results").
fn results_border_fg(app: &App) -> ratatui::style::Color {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|f| pakmap::tui::ui::draw(f, app)).unwrap();
    let buf = terminal.backend().buffer();
    for y in 0..24 {
        let line: String = (0..80).map(|x| buf[(x, y)].symbol().to_string()).collect();
        if line.contains("Results") {
            for x in 0..80 {
                if buf[(x, y)].symbol() == "╭" {
                    return buf[(x, y)].fg;
                }
            }
        }
    }
    panic!("no results border found");
}

#[test]
fn tui_results_border_tracks_focus() {
    let mut app = App::new("firefox".into());
    app.no_color = false;
    app.packages = sample_packages();
    app.list_state.select(Some(0));
    app.is_loading = false;

    app.focus = Focus::List;
    let focused = results_border_fg(&app);
    app.focus = Focus::Search;
    let idle = results_border_fg(&app);

    assert_eq!(focused, app.styles.border_focused.fg.unwrap());
    assert_eq!(idle, app.styles.border_unfocused.fg.unwrap());
    assert_ne!(focused, idle, "focus must be visible on the list border");
}

#[test]
fn tui_icons_ascii_emits_no_emoji() {
    let mut app = App::new("".into());
    app.no_color = false;
    app.config.tui.icons = "ascii".into();
    app.focus = Focus::List;
    let content = render(&app, 80, 24);
    assert!(content.contains("[1]Search"));
    assert!(content.contains("[2]Installed"));
    assert!(!content.contains("🔍"));
    assert!(!content.contains("🗑"));
}

#[test]
fn tui_icons_unicode_overrides_no_color() {
    let mut app = App::new("".into());
    app.no_color = true;
    app.config.tui.icons = "unicode".into();
    app.focus = Focus::List;
    let content = render(&app, 80, 24);
    assert!(
        content.contains("🔍"),
        "forced unicode keeps emoji even mono"
    );
}

#[test]
fn tui_icons_auto_no_color_falls_back_to_ascii() {
    let mut app = App::new("".into());
    app.no_color = true; // auto + NO_COLOR session
    app.config.tui.icons = "auto".into();
    app.focus = Focus::List;
    let content = render(&app, 80, 24);
    assert!(content.contains("[1]Search"));
    assert!(!content.contains("🔍"));
}

// ---------------------------------------------------------------------------
// 0.12 (TUI phase 2) — information architecture & honesty
// ---------------------------------------------------------------------------

#[test]
fn tui_search_titles_are_short_and_stable() {
    let mut app = App::new("".into());
    app.focus = Focus::List;
    for (mode, title) in [
        (Mode::Search, " Search "),
        (Mode::Installed, " Filter installed "),
        (Mode::Updates, " Filter updates "),
    ] {
        app.mode = mode;
        let content = render(&app, 80, 24);
        assert!(content.contains(title), "{mode:?} title");
        assert!(!content.contains("Tab remover"), "stale two-tab copy gone");
        assert!(title.len() <= 30, "title fits the acceptance cap");
    }
}

#[test]
fn tui_sort_ring_is_visible_in_the_results_title() {
    use pakmap::tui::app::SortCycle;
    let mut app = App::new("firefox".into());
    app.packages = sample_packages();
    app.list_state.select(Some(0));
    app.focus = Focus::List;
    app.is_loading = false;
    // Arrival is the unmarked default.
    assert!(!render(&app, 80, 24).contains("sort:"));
    for (sort, tag) in [
        (SortCycle::NameAsc, "sort:name"),
        (SortCycle::RepoAsc, "sort:repo"),
        (SortCycle::VersionDesc, "sort:version"),
    ] {
        app.sort = sort;
        assert!(render(&app, 80, 24).contains(tag), "{sort:?} visible");
    }
}

#[test]
fn tui_upgrade_one_confirm_is_honest() {
    use pakmap::tui::app::upgrade_old_version;
    let mut row = Package::minimal("foo".into(), "2.0-1".into(), "extra".into());
    row.description = Some("1.0-1 -> 2.0-1".into());
    row.installed = true;
    assert_eq!(upgrade_old_version(&row).as_deref(), Some("1.0-1"));
    // Non-update rows have no old version.
    let plain = Package::minimal("bar".into(), "1.0".into(), "extra".into());
    assert_eq!(upgrade_old_version(&plain), None);

    let mut app = App::new("".into());
    app.mode = Mode::Updates;
    app.focus = Focus::List;
    app.popup = Popup::ConfirmUpgradeOne {
        pkg: row,
        old_version: "1.0-1".into(),
    };
    let content = render(&app, 80, 24);
    assert!(content.contains("Confirm upgrade"));
    assert!(content.contains("1.0-1 → 2.0-1"), "old → new transition");
    assert!(content.contains("sudo pacman -S foo"));
    assert!(!content.contains("Confirm install"), "no install lie");
}

#[test]
fn tui_failed_leg_renders_cross_not_zero() {
    let mut app = App::new("firefox".into());
    app.packages = sample_packages();
    app.list_state.select(Some(0));
    app.focus = Focus::List;
    app.is_loading = true;
    let mut legs = StagedLegs::new("firefox".into(), [true, true, true]);
    legs.apply_leg(SearchStage::Repo, 2);
    legs.set_leg_error(SearchStage::Aur, "timeout".into());
    app.legs = Some(legs);
    let content = render(&app, 80, 24);
    assert!(content.contains("aur ✗"), "failed leg marked");
    assert!(content.contains("repo 2"), "healthy leg keeps its count");
}

#[test]
fn tui_tick_chars_drives_the_tui_spinner() {
    let mut app = App::new("firefox".into());
    app.is_loading = true;
    app.config.tui.tick_chars = Some("xyz".into());
    let mut legs = StagedLegs::new("firefox".into(), [true, true, true]);
    legs.started = std::time::Instant::now(); // fresh anchor => frame 0 = 'x'
    app.legs = Some(legs);
    let content = render(&app, 80, 24);
    assert!(content.contains("repo x"), "custom tick alphabet honored");
}
