pub mod app;
pub mod keys;
pub mod settings;
pub mod style;
pub mod ui;

use color_eyre::Result;
use ratatui::DefaultTerminal;

use app::App;

pub fn run_with_config(
    initial_query: String,
    cli: &crate::cli::Cli,
    cfg: &crate::config::Config,
) -> Result<()> {
    let _ = color_eyre::install();
    // Deferred-phase launch maintenance (behavior.clean_interval_secs):
    // user-owned dirs, no sudo, never fatal — runs BEFORE the alternate
    // screen so a prune is plain fs work, and the honest note rides the
    // boot status line (transient — the next status update replaces it).
    let maintenance_note = crate::install::clean::launch_maintenance(cfg);
    let mut terminal = ratatui::try_init().map_err(|e| color_eyre::eyre::eyre!(e))?;
    // 0.12 (TUI phase 4): opt-in mouse capture. Off by default so terminal
    // text selection keeps working; Shift+drag recovers it when enabled.
    if cfg.tui.mouse {
        let _ = crossterm::execute!(std::io::stdout(), crossterm::event::EnableMouseCapture);
    }
    let mut app = App::new_with_config(initial_query, cli, cfg);
    if let Some(note) = maintenance_note {
        app.status = format!("{} — {note}", app.status);
    }
    let result = app.run(&mut terminal);
    if let Err(e) = ratatui::try_restore() {
        let _ = crossterm::terminal::disable_raw_mode();
        let _ = crossterm::execute!(
            std::io::stdout(),
            crossterm::terminal::LeaveAlternateScreen,
            crossterm::cursor::Show,
            crossterm::event::DisableMouseCapture
        );
        eprintln!("restore warning: {e}");
    }
    // Always release capture on exit, even when try_restore succeeded.
    let _ = crossterm::execute!(std::io::stdout(), crossterm::event::DisableMouseCapture);
    result
}

// For suspending TUI to run child process (pacman/makepkg) — per tui-design lifecycle
// Phase D4: flush + drain EventStream per ratatui recipe, both-fail aggregation already handled
pub fn suspend_and_run<F, T>(
    terminal: &mut DefaultTerminal,
    mouse: bool,
    f: F,
) -> color_eyre::Result<T>
where
    F: FnOnce() -> anyhow::Result<T>,
{
    // 0.12 (TUI phase 4): give the mouse back to the shell before the child
    // owns the TTY, then re-capture it on reentry.
    if mouse {
        let _ = crossterm::execute!(std::io::stdout(), crossterm::event::DisableMouseCapture);
    }
    // 1. Restore shell modes — best-effort, log but don't abort child
    if let Err(e) = ratatui::try_restore() {
        let _ = crossterm::terminal::disable_raw_mode();
        let _ = crossterm::execute!(
            std::io::stdout(),
            crossterm::terminal::LeaveAlternateScreen,
            crossterm::cursor::Show
        );
        eprintln!("suspend restore warning: {e}");
    }
    // Drain any pending crossterm events that could be consumed during reinit (tui-design)
    while crossterm::event::poll(std::time::Duration::from_millis(0)).unwrap_or(false) {
        let _ = crossterm::event::read();
    }
    let _ = std::io::Write::flush(&mut std::io::stdout());

    let child_result = f().map_err(|e| color_eyre::eyre::eyre!(e));

    // 2. Re-enter TUI — independent best-effort, per tui-design recipe (store child_result)
    let reinit = ratatui::try_init();
    // Best-effort cleanup so a failed reentry never leaves the shell in a
    // half-initialized state (raw mode / alt-screen) before reporting.
    let cleanup_terminal = || {
        let _ = crossterm::terminal::disable_raw_mode();
        let _ = crossterm::execute!(
            std::io::stdout(),
            crossterm::terminal::LeaveAlternateScreen,
            crossterm::cursor::Show,
            crossterm::event::DisableMouseCapture
        );
    };
    match (child_result, reinit) {
        (Ok(v), Ok(t)) => {
            *terminal = t;
            let _ = terminal.clear();
            if mouse {
                let _ =
                    crossterm::execute!(std::io::stdout(), crossterm::event::EnableMouseCapture);
            }
            // Flush and drain again after reentry
            let _ = std::io::Write::flush(&mut std::io::stdout());
            while crossterm::event::poll(std::time::Duration::from_millis(0)).unwrap_or(false) {
                let _ = crossterm::event::read();
            }
            Ok(v)
        }
        (Ok(_v), Err(e)) => {
            cleanup_terminal();
            Err(color_eyre::eyre::eyre!(
                "reentry failed after child success: {e}"
            ))
        }
        (Err(e), Ok(t)) => {
            *terminal = t;
            let _ = terminal.clear();
            if mouse {
                let _ =
                    crossterm::execute!(std::io::stdout(), crossterm::event::EnableMouseCapture);
            }
            let _ = std::io::Write::flush(&mut std::io::stdout());
            while crossterm::event::poll(std::time::Duration::from_millis(0)).unwrap_or(false) {
                let _ = crossterm::event::read();
            }
            Err(e)
        }
        (Err(e1), Err(e2)) => {
            cleanup_terminal();
            Err(color_eyre::eyre::eyre!(
                "child failed: {e1}; reentry also failed: {e2}"
            ))
        }
    }
}
