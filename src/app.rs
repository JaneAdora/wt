use crate::actions;
use crate::discovery;
use crate::model::{ActiveFilter, AppState, Pane, Project, Session, StatusLine, TreePath, Worktree};
use crate::sessions;
use crate::ui::layout;
use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::Terminal;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Debug)]
pub enum RunOutcome {
    Quit,
    PrintAndExit(String),
}

#[derive(Debug)]
pub enum TickMsg {
    JobsRefreshed {
        generation: u64,
        sessions: Vec<Session>,
    },
}

#[derive(Debug, Clone)]
pub enum InputMode {
    Normal,
    Search(String),
    Modal(Vec<crate::git::LogEntry>, String),
}

pub struct UiState {
    pub mode: InputMode,
}

impl UiState {
    pub fn new() -> Self {
        Self {
            mode: InputMode::Normal,
        }
    }
}

impl Default for UiState {
    fn default() -> Self {
        Self::new()
    }
}

pub fn default_projects_root() -> PathBuf {
    dirs::home_dir().unwrap_or_default().join("projects")
}

pub fn initial_state(projects_root: PathBuf) -> Result<AppState> {
    let mut projects = discovery::scan(&projects_root)?;
    discovery::enrich_with_status(&mut projects);

    let known: Vec<PathBuf> = projects
        .iter()
        .flat_map(|p| p.worktrees.iter().map(|w| w.path.clone()))
        .collect();
    let home = dirs::home_dir().unwrap_or_default();
    let jobs = sessions::scan_jobs(&home.join(".claude/jobs")).unwrap_or_default();
    let interactive =
        sessions::scan_interactive(&home.join(".claude/projects"), &known).unwrap_or_default();
    sessions::attach_to_worktrees(&mut projects, jobs);
    sessions::attach_to_worktrees(&mut projects, interactive);

    let selected = first_visible(&projects);
    let expanded = projects.iter().map(|p| p.name.clone()).collect();
    Ok(AppState {
        projects,
        selected,
        focus: Pane::Worktrees,
        filter: ActiveFilter::ActiveOnly,
        search: None,
        last_refresh: Instant::now(),
        status: StatusLine::default(),
        expanded,
        generation: 0,
    })
}

/// Refresh in place, preserving selection/filter/search/focus/expanded.
/// Bumps `generation` so in-flight tick messages can be dropped.
pub fn refresh_in_place(state: &mut AppState, projects_root: PathBuf) -> Result<()> {
    let mut projects = discovery::scan(&projects_root)?;
    discovery::enrich_with_status(&mut projects);

    let known: Vec<PathBuf> = projects
        .iter()
        .flat_map(|p| p.worktrees.iter().map(|w| w.path.clone()))
        .collect();
    let home = dirs::home_dir().unwrap_or_default();
    let jobs = sessions::scan_jobs(&home.join(".claude/jobs")).unwrap_or_default();
    let interactive =
        sessions::scan_interactive(&home.join(".claude/projects"), &known).unwrap_or_default();
    sessions::attach_to_worktrees(&mut projects, jobs);
    sessions::attach_to_worktrees(&mut projects, interactive);

    state.projects = projects;
    state.last_refresh = Instant::now();
    state.generation = state.generation.wrapping_add(1);
    Ok(())
}

fn first_visible(projects: &[Project]) -> Option<TreePath> {
    projects
        .iter()
        .find_map(|p| p.worktrees.first().map(|w| TreePath::worktree_row(p, w)))
}

pub fn run<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    state: &mut AppState,
    ui: &mut UiState,
    tick_rx: Receiver<TickMsg>,
    gen_counter: Arc<AtomicU64>,
) -> Result<RunOutcome> {
    loop {
        terminal.draw(|f| render_frame(f, state, ui))?;

        while let Ok(msg) = tick_rx.try_recv() {
            apply_tick(state, msg);
        }

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                let prev_gen = state.generation;
                if let Some(outcome) = handle_key(state, ui, key)? {
                    return Ok(outcome);
                }
                if state.generation != prev_gen {
                    gen_counter.store(state.generation, Ordering::SeqCst);
                }
            }
        }
    }
}

