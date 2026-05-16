mod actions;
mod app;
mod discovery;
mod git;
mod model;
mod sessions;
mod tick;
mod ui;

use anyhow::Result;
use std::sync::atomic::AtomicU64;
use std::sync::{mpsc, Arc};

fn main() -> Result<()> {
    let projects_root = dirs::home_dir().unwrap_or_default().join("projects");
    let mut state = app::initial_state(projects_root)?;
    let mut ui_state = app::UiState::new();

    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    crossterm::execute!(stdout, crossterm::terminal::EnterAlternateScreen)?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = ratatui::Terminal::new(backend)?;

    let (tick_tx, tick_rx) = mpsc::channel::<app::TickMsg>();
    let gen_counter = Arc::new(AtomicU64::new(state.generation));
    let _tick_handle = tick::spawn(tick_tx, Arc::clone(&gen_counter));

    let result = app::run(&mut terminal, &mut state, &mut ui_state, tick_rx, gen_counter);

    crossterm::execute!(std::io::stdout(), crossterm::terminal::LeaveAlternateScreen)?;
    crossterm::terminal::disable_raw_mode()?;

    match result? {
        app::RunOutcome::Quit => Ok(()),
        app::RunOutcome::PrintAndExit(cmd) => {
            println!("{cmd}");
            Ok(())
        }
    }
}
