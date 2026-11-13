//! The TUI drawing tree (overhaul phase 2): `draw` stays the single public
//! entry — the run loop and the integration snapshots keep calling
//! `pakmap::tui::ui::draw` — while the monolith split into named layers:
//!
//! - [`layout`] — the pure geometry/truncation vocabulary (stress-matrix
//!   tested resize contract, plan §3),
//! - [`chrome`] — tabs / status / help / the too-small floor warning,
//! - [`body`] — search input + the results/installed/updates list,
//! - [`overlays`] — the centered popups (info, confirms, help, message),
//! - [`settings_overlay`] — the `S` settings modal ( thereby pinned
//!   byte-identical by its render-hash test).
//!
//! Layer rule: chrome < body < overlays < settings modal < popups — the same
//! paint order the v0.10 monolith had. D1: every style comes from
//! `crate::tui::style`; no file in this tree constructs ratatui styles.

pub mod body;
pub mod chrome;
pub mod layout;
pub mod overlays;
pub mod settings_overlay;

use ratatui::Frame;

use super::app::{App, Popup};

// tui-design skill: calm, predictable, fast — single border depth, semantic tokens, NO_COLOR aware
// v0.10.0 lineage: staged tabs bar, ThemeStyles cache (phase 1 registry), `S` settings modal.
pub fn draw(f: &mut Frame, app: &App) {
    let area = f.area();
    let pal = app.palette();

    // Responsive floor: configurable via [tui] floor_width/floor_height per
    // tui-design responsive + layout Min — layout.rs owns the contract now.
    let (fw, fh) = layout::floor_required(&app.config.tui);
    if !layout::floor_ok(area, (fw, fh)) {
        chrome::draw_floor_warning(f, area, pal, fw, fh);
        return;
    }

    // Frame regions: tabs(1) + search(Ls) + results(Min Lm) + status(1) + help(1)
    let regions_ = layout::regions(
        area,
        app.config.tui.show_tabs,
        app.config.tui.layout_search.max(2),
        app.config.tui.layout_results_min.max(4),
        app.filter_draft.is_some(),
    );
    if let Some(tabs) = regions_.tabs {
        chrome::draw_tabs(f, app, tabs);
    }
    body::draw_search(f, app, regions_.search);
    // v0.10.1 phase 3: the one-row inline lens line under the search box.
    if let Some(fa) = regions_.filter {
        chrome::draw_filter_line(f, app, fa);
    }
    body::draw_results(f, app, regions_.results);
    chrome::draw_status(f, app, regions_.status);
    chrome::draw_help(f, app, regions_.help);

    // v0.10.0 phase 3: settings modal above the app view, below popups.
    if app.settings.is_some() {
        settings_overlay::draw_settings_modal(f, app, area);
    }

    // Popups on top with Clear hole-punch
    match &app.popup {
        Popup::Info(pkg) => overlays::draw_info_popup(f, pkg, area, app),
        Popup::Confirm(pkg) => overlays::draw_confirm_popup(f, pkg, area, app),
        Popup::ConfirmRemove(pkg) => overlays::draw_confirm_remove_popup(f, pkg, area, app),
        Popup::ConfirmRefresh => overlays::draw_confirm_refresh_popup(f, area, app),
        Popup::ConfirmUpgrade => overlays::draw_confirm_upgrade_popup(f, area, app),
        Popup::ConfirmClean {
            files,
            bytes,
            flatpak,
            clones,
        } => overlays::draw_confirm_clean_popup(f, *files, *bytes, *flatpak, *clones, area, app),
        Popup::Help => overlays::draw_help_popup(f, area, app),
        Popup::Message(msg) => overlays::draw_message_popup(f, msg, area, app),
        Popup::None => {}
    }
}