fn apply_tick(state: &mut AppState, msg: TickMsg) {
    match msg {
        TickMsg::JobsRefreshed {
            generation,
            sessions,
        } => {
            if generation != state.generation {
                return;
            }
            for p in state.projects.iter_mut() {
                for wt in p.worktrees.iter_mut() {
                    wt.sessions
                        .retain(|s| !matches!(s, Session::BackgroundJob { .. }));
                }
            }
            sessions::attach_to_worktrees(&mut state.projects, sessions);
        }
    }
}

fn render_frame(f: &mut ratatui::Frame, state: &AppState, ui: &UiState) {
    let area = f.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(60),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(area);
    let cols = layout::choose_columns(area.width);

    crate::ui::worktrees::render(f, chunks[0], state, cols);
    crate::ui::sessions::render(f, chunks[1], state);
    render_footer(f, chunks[2], state, cols, ui);

    if let InputMode::Modal(log, title) = &ui.mode {
        let modal_area = centered_rect(area, 60, 60);
        crate::ui::modal::render(f, modal_area, log, title);
    }
}

fn render_footer(
    f: &mut ratatui::Frame,
    area: Rect,
    state: &AppState,
    cols: layout::Columns,
    ui: &UiState,
) {
    use ratatui::text::{Line, Span};
    use ratatui::widgets::Paragraph;

    if let InputMode::Search(buf) = &ui.mode {
        f.render_widget(
            Paragraph::new(format!("/ {buf}_  (Enter to apply, Esc to cancel)")),
            area,
        );
        return;
    }
    let mut bits = vec![Span::raw("↑↓ ↵ Tab c o r / a g q")];
    if cols.too_narrow {
        bits.push(Span::raw("  "));
        bits.push(Span::styled("narrow", crate::ui::theme::status_line()));
    }
    if let Some(msg) = state.status.current() {
        bits.push(Span::raw("  "));
        bits.push(Span::styled(
            msg.to_string(),
            crate::ui::theme::status_line(),
        ));
    }
    f.render_widget(Paragraph::new(Line::from(bits)), area);
}

fn centered_rect(parent: Rect, percent_x: u16, percent_y: u16) -> Rect {
    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(parent);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(v[1])[1]
}

fn handle_key(
    state: &mut AppState,
    ui: &mut UiState,
    key: KeyEvent,
) -> Result<Option<RunOutcome>> {
    match &mut ui.mode {
        InputMode::Search(buf) => {
            match key.code {
                KeyCode::Esc => {
                    ui.mode = InputMode::Normal;
                    state.search = None;
                }
                KeyCode::Enter => {
                    state.search = if buf.is_empty() {
                        None
                    } else {
                        Some(buf.clone())
                    };
                    ui.mode = InputMode::Normal;
                }
                KeyCode::Backspace => {
                    buf.pop();
                    state.search = Some(buf.clone());
                }
                KeyCode::Char(c) => {
                    buf.push(c);
                    state.search = Some(buf.clone());
                }
                _ => {}
            }
            return Ok(None);
        }
        InputMode::Modal(_, _) => {
            if matches!(key.code, KeyCode::Esc | KeyCode::Char('q') | KeyCode::Enter) {
                ui.mode = InputMode::Normal;
            }
            return Ok(None);
        }
        InputMode::Normal => {}
    }

    match (key.code, key.modifiers) {
        (KeyCode::Char('q'), _) => Ok(Some(RunOutcome::Quit)),
        (KeyCode::Char('c'), m) if m.contains(KeyModifiers::CONTROL) => {
            Ok(Some(RunOutcome::Quit))
        }
        (KeyCode::Char('c'), _) => {
            copy_current(state)?;
            Ok(None)
        }
        (KeyCode::Char('o'), _) => {
            if let Some(cmd) = launch_for_selected(state) {
                return Ok(Some(RunOutcome::PrintAndExit(cmd)));
            }
            Ok(None)
        }
        (KeyCode::Char('r'), _) => {
            refresh_in_place(state, default_projects_root())?;
            state.status.say("refreshed");
            Ok(None)
        }
        (KeyCode::Char('a'), _) => {
            state.filter = match state.filter {
                ActiveFilter::ActiveOnly => ActiveFilter::All,
                ActiveFilter::All => ActiveFilter::ActiveOnly,
            };
            Ok(None)
        }
        (KeyCode::Char('/'), _) => {
            ui.mode = InputMode::Search(String::new());
            state.search = Some(String::new());
            Ok(None)
        }
        (KeyCode::Char('g'), _) => {
            if let Some(path) = current_worktree_path(state) {
                let entries = crate::git::run_log_recent(&path, 20).unwrap_or_default();
                let title = path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("?")
                    .to_string();
                ui.mode = InputMode::Modal(entries, title);
            }
            Ok(None)
        }
        (KeyCode::Enter, _) => {
            handle_enter(state);
            Ok(None)
        }
        (KeyCode::Down, _) | (KeyCode::Char('j'), _) => {
            move_selection(state, 1);
            Ok(None)
        }
        (KeyCode::Up, _) | (KeyCode::Char('k'), _) => {
            move_selection(state, -1);
            Ok(None)
        }
        (KeyCode::Tab, _) => {
            state.focus = next_pane(state.focus);
            Ok(None)
        }
        (KeyCode::BackTab, _) => {
            state.focus = next_pane(state.focus);
            Ok(None)
        }
        _ => Ok(None),
    }
}

