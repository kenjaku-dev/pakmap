pub mod app;
pub mod keys;
pub mod settings;
pub mod style;
pub mod ui;

use color_eyre::Result;
use ratatui::DefaultTerminal;

use app::App;

pub fn run(initial_query: String) -> Result<()> {
    let _ = color_eyre::install();
    let mut terminal = ratatui::try_init().map_err(|e| color_eyre::eyre::eyre!(e))?;
    let result = App::new(initial_query).run(&mut terminal);
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
    result
}

pub fn run_with_cli(initial_query: String, cli: &crate::cli::Cli) -> Result<()> {
    let _ = color_eyre::install();
    let mut terminal = ratatui::try_init().map_err(|e| color_eyre::eyre::eyre!(e))?;
    let result = App::new_with_cli(initial_query, cli).run(&mut terminal);
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
    result
}

pub fn run_with_config(
    initial_query: String,
    cli: &crate::cli::Cli,
    cfg: &crate::config::Config,
) -> Result<()> {
    let _ = color_eyre::install();
    let mut terminal = ratatui::try_init().map_err(|e| color_eyre::eyre::eyre!(e))?;
    let result = App::new_with_config(initial_query, cli, cfg).run(&mut terminal);
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
    result
}

// For suspending TUI to run child process (pacman/makepkg) — per tui-design lifecycle
// Phase D4: flush + drain EventStream per ratatui recipe, both-fail aggregation already handled
pub fn suspend_and_run<F, T>(terminal: &mut DefaultTerminal, f: F) -> color_eyre::Result<T>
where
    F: FnOnce() -> anyhow::Result<T>,
{
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
            crossterm::cursor::Show
        );
    };
    match (child_result, reinit) {
        (Ok(v), Ok(t)) => {
            *terminal = t;
            let _ = terminal.clear();
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
