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
    Modal {
        entries: Vec<crate::git::LogEntry>,
        title: String,
        scroll: u16,
    },
    Help {
        scroll: u16,
    },
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

pub fn default_projects_roots() -> Vec<PathBuf> {
    discovery::default_roots()
}

pub fn initial_state(projects_roots: Vec<PathBuf>) -> Result<AppState> {
    let mut projects = discovery::scan_many(&projects_roots)?;
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
        selected_session: None,
    })
}

/// Refresh in place, preserving selection/filter/search/focus/expanded.
/// Bumps `generation` so in-flight tick messages can be dropped.
pub fn refresh_in_place(state: &mut AppState, projects_roots: Vec<PathBuf>) -> Result<()> {
    let mut projects = discovery::scan_many(&projects_roots)?;
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

    match &ui.mode {
        InputMode::Modal { entries, title, scroll } => {
            let modal_area = centered_rect(area, 94, 90);
            crate::ui::modal::render(f, modal_area, entries, title, *scroll);
        }
        InputMode::Help { scroll } => {
            let modal_area = centered_rect(area, 94, 90);
            crate::ui::modal::render_help(f, modal_area, *scroll);
        }
        _ => {}
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

    match &ui.mode {
        InputMode::Search(buf) => {
            f.render_widget(
                Paragraph::new(format!("/ {buf}_  Enter apply · Esc cancel")),
                area,
            );
            return;
        }
        InputMode::Modal { .. } | InputMode::Help { .. } => {
            // Modal/help have their own footer hints in title_bottom.
            return;
        }
        InputMode::Normal => {}
    }

    let mut bits = vec![Span::raw("? help · ↑↓/jk · Tab · ↵ · c copy · D launch · r refresh · / filter · a active · g log · q")];
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
        InputMode::Modal { entries, scroll, .. } => {
            let total = entries.len() as u16 * 2;
            let close = handle_scroll_key(scroll, total, &key);
            if close {
                ui.mode = InputMode::Normal;
            }
            return Ok(None);
        }
        InputMode::Help { scroll } => {
            // Help text is ~50 lines; cap scroll on that.
            let close = handle_scroll_key(scroll, 50, &key);
            if close {
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
            copy_current(state, false)?;
            Ok(None)
        }
        (KeyCode::Char('o'), _) => {
            if let Some(cmd) = launch_for_selected(state, false) {
                return Ok(Some(RunOutcome::PrintAndExit(cmd)));
            }
            Ok(None)
        }
        (KeyCode::Char('D'), _) => {
            if let Some(cmd) = launch_for_selected(state, true) {
                return Ok(Some(RunOutcome::PrintAndExit(cmd)));
            }
            Ok(None)
        }
        (KeyCode::Char('?'), _) => {
            ui.mode = InputMode::Help { scroll: 0 };
            Ok(None)
        }
        (KeyCode::Char('r'), _) => {
            refresh_in_place(state, default_projects_roots())?;
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
            if let Some(wt) = current_worktree(state) {
                let entries = crate::git::run_log_recent(&wt.path, 20).unwrap_or_default();
                let title = wt
                    .path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("?")
                    .to_string();
                ui.mode = InputMode::Modal {
                    entries,
                    title,
                    scroll: 0,
                };
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

/// Apply a scroll/dismiss key to a modal's scroll offset. Returns true
/// when the modal should close. Doesn't touch UiState so the caller can
/// hold a borrow on it.
#[must_use]
fn handle_scroll_key(scroll: &mut u16, total: u16, key: &KeyEvent) -> bool {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Enter => return true,
        KeyCode::Down | KeyCode::Char('j') => {
            *scroll = scroll.saturating_add(1);
        }
        KeyCode::Up | KeyCode::Char('k') => {
            *scroll = scroll.saturating_sub(1);
        }
        KeyCode::PageDown | KeyCode::Char(' ') => {
            *scroll = scroll.saturating_add(10).min(total.saturating_sub(1));
        }
        KeyCode::PageUp | KeyCode::Char('b') => {
            *scroll = scroll.saturating_sub(10);
        }
        KeyCode::Char('g') | KeyCode::Home => {
            *scroll = 0;
        }
        KeyCode::Char('G') | KeyCode::End => {
            *scroll = total.saturating_sub(1);
        }
        _ => {}
    }
    false
}

fn handle_enter(state: &mut AppState) {
    let Some(sel) = state.selected.clone() else {
        return;
    };
    if sel.worktree.is_none() {
        if state.expanded.contains(&sel.project) {
            state.expanded.remove(&sel.project);
            // Selection may now point at a hidden row; if so, leave the
            // selection on the project header instead so the cursor stays
            // visible. The TreePath struct enforces this via worktree=None
            // for project rows, but the selected field is unchanged here,
            // which is correct as long as we collapsed the *currently*
            // selected project's worktrees.
        } else {
            state.expanded.insert(sel.project.clone());
        }
    } else {
        state.focus = Pane::Sessions;
        // Initialise session selection so j/k in the sessions pane works.
        if state.selected_session.is_none() {
            if let Some(wt) = current_worktree(state) {
                if !wt.sessions.is_empty() {
                    state.selected_session = Some(0);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{JobStatus, Project, Session, SessionState, Worktree};
    use std::collections::HashSet;
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    fn fixture_state() -> AppState {
        let now = std::time::SystemTime::now();
        let interactive = Session::Interactive {
            id: "abc-123".to_string(),
            summary: "fix gmail".to_string(),
            cwd: PathBuf::from("/p/alpha"),
            mtime: now,
            state: SessionState::Active,
        };
        let bgjob = Session::BackgroundJob {
            id: "fe9c".to_string(),
            status: JobStatus::Running,
            cwd: PathBuf::from("/p/alpha"),
            mtime: now,
            intent: None,
        };
        let wt = Worktree {
            path: PathBuf::from("/p/alpha"),
            branch: Some("main".into()),
            dirty: false,
            ahead: 0,
            behind: 0,
            last_commit: None,
            sessions: vec![interactive, bgjob],
            has_upstream: false,
        };
        let proj = Project {
            name: "alpha".into(),
            root: PathBuf::from("/p"),
            worktrees: vec![wt.clone()],
        };
        let selected = Some(TreePath::worktree_row(&proj, &wt));
        let mut expanded = HashSet::new();
        expanded.insert("alpha".to_string());
        AppState {
            projects: vec![proj],
            selected,
            focus: Pane::Worktrees,
            filter: ActiveFilter::All,
            search: None,
            last_refresh: Instant::now(),
            status: StatusLine::default(),
            expanded,
            generation: 0,
            selected_session: None,
        }
    }

    #[test]
    fn launch_for_worktree_focus_uses_no_resume() {
        let state = fixture_state();
        let cmd = launch_for_selected(&state, false).unwrap();
        assert_eq!(cmd, "cd /p/alpha && claude");
    }

    #[test]
    fn launch_for_session_focus_uses_resume_for_interactive() {
        let mut state = fixture_state();
        state.focus = Pane::Sessions;
        state.selected_session = Some(0); // first session is interactive
        let cmd = launch_for_selected(&state, false).unwrap();
        assert_eq!(cmd, "cd /p/alpha && claude --resume abc-123");
    }

    #[test]
    fn launch_for_session_focus_bg_job_falls_back_to_worktree() {
        let mut state = fixture_state();
        state.focus = Pane::Sessions;
        state.selected_session = Some(1); // second session is bg job
        let cmd = launch_for_selected(&state, false).unwrap();
        assert_eq!(cmd, "cd /p/alpha && claude");
    }

    #[test]
    fn launch_with_dangerous_flag_for_worktree() {
        let state = fixture_state();
        let cmd = launch_for_selected(&state, true).unwrap();
        assert_eq!(cmd, "cd /p/alpha && claude --dangerously-skip-permissions");
    }

    #[test]
    fn launch_with_dangerous_flag_for_interactive_session() {
        let mut state = fixture_state();
        state.focus = Pane::Sessions;
        state.selected_session = Some(0);
        let cmd = launch_for_selected(&state, true).unwrap();
        assert_eq!(
            cmd,
            "cd /p/alpha && claude --resume abc-123 --dangerously-skip-permissions"
        );
    }

    #[test]
    fn move_session_advances_within_pane() {
        let mut state = fixture_state();
        state.focus = Pane::Sessions;
        state.selected_session = Some(0);
        move_selection(&mut state, 1);
        assert_eq!(state.selected_session, Some(1));
        move_selection(&mut state, 1);
        assert_eq!(state.selected_session, Some(1)); // clamped at last
        move_selection(&mut state, -1);
        assert_eq!(state.selected_session, Some(0));
    }

    #[test]
    fn worktree_selection_change_resets_session_index() {
        // Two worktrees on the same project so the cursor can move.
        let mut state = fixture_state();
        // Add a second worktree.
        let wt2 = Worktree {
            path: PathBuf::from("/p/alpha/.claude/worktrees/x"),
            branch: Some("x".into()),
            dirty: false,
            ahead: 0,
            behind: 0,
            last_commit: None,
            sessions: vec![],
            has_upstream: false,
        };
        state.projects[0].worktrees.push(wt2);
        state.selected_session = Some(3); // pretend we had one selected

        move_selection(&mut state, 1); // moves worktree selection in Worktrees pane
        assert_eq!(state.selected_session, None, "session selection must reset on worktree change");
    }

    #[test]
    fn refresh_preserves_user_state() {
        // Build a temp project root with one real repo so initial_state +
        // refresh_in_place can both run.
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("alpha");
        std::fs::create_dir_all(&repo).unwrap();
        std::process::Command::new("git").args(["init", "-q"]).current_dir(&repo).status().unwrap();
        std::process::Command::new("git")
            .args(["commit", "--allow-empty", "-q", "-m", "init"])
            .current_dir(&repo)
            .env("GIT_AUTHOR_NAME", "t").env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "t").env("GIT_COMMITTER_EMAIL", "t@t")
            .status().unwrap();

        let roots = vec![tmp.path().to_path_buf()];
        let mut state = initial_state(roots.clone()).unwrap();
        let original_selected = state.selected.clone();
        state.filter = ActiveFilter::All;
        state.search = Some("alph".to_string());
        state.focus = Pane::Sessions;
        let original_gen = state.generation;

        refresh_in_place(&mut state, roots).unwrap();

        assert_eq!(state.selected, original_selected, "selection preserved");
        assert_eq!(state.filter, ActiveFilter::All, "filter preserved");
        assert_eq!(state.search.as_deref(), Some("alph"), "search preserved");
        assert_eq!(state.focus, Pane::Sessions, "focus preserved");
        assert_ne!(state.generation, original_gen, "generation bumped");
    }
}

fn current_worktree<'a>(state: &'a AppState) -> Option<&'a crate::model::Worktree> {
    let sel = state.selected.as_ref()?;
    let p = state.projects.iter().find(|p| p.name == sel.project)?;
    match &sel.worktree {
        Some(path) => p.worktrees.iter().find(|w| w.path == *path),
        None => p.worktrees.first(),
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
    match state.focus {
        Pane::Worktrees => move_worktree_selection(state, delta),
        Pane::Sessions => move_session_selection(state, delta),
    }
}

fn move_worktree_selection(state: &mut AppState, delta: i32) {
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
    let new_path = paths[new].clone();
    if state.selected.as_ref() != Some(&new_path) {
        // Worktree (or project header) changed: reset session selection.
        state.selected_session = None;
    }
    state.selected = Some(new_path);
}

fn move_session_selection(state: &mut AppState, delta: i32) {
    let count = current_worktree(state)
        .map(|wt| wt.sessions.len())
        .unwrap_or(0);
    if count == 0 {
        state.selected_session = None;
        return;
    }
    let cur = state.selected_session.unwrap_or(0) as i32;
    let new = (cur + delta).clamp(0, count as i32 - 1) as usize;
    state.selected_session = Some(new);
}

fn copy_current(state: &mut AppState, dangerous: bool) -> Result<()> {
    if let Some(cmd) = launch_for_selected(state, dangerous) {
        actions::copy_to_clipboard(&cmd)?;
        let msg = if dangerous { "copied (dangerous)" } else { "copied" };
        state.status.say(msg);
    } else {
        state.status.say("nothing selected");
    }
    Ok(())
}

fn launch_for_selected(state: &AppState, dangerous: bool) -> Option<String> {
    let wt = current_worktree(state)?;

    // If the sessions pane has focus and an interactive session is selected,
    // build `cd <session-cwd> && claude --resume <id>`. Background jobs and
    // unfocused selection fall back to `cd <worktree-path> && claude`.
    if state.focus == Pane::Sessions {
        if let Some(idx) = state.selected_session {
            if let Some(Session::Interactive { id, cwd, .. }) = wt.sessions.get(idx) {
                return Some(actions::launch_command_for(cwd, Some(id), dangerous));
            }
        }
    }

    Some(actions::launch_command_for(&wt.path, None, dangerous))
}

