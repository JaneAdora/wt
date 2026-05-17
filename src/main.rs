mod actions;
mod app;
mod config;
mod discovery;
mod git;
mod model;
mod sessions;
mod tick;
mod ui;

use anyhow::Result;
use std::sync::atomic::AtomicU64;
use std::sync::{mpsc, Arc};

const VERSION: &str = env!("CARGO_PKG_VERSION");

const CLI_HELP: &str = "\
wt :: Worktree Wizard

USAGE:
  wt              Launch the dashboard (interactive TTY required).
  wt --help       Print this message and exit.
  wt --version    Print the version and exit.

ENVIRONMENT:
  WT_ROOTS        Override scan roots, colon-separated (like PATH).
                  Default: ~/:~/projects:~/Projects:~/Projects/clients
  TZ              Standard timezone override (e.g., America/Chicago).
                  Affects how timestamps are displayed.

INSIDE wt:
  Press ? for the full keymap, or visit docs/ROADMAP.md.
";

fn main() -> Result<()> {
    // Handle CLI flags before touching the terminal.
    let args: Vec<String> = std::env::args().skip(1).collect();
    for arg in &args {
        match arg.as_str() {
            "--version" | "-V" => {
                println!("wt {VERSION}");
                return Ok(());
            }
            "--help" | "-h" => {
                print!("{CLI_HELP}");
                return Ok(());
            }
            other => {
                eprintln!("wt: unknown argument: {other}\n\nTry: wt --help");
                std::process::exit(2);
            }
        }
    }

    let roots = app::default_projects_roots();
    let mut state = app::initial_state(roots)?;
    let mut ui_state = app::UiState::new();

    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    crossterm::execute!(
        stdout,
        crossterm::terminal::EnterAlternateScreen,
        crossterm::terminal::SetTitle("Worktree Wizard"),
    )?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = ratatui::Terminal::new(backend)?;

    let (tick_tx, tick_rx) = mpsc::channel::<app::TickMsg>();
    let gen_counter = Arc::new(AtomicU64::new(state.generation));
    let _tick_handle = tick::spawn(tick_tx, Arc::clone(&gen_counter));

    let result = app::run(&mut terminal, &mut state, &mut ui_state, tick_rx, gen_counter);

    // Persist UI state regardless of how we exit. Best-effort; failure
    // is silent.
    let _ = config::save(&config::snapshot(&state));

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