fn next_pane(p: Pane) -> Pane {
    match p {
        Pane::Worktrees => Pane::Sessions,
        Pane::Sessions => Pane::Worktrees,
    }
}

fn handle_enter(state: &mut AppState) {
    let Some(sel) = state.selected.clone() else {
        return;
    };
    if sel.worktree.is_none() {
        if state.expanded.contains(&sel.project) {
            state.expanded.remove(&sel.project);
        } else {
            state.expanded.insert(sel.project.clone());
        }
    } else {
        state.focus = Pane::Sessions;
    }
}

fn visible_paths(state: &AppState) -> Vec<TreePath> {
    let mut out = Vec::new();
    for p in &state.projects {
        let has_any = match state.filter {
            ActiveFilter::All => !p.worktrees.is_empty(),
            ActiveFilter::ActiveOnly => p
                .worktrees
                .iter()
                .any(|w| is_active_or_search_match(w, state)),
        };
        if !has_any {
            continue;
        }
        out.push(TreePath::project_header(p));
        if state.expanded.contains(&p.name) {
            for w in &p.worktrees {
                if !is_active_or_search_match(w, state) {
                    continue;
                }
                out.push(TreePath::worktree_row(p, w));
            }
        }
    }
    out
}

fn is_active_or_search_match(w: &Worktree, state: &AppState) -> bool {
    let search_ok = match &state.search {
        Some(q) if !q.is_empty() => w
            .path
            .to_string_lossy()
            .to_lowercase()
            .contains(&q.to_lowercase()),
        _ => true,
    };
    if !search_ok {
        return false;
    }
    if state.filter == ActiveFilter::All {
        return true;
    }
    crate::ui::worktrees::is_active(w)
}

fn move_selection(state: &mut AppState, delta: i32) {
    let paths = visible_paths(state);
    if paths.is_empty() {
        return;
    }
    let idx = state
        .selected
        .as_ref()
        .and_then(|s| paths.iter().position(|p| p == s))
        .unwrap_or(0) as i32;
    let new = (idx + delta).clamp(0, paths.len() as i32 - 1) as usize;
    state.selected = Some(paths[new].clone());
}

fn copy_current(state: &mut AppState) -> Result<()> {
    if let Some(cmd) = launch_for_selected(state) {
        actions::copy_to_clipboard(&cmd)?;
        state.status.say("copied to clipboard");
    } else {
        state.status.say("nothing selected");
    }
    Ok(())
}

fn launch_for_selected(state: &AppState) -> Option<String> {
    let sel = state.selected.as_ref()?;
    let p = state.projects.iter().find(|p| p.name == sel.project)?;
    let wt = match &sel.worktree {
        Some(path) => p.worktrees.iter().find(|w| w.path == *path)?,
        None => p.worktrees.first()?,
    };
    Some(actions::launch_command_for(&wt.path, None))
}

fn current_worktree_path(state: &AppState) -> Option<PathBuf> {
    let sel = state.selected.as_ref()?;
    let p = state.projects.iter().find(|p| p.name == sel.project)?;
    let wt = match &sel.worktree {
        Some(path) => p.worktrees.iter().find(|w| w.path == *path)?,
        None => p.worktrees.first()?,
    };
    Some(wt.path.clone())
}
