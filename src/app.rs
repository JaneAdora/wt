use crate::actions;
use crate::discovery;
use crate::model::{
    ActiveFilter, AppState, Pane, Project, Session, StatusLine, TreePath, Worktree,
};
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
    /// Detail view of a single item (session). Pre-rendered key/value
    /// lines so the modal renderer can wrap them.
    Detail {
        title: String,
        lines: Vec<(String, String)>,
        scroll: u16,
    },
}

pub struct UiState {
    pub mode: InputMode,
    /// Two-step delete confirm for bg jobs. First `x` arms it with the
    /// job id and the press timestamp; second `x` within 3s on the same
    /// job fires the delete. Cleared on selection change or timeout.
    pub pending_delete: Option<(String, Instant)>,
    /// Two-step confirm for bulk delete (X capital).
    pub pending_bulk_delete: Option<Instant>,
    /// Last soft-deleted entries, available for restore with `u`.
    /// Each entry is (original_path, trash_path). LIFO; cap small.
    pub trash_history: Vec<(PathBuf, PathBuf)>,
}

impl UiState {
    pub fn new() -> Self {
        Self {
            mode: InputMode::Normal,
            pending_delete: None,
            pending_bulk_delete: None,
            trash_history: Vec::new(),
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
    let saved = crate::config::load_or_default();
    let mut projects = discovery::scan_many(&projects_roots)?;
    discovery::enrich_with_status(&mut projects);

    let known: Vec<PathBuf> = projects
        .iter()
        .flat_map(|p| p.worktrees.iter().map(|w| w.path.clone()))
        .collect();
    let home = dirs::home_dir().unwrap_or_default();
    let jobs = sessions::scan_jobs(&home.join(".claude/jobs")).unwrap_or_default();
    let interactive = sessions::scan_interactive(
        &home.join(".claude/projects"),
        &known,
        saved.session_window.cutoff_seconds(),
    )
    .unwrap_or_default();
    sessions::attach_to_worktrees(&mut projects, jobs);
    sessions::attach_to_worktrees(&mut projects, interactive);

    let expanded: std::collections::HashSet<String> = if saved.expanded.is_empty() {
        projects.iter().map(|p| p.name.clone()).collect()
    } else {
        saved.expanded.clone().into_iter().collect()
    };
    let selected = resolve_saved_selection(&projects, &saved).or_else(|| first_visible(&projects));

    let session_groups = sessions::build_session_groups(
        &home.join(".claude/jobs"),
        &home.join(".claude/projects"),
        &known,
        saved.session_window.cutoff_seconds(),
    );
    let selected_in_view = if session_groups.is_empty() {
        None
    } else {
        // Initial selection: first group's header so user sees the structure.
        Some((0, None))
    };
    // Default to all groups expanded so the view shows everything at first.
    let expanded_groups: std::collections::HashSet<String> = session_groups
        .iter()
        .map(|g| g.cwd.to_string_lossy().to_string())
        .collect();

    Ok(AppState {
        projects,
        selected,
        focus: Pane::Worktrees,
        filter: saved.filter,
        search: saved.search.clone(),
        last_refresh: Instant::now(),
        status: StatusLine::default(),
        expanded,
        generation: 0,
        selected_session: None,
        session_window: saved.session_window,
        view: crate::model::ViewMode::Tree,
        session_groups,
        selected_in_view,
        expanded_groups,
    })
}

/// Try to map a persisted selection back onto the freshly discovered projects.
/// Returns None if the saved project/worktree no longer exists.
fn resolve_saved_selection(
    projects: &[Project],
    saved: &crate::config::SavedState,
) -> Option<TreePath> {
    let proj_name = saved.selected_project.as_ref()?;
    let p = projects.iter().find(|p| &p.name == proj_name)?;
    match &saved.selected_worktree {
        Some(wt_path) => {
            let wp = std::path::PathBuf::from(wt_path);
            let w = p.worktrees.iter().find(|w| w.path == wp)?;
            Some(TreePath::worktree_row(p, w))
        }
        None => Some(TreePath::project_header(p)),
    }
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
    let interactive = sessions::scan_interactive(
        &home.join(".claude/projects"),
        &known,
        state.session_window.cutoff_seconds(),
    )
    .unwrap_or_default();
    sessions::attach_to_worktrees(&mut projects, jobs);
    sessions::attach_to_worktrees(&mut projects, interactive);

    state.session_groups = sessions::build_session_groups(
        &home.join(".claude/jobs"),
        &home.join(".claude/projects"),
        &known,
        state.session_window.cutoff_seconds(),
    );
    // Reconcile the carried-over selection against the rebuilt groups: a refresh
    // or a narrower session window can shrink the group list, leaving a stale
    // out-of-range index that would panic the very next render. Reset it.
    state.selected_in_view = match state.selected_in_view {
        _ if state.session_groups.is_empty() => None,
        Some((gi, _)) if gi >= state.session_groups.len() => Some((0, None)),
        Some((gi, Some(si))) if si >= state.session_groups[gi].sessions.len() => Some((gi, None)),
        Some(sel) => Some(sel),
        None => Some((0, None)),
    };
    // Newly discovered groups default to expanded so the user sees content.
    for g in &state.session_groups {
        state.expanded_groups.insert(g.cwd.to_string_lossy().to_string());
    }
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
    let footer_rows = footer_height(area.width, ui);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(60),
            Constraint::Min(0),
            Constraint::Length(footer_rows),
        ])
        .split(area);
    let cols = layout::choose_columns(area.width);

    match state.view {
        crate::model::ViewMode::Tree => {
            crate::ui::worktrees::render(f, chunks[0], state, cols);
            crate::ui::sessions::render(f, chunks[1], state);
        }
        crate::model::ViewMode::Sessions => {
            crate::ui::sessions_view::render(f, chunks[0], state);
            render_sessions_view_detail(f, chunks[1], state);
        }
    }
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
        InputMode::Detail { title, lines, scroll } => {
            let modal_area = centered_rect(area, 94, 90);
            crate::ui::modal::render_detail(f, modal_area, title, lines, *scroll);
        }
        _ => {}
    }
}

const FOOTER_KEYS: &str =
    "? help · v view · 1/2 pane · ↑↓/jk · ←→/hl tree · ↵ · e expand-all · c copy · o open · d danger-open · r refresh · / filter · a active · t window · g log · x del-bg · X bulk-del · u undo · q quit";

/// How many rows the bottom footer needs at this terminal width and mode.
/// Modal/help modes return 0 (their own title_bottom hosts the hint).
/// Search mode returns 1. Normal mode word-wraps FOOTER_KEYS.
fn footer_height(width: u16, ui: &UiState) -> u16 {
    match &ui.mode {
        InputMode::Modal { .. } | InputMode::Help { .. } | InputMode::Detail { .. } => 0,
        InputMode::Search(_) => 1,
        InputMode::Normal => {
            let chars = FOOTER_KEYS.chars().count() as u16;
            let w = width.max(1);
            // Ceiling division. Cap at 4 rows so the footer never eats
            // more than ~1/6 of a phone screen.
            ((chars + w - 1) / w).clamp(1, 4)
        }
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
    use ratatui::widgets::{Paragraph, Wrap};

    match &ui.mode {
        InputMode::Search(buf) => {
            f.render_widget(
                Paragraph::new(format!("/ {buf}_  Enter apply · Esc cancel")),
                area,
            );
            return;
        }
        InputMode::Modal { .. } | InputMode::Help { .. } | InputMode::Detail { .. } => {
            return;
        }
        InputMode::Normal => {}
    }

    // Compose: keymap (wraps) + any transient status + "narrow" warn.
    let mut text = FOOTER_KEYS.to_string();
    if cols.too_narrow {
        text.push_str("  [narrow]");
    }
    if let Some(msg) = state.status.current() {
        text.push_str("  · ");
        text.push_str(msg);
    }

    f.render_widget(
        Paragraph::new(Line::from(Span::raw(text))).wrap(Wrap { trim: true }),
        area,
    );
}

/// Bottom pane in Sessions view: live preview of the selected session's
/// recent content. For interactive sessions, shows the last 3 user/
/// assistant turns. For background jobs, shows intent + tail of the
/// timeline. Press Enter for the structured detail modal.
fn render_sessions_view_detail(f: &mut ratatui::Frame, area: Rect, state: &AppState) {
    use ratatui::text::{Line, Span};
    use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
    use ratatui::style::{Modifier, Style};

    let title = match (current_selected_is_group_header(state), current_session_in_view(state)) {
        (Some(gi), _) => {
            let g = &state.session_groups[gi];
            format!(
                "GROUP · {} ({} sess)  ↵ to toggle",
                g.cwd.display(),
                g.sessions.len(),
            )
        }
        (None, Some(_)) => "SELECTED SESSION  ↵ for full detail".to_string(),
        _ => "SELECTED SESSION".to_string(),
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(title, crate::ui::theme::pane_header()));

    let lines: Vec<Line> = match current_session_in_view(state) {
        Some(s) => session_preview_lines(s),
        None => {
            // Group header is selected (or nothing). Surface group-level info.
            if let Some(gi) = current_selected_is_group_header(state) {
                group_preview_lines(&state.session_groups[gi])
            } else {
                vec![Line::from(Span::styled(
                    "(no session selected — press v if you don't see any)",
                    crate::ui::theme::dim_footer(),
                ))]
            }
        }
    };
    // Add a usage hint at the bottom if there is room. We just append a dim
    // hint line so it doesn't crowd narrow widths.
    let mut all_lines = lines;
    all_lines.push(Line::from(""));
    all_lines.push(Line::from(Span::styled(
        "[↵ full detail · c copy launch · o open · d danger-open · / filter]",
        crate::ui::theme::dim_footer(),
    )));

    let _ = (Modifier::BOLD, Style::default()); // keep imports tidy
    f.render_widget(
        Paragraph::new(all_lines).block(block).wrap(Wrap { trim: false }),
        area,
    );
}

/// Lines for a group-header selection: cwd, worktree presence, count,
/// and the n most-recent session ids inside the group.
fn group_preview_lines(g: &crate::model::SessionGroup) -> Vec<ratatui::text::Line<'static>> {
    use ratatui::style::{Modifier, Style};
    use ratatui::text::{Line, Span};
    let marker = if g.has_worktree { "● worktree" } else { "○ no worktree" };
    let mut out = vec![
        Line::from(vec![
            Span::styled(
                "Directory: ",
                Style::default().add_modifier(Modifier::BOLD).fg(crate::ui::theme::pink()),
            ),
            Span::raw(g.cwd.display().to_string()),
        ]),
        Line::from(vec![
            Span::styled(
                "Status:    ",
                Style::default().add_modifier(Modifier::BOLD).fg(crate::ui::theme::pink()),
            ),
            Span::raw(format!("{marker} · {} sessions", g.sessions.len())),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "Recent sessions in this group:",
            crate::ui::theme::dim_footer(),
        )),
    ];
    for s in g.sessions.iter().take(8) {
        let (kind, id, when) = match s {
            Session::Interactive { id, mtime, .. } => (
                "💬 int",
                id.clone(),
                crate::ui::sessions::fmt_when_short(*mtime),
            ),
            Session::BackgroundJob { id, mtime, .. } => (
                "⚙ bg ",
                id.clone(),
                crate::ui::sessions::fmt_when_short(*mtime),
            ),
        };
        out.push(Line::from(vec![
            Span::raw("  "),
            Span::raw(kind.to_string()),
            Span::raw("  "),
            Span::raw(short_id(&id)),
            Span::raw("  "),
            Span::raw(when),
        ]));
    }
    out
}

fn short_id(id: &str) -> String {
    let take: String = id.chars().take(8).collect();
    if id.chars().count() > 8 {
        format!("{take}…")
    } else {
        take
    }
}

/// Recent content for a single session, formatted as a Vec<Line>.
fn session_preview_lines(s: &Session) -> Vec<ratatui::text::Line<'static>> {
    use ratatui::style::{Modifier, Style};
    use ratatui::text::{Line, Span};
    match s {
        Session::Interactive { id, cwd, mtime, .. } => {
            let mut out = vec![
                Line::from(vec![
                    Span::styled(
                        "Session: ",
                        Style::default().add_modifier(Modifier::BOLD).fg(crate::ui::theme::pink()),
                    ),
                    Span::raw(short_id(id)),
                    Span::raw("  "),
                    Span::styled(
                        crate::ui::sessions::fmt_when_detail(*mtime),
                        crate::ui::theme::dim_footer(),
                    ),
                ]),
                Line::from(vec![
                    Span::styled(
                        "CWD:     ",
                        Style::default().add_modifier(Modifier::BOLD).fg(crate::ui::theme::pink()),
                    ),
                    Span::raw(cwd.display().to_string()),
                ]),
                Line::from(""),
                Line::from(Span::styled(
                    "Recent messages:",
                    crate::ui::theme::dim_footer(),
                )),
            ];
            let home = dirs::home_dir().unwrap_or_default();
            let projects_dir = home.join(".claude/projects");
            let path = crate::sessions::jsonl_path_for(&projects_dir, cwd, id);
            let turns = crate::sessions::last_turns_from_jsonl(&path, 3);
            if turns.is_empty() {
                out.push(Line::from(Span::styled(
                    "  (no readable turns in jsonl)",
                    crate::ui::theme::dim_footer(),
                )));
            } else {
                for (role, content) in turns {
                    let role_style = if role == "user" {
                        Style::default()
                            .add_modifier(Modifier::BOLD)
                            .fg(crate::ui::theme::magenta())
                    } else {
                        Style::default()
                            .add_modifier(Modifier::BOLD)
                            .fg(crate::ui::theme::lavender())
                    };
                    out.push(Line::from(Span::styled(format!("  [{role}]"), role_style)));
                    // The Paragraph::wrap will handle the long line; we still
                    // hard-trim massive pastes to keep the pane scannable.
                    let body = if content.chars().count() > 800 {
                        let head: String = content.chars().take(800).collect();
                        format!("  {head}…")
                    } else {
                        format!("  {content}")
                    };
                    out.push(Line::from(body));
                }
            }
            out
        }
        Session::BackgroundJob { id, intent, status, mtime, .. } => {
            let mut out = vec![
                Line::from(vec![
                    Span::styled(
                        "Job:    ",
                        Style::default().add_modifier(Modifier::BOLD).fg(crate::ui::theme::pink()),
                    ),
                    Span::raw(short_id(id)),
                    Span::raw("  "),
                    Span::styled(
                        crate::ui::sessions::fmt_when_detail(*mtime),
                        crate::ui::theme::dim_footer(),
                    ),
                    Span::raw("  "),
                    Span::styled(format!("{status:?}").to_lowercase(), crate::ui::theme::status_icon()),
                ]),
                Line::from(vec![
                    Span::styled(
                        "Intent: ",
                        Style::default().add_modifier(Modifier::BOLD).fg(crate::ui::theme::pink()),
                    ),
                    Span::raw(intent.clone().unwrap_or_else(|| "(none)".into())),
                ]),
                Line::from(""),
                Line::from(Span::styled(
                    "Recent timeline:",
                    crate::ui::theme::dim_footer(),
                )),
            ];
            let jobs_dir = dirs::home_dir().unwrap_or_default().join(".claude/jobs");
            let tail = crate::sessions::last_timeline_lines(&jobs_dir, id, 5);
            if tail.is_empty() {
                out.push(Line::from(Span::styled(
                    "  (no timeline.jsonl)",
                    crate::ui::theme::dim_footer(),
                )));
            } else {
                for line in tail {
                    let trimmed: String = line.chars().take(400).collect();
                    out.push(Line::from(format!("  {trimmed}")));
                }
            }
            out
        }
    }
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
            let close = handle_scroll_key(scroll, crate::ui::modal::help_line_count(), &key);
            if close {
                ui.mode = InputMode::Normal;
            }
            return Ok(None);
        }
        InputMode::Detail { lines, scroll, .. } => {
            // Each row prints 2 visual lines (key/value + blank between).
            let total = (lines.len() as u16).saturating_mul(2);
            let close = handle_scroll_key(scroll, total, &key);
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
                // Copy first (best-effort; OSC 52 isn't honoured by all
                // terminals). Then print to stdout and exit so the user
                // has the command both ways.
                let _ = actions::copy_to_clipboard(&cmd);
                return Ok(Some(RunOutcome::PrintAndExit(cmd)));
            }
            Ok(None)
        }
        // Bind both cases. Speech-to-text often capitalises and we'd
        // rather forgive that than make the user retype.
        (KeyCode::Char('d') | KeyCode::Char('D'), _) => {
            if let Some(cmd) = launch_for_selected(state, true) {
                let _ = actions::copy_to_clipboard(&cmd);
                return Ok(Some(RunOutcome::PrintAndExit(cmd)));
            }
            Ok(None)
        }
        (KeyCode::Char('?'), _) => {
            ui.mode = InputMode::Help { scroll: 0 };
            Ok(None)
        }
        // Bind both cases so caps-lock / phone-keyboard quirks don't
        // hide the key. 'e' is unused elsewhere so no conflict.
        (KeyCode::Char('E') | KeyCode::Char('e'), _) => {
            toggle_expand_all(state);
            Ok(None)
        }
        (KeyCode::Char('v') | KeyCode::Char('V'), _) => {
            state.view = match state.view {
                crate::model::ViewMode::Tree => crate::model::ViewMode::Sessions,
                crate::model::ViewMode::Sessions => crate::model::ViewMode::Tree,
            };
            let label = match state.view {
                crate::model::ViewMode::Tree => "view: tree",
                crate::model::ViewMode::Sessions => "view: sessions",
            };
            state.status.say(label);
            Ok(None)
        }
        (KeyCode::Char('t') | KeyCode::Char('T'), _) => {
            state.session_window = state.session_window.cycle();
            state.status.say(format!("window: {}", state.session_window.label()));
            refresh_in_place(state, default_projects_roots())?;
            Ok(None)
        }
        (KeyCode::Char('x'), _) => {
            handle_delete_job(state, ui)?;
            Ok(None)
        }
        (KeyCode::Char('X'), _) => {
            handle_bulk_delete(state, ui)?;
            Ok(None)
        }
        (KeyCode::Char('u') | KeyCode::Char('U'), _) => {
            handle_undo_delete(state, ui)?;
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
            // Sessions view:
            //   On a group header: toggle expand/collapse for that group.
            //   On a session row: open detail modal.
            // Tree view: focused-pane dependent.
            if state.view == crate::model::ViewMode::Sessions {
                if let Some(gi) = current_selected_is_group_header(state) {
                    toggle_group_expansion(state, gi);
                } else if let Some((title, lines)) = build_session_detail_in_view(state) {
                    ui.mode = InputMode::Detail { title, lines, scroll: 0 };
                }
            } else if state.focus == Pane::Sessions {
                if let Some((title, lines)) = build_session_detail(state) {
                    ui.mode = InputMode::Detail { title, lines, scroll: 0 };
                }
            } else {
                handle_enter(state);
            }
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
        (KeyCode::Right, _) | (KeyCode::Char('l'), _) => {
            handle_expand(state);
            Ok(None)
        }
        (KeyCode::Left, _) | (KeyCode::Char('h'), _) => {
            handle_collapse(state);
            Ok(None)
        }
        (KeyCode::Tab, _) | (KeyCode::BackTab, _) => {
            state.focus = next_pane(state.focus);
            Ok(None)
        }
        // Direct pane jumps. Easier than Tab on phone keyboards where Tab
        // is buried or hands behaviour to the keyboard manager (Termius).
        (KeyCode::Char('1'), _) => {
            state.focus = Pane::Worktrees;
            Ok(None)
        }
        (KeyCode::Char('2'), _) => {
            state.focus = Pane::Sessions;
            if state.selected_session.is_none() {
                if let Some(wt) = current_worktree(state) {
                    if !wt.sessions.is_empty() {
                        state.selected_session = Some(0);
                    }
                }
            }
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
    let max = total.saturating_sub(1);
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Enter => return true,
        KeyCode::Down | KeyCode::Char('j') => {
            *scroll = scroll.saturating_add(1).min(max);
        }
        KeyCode::Up | KeyCode::Char('k') => {
            *scroll = scroll.saturating_sub(1);
        }
        KeyCode::PageDown | KeyCode::Char(' ') => {
            *scroll = scroll.saturating_add(10).min(max);
        }
        KeyCode::PageUp | KeyCode::Char('b') => {
            *scroll = scroll.saturating_sub(10);
        }
        KeyCode::Char('g') | KeyCode::Home => {
            *scroll = 0;
        }
        KeyCode::Char('G') | KeyCode::End => {
            *scroll = max;
        }
        _ => {}
    }
    false
}

/// Delete the selected background job. Requires:
/// - sessions pane is focused
/// - selected session is a Session::BackgroundJob
/// - job status is Completed or Failed (never deletes Running)
/// - two presses of `x` within 3 seconds on the same job (confirm)
fn handle_delete_job(state: &mut AppState, ui: &mut UiState) -> Result<()> {
    if state.focus != Pane::Sessions {
        state.status.say("focus sessions pane first (Tab)");
        return Ok(());
    }
    let wt = match current_worktree(state) {
        Some(w) => w,
        None => return Ok(()),
    };
    let sel_idx = state.selected_session.unwrap_or(0);
    let sess = match wt.sessions.get(sel_idx) {
        Some(s) => s.clone(),
        None => return Ok(()),
    };
    let (id, status) = match sess {
        Session::BackgroundJob { id, status, .. } => (id, status),
        Session::Interactive { .. } => {
            state.status.say("delete only works on bg jobs");
            return Ok(());
        }
    };
    if matches!(status, crate::model::JobStatus::Running) {
        state.status.say("can't delete a running job");
        return Ok(());
    }

    // Two-step confirm.
    let now = Instant::now();
    let armed = ui
        .pending_delete
        .as_ref()
        .is_some_and(|(pid, when)| pid == &id && now.duration_since(*when).as_secs() < 3);
    if !armed {
        ui.pending_delete = Some((id.clone(), now));
        state.status.say(format!("press x again within 3s to delete {id}"));
        return Ok(());
    }

    // Confirmed. Soft-delete: move to trash dir rather than rm -rf.
    let job_dir = dirs::home_dir().unwrap_or_default().join(".claude/jobs").join(&id);
    match soft_delete_to_trash(&job_dir) {
        Ok(trash_path) => {
            ui.trash_history.push((job_dir, trash_path));
            // Keep the trash history small so we don't accumulate forever.
            if ui.trash_history.len() > 50 {
                ui.trash_history.remove(0);
            }
            state.status.say(format!("deleted {id} (u to undo)"));
            ui.pending_delete = None;
            let _ = refresh_in_place(state, default_projects_roots());
        }
        Err(e) => {
            state.status.say(format!("delete failed: {e}"));
            ui.pending_delete = None;
        }
    }
    Ok(())
}

/// Move a directory into ~/.config/wt/trash/ under a timestamped name so
/// collisions across runs are impossible. Returns the new path so the
/// caller can record it for undo.
fn soft_delete_to_trash(src: &std::path::Path) -> std::io::Result<PathBuf> {
    let trash_root = dirs::config_dir()
        .unwrap_or_default()
        .join("wt")
        .join("trash");
    std::fs::create_dir_all(&trash_root)?;
    let name = src
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "unnamed".into());
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let dest = trash_root.join(format!("{name}.{ts}"));
    std::fs::rename(src, &dest)?;
    Ok(dest)
}

/// Restore the most recent soft-deleted job, if any.
fn handle_undo_delete(state: &mut AppState, ui: &mut UiState) -> Result<()> {
    match ui.trash_history.pop() {
        Some((original, trash)) => {
            if let Some(parent) = original.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            match std::fs::rename(&trash, &original) {
                Ok(_) => {
                    let id = original
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    state.status.say(format!("restored {id}"));
                    let _ = refresh_in_place(state, default_projects_roots());
                }
                Err(e) => {
                    // Put it back on history so a future undo could retry.
                    ui.trash_history.push((original, trash));
                    state.status.say(format!("restore failed: {e}"));
                }
            }
        }
        None => state.status.say("nothing to undo"),
    }
    Ok(())
}

/// Bulk delete every Completed or Failed bg job attached to the currently
/// selected worktree. Running jobs are skipped. Two-press confirm.
fn handle_bulk_delete(state: &mut AppState, ui: &mut UiState) -> Result<()> {
    let now = Instant::now();
    let armed = ui
        .pending_bulk_delete
        .is_some_and(|when| now.duration_since(when).as_secs() < 3);

    let wt = match current_worktree(state) {
        Some(w) => w,
        None => {
            state.status.say("no worktree selected");
            return Ok(());
        }
    };

    let candidates: Vec<String> = wt
        .sessions
        .iter()
        .filter_map(|s| match s {
            Session::BackgroundJob { id, status, .. }
                if matches!(
                    status,
                    crate::model::JobStatus::Completed | crate::model::JobStatus::Failed
                ) =>
            {
                Some(id.clone())
            }
            _ => None,
        })
        .collect();

    if candidates.is_empty() {
        state.status.say("no completed/failed bg jobs here");
        ui.pending_bulk_delete = None;
        return Ok(());
    }

    if !armed {
        ui.pending_bulk_delete = Some(now);
        state.status.say(format!(
            "press X again within 3s to delete {} bg jobs",
            candidates.len()
        ));
        return Ok(());
    }

    // Confirmed. Soft-delete each candidate, accumulate trash history.
    let mut deleted = 0usize;
    let mut errors = 0usize;
    for id in &candidates {
        let job_dir = dirs::home_dir()
            .unwrap_or_default()
            .join(".claude/jobs")
            .join(id);
        match soft_delete_to_trash(&job_dir) {
            Ok(trash_path) => {
                ui.trash_history.push((job_dir, trash_path));
                deleted += 1;
            }
            Err(_) => errors += 1,
        }
    }
    if ui.trash_history.len() > 50 {
        let overflow = ui.trash_history.len() - 50;
        ui.trash_history.drain(0..overflow);
    }
    ui.pending_bulk_delete = None;
    if errors == 0 {
        state.status.say(format!("deleted {deleted} bg jobs (u to undo)"));
    } else {
        state
            .status
            .say(format!("deleted {deleted}, failed {errors}"));
    }
    let _ = refresh_in_place(state, default_projects_roots());
    Ok(())
}

/// If any project is collapsed, expand all. Otherwise collapse all.
/// The "all collapsed" state empties the expanded set; expansion
/// re-fills it with every project name.
fn toggle_expand_all(state: &mut AppState) {
    if state.view == crate::model::ViewMode::Sessions {
        toggle_expand_all_groups(state);
        return;
    }
    let all_expanded = state.projects.iter().all(|p| state.expanded.contains(&p.name));
    if all_expanded {
        state.expanded.clear();
        state.status.say("collapsed all");
    } else {
        state.expanded = state.projects.iter().map(|p| p.name.clone()).collect();
        state.status.say("expanded all");
    }
}

fn toggle_expand_all_groups(state: &mut AppState) {
    let all_expanded = state
        .session_groups
        .iter()
        .all(|g| state.expanded_groups.contains(&g.cwd.to_string_lossy().to_string()));
    if all_expanded {
        state.expanded_groups.clear();
        state.status.say("collapsed all groups");
    } else {
        state.expanded_groups = state
            .session_groups
            .iter()
            .map(|g| g.cwd.to_string_lossy().to_string())
            .collect();
        state.status.say("expanded all groups");
    }
}

fn toggle_group_expansion(state: &mut AppState, group_idx: usize) {
    let Some(g) = state.session_groups.get(group_idx) else {
        return;
    };
    let key = g.cwd.to_string_lossy().to_string();
    if state.expanded_groups.contains(&key) {
        state.expanded_groups.remove(&key);
    } else {
        state.expanded_groups.insert(key);
    }
}

/// Right arrow / l: expand a collapsed project; on an already-expanded
/// project, step into its first worktree; on a worktree row, focus the
/// sessions pane (mirrors Enter).
fn handle_expand(state: &mut AppState) {
    if state.view == crate::model::ViewMode::Sessions {
        if let Some(gi) = current_selected_is_group_header(state) {
            let key = state.session_groups[gi].cwd.to_string_lossy().to_string();
            state.expanded_groups.insert(key);
        }
        return;
    }
    if state.focus != Pane::Worktrees {
        return;
    }
    let Some(sel) = state.selected.clone() else {
        return;
    };
    if sel.worktree.is_none() {
        if state.expanded.contains(&sel.project) {
            // Already expanded: step to first child.
            if let Some(p) = state.projects.iter().find(|p| p.name == sel.project) {
                if let Some(first_wt) = p.worktrees.first() {
                    state.selected = Some(TreePath::worktree_row(p, first_wt));
                    state.selected_session = None;
                }
            }
        } else {
            state.expanded.insert(sel.project.clone());
        }
    } else {
        state.focus = Pane::Sessions;
        if state.selected_session.is_none() {
            if let Some(wt) = current_worktree(state) {
                if !wt.sessions.is_empty() {
                    state.selected_session = Some(0);
                }
            }
        }
    }
}

/// Left arrow / h: collapse an expanded project; on a worktree row, move
/// selection up to the parent project header (without collapsing).
fn handle_collapse(state: &mut AppState) {
    if state.view == crate::model::ViewMode::Sessions {
        match state.selected_in_view {
            Some((gi, None)) => {
                // Group header: collapse it (guard a possibly-stale index).
                if let Some(g) = state.session_groups.get(gi) {
                    let key = g.cwd.to_string_lossy().to_string();
                    state.expanded_groups.remove(&key);
                }
            }
            Some((gi, Some(_))) => {
                // Session row: jump up to its group header.
                state.selected_in_view = Some((gi, None));
            }
            None => {}
        }
        return;
    }
    if state.focus != Pane::Worktrees {
        return;
    }
    let Some(sel) = state.selected.clone() else {
        return;
    };
    if sel.worktree.is_none() {
        state.expanded.remove(&sel.project);
    } else {
        state.selected = Some(TreePath {
            project: sel.project,
            worktree: None,
        });
        state.selected_session = None;
    }
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
    use crate::model::{JobStatus, Project, Session, SessionState, SessionWindow, Worktree};
    use std::collections::HashSet;
    use std::path::PathBuf;
    use std::time::Instant;

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
            size_bytes: 0,
        };
        let wt = Worktree {
            path: PathBuf::from("/p/alpha"),
            branch: Some("main".into()),
            dirty: false,
            ahead: 0,
            behind: 0,
            recent_commits: vec![],
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
            session_window: SessionWindow::Days30,
            view: crate::model::ViewMode::Tree,
            session_groups: vec![],
            selected_in_view: None,
            expanded_groups: std::collections::HashSet::new(),
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
    fn scroll_key_clamps_at_total() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut scroll = 0u16;
        // Press j 100 times against a 10-line total; we should stop at 9.
        for _ in 0..100 {
            let _ = handle_scroll_key(
                &mut scroll,
                10,
                &KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE),
            );
        }
        assert_eq!(scroll, 9);
    }

    #[test]
    fn scroll_key_pgdn_clamps_at_total() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut scroll = 0u16;
        for _ in 0..50 {
            let _ = handle_scroll_key(
                &mut scroll,
                10,
                &KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE),
            );
        }
        assert_eq!(scroll, 9);
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
            recent_commits: vec![],
            sessions: vec![],
            has_upstream: false,
        };
        state.projects[0].worktrees.push(wt2);
        state.selected_session = Some(3); // pretend we had one selected

        move_selection(&mut state, 1); // moves worktree selection in Worktrees pane
        assert_eq!(state.selected_session, None, "session selection must reset on worktree change");
    }

    #[test]
    fn stale_group_index_does_not_panic() {
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
        // Stale selection pointing past the rebuilt group list.
        state.selected_in_view = Some((999, None));
        // Render-path helper must refuse the out-of-range index (no panic).
        assert_eq!(current_selected_is_group_header(&state), None);
        // A refresh must reconcile the stale index to something valid (or None).
        refresh_in_place(&mut state, roots).unwrap();
        if let Some((gi, _)) = state.selected_in_view {
            assert!(gi < state.session_groups.len(), "reconciled index in range");
        }
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

/// Build the (title, key-value lines) payload for the Detail modal from the
/// currently selected session. Returns None if no session is selected.
fn build_session_detail_in_view(state: &AppState) -> Option<(String, Vec<(String, String)>)> {
    let sess = current_session_in_view(state)?;
    let cwd = crate::sessions::session_cwd(sess).to_path_buf();
    let title = format!("SESSION · {}", cwd.display());
    let lines = session_to_detail_lines(sess);
    Some((title, lines))
}

fn session_to_detail_lines(sess: &Session) -> Vec<(String, String)> {
    match sess {
        Session::Interactive { id, summary, cwd, mtime, state: sstate, .. } => vec![
            ("ID".into(), id.clone()),
            ("Type".into(), "interactive".into()),
            ("State".into(), format!("{sstate:?}").to_lowercase()),
            ("When".into(), crate::ui::sessions::fmt_when_detail(*mtime)),
            ("CWD".into(), cwd.to_string_lossy().to_string()),
            ("Summary".into(), summary.clone()),
        ],
        Session::BackgroundJob { id, status, cwd, mtime, intent, size_bytes, .. } => vec![
            ("ID".into(), id.clone()),
            ("Type".into(), "background job".into()),
            ("Status".into(), format!("{status:?}").to_lowercase()),
            ("When".into(), crate::ui::sessions::fmt_when_detail(*mtime)),
            ("CWD".into(), cwd.to_string_lossy().to_string()),
            ("Size".into(), crate::ui::sessions::fmt_bytes_pub(*size_bytes)),
            ("Intent".into(), intent.clone().unwrap_or_else(|| "(none)".into())),
        ],
    }
}

fn build_session_detail(state: &AppState) -> Option<(String, Vec<(String, String)>)> {
    let wt = current_worktree(state)?;
    let idx = state.selected_session?;
    let sess = wt.sessions.get(idx)?;
    let project = state.selected.as_ref().map(|s| s.project.clone()).unwrap_or_default();
    let wt_name = wt.path.file_name().and_then(|s| s.to_str()).unwrap_or("?");
    Some((format!("SESSION · {project}/{wt_name}"), session_to_detail_lines(sess)))
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
    // Sessions view: navigation is a flat walk through all (group, session)
    // pairs. Tree view: routes to worktrees/sessions panes as before.
    if state.view == crate::model::ViewMode::Sessions {
        move_in_sessions_view(state, delta);
        return;
    }
    match state.focus {
        Pane::Worktrees => move_worktree_selection(state, delta),
        Pane::Sessions => move_session_selection(state, delta),
    }
}

/// Build a flat ordered list of currently visible rows in sessions view.
/// Each row is (group_idx, Option<session_idx>) where None = group header
/// and Some(i) = the i-th session of that group (only when expanded).
/// Respects the global search filter (state.search).
pub fn visible_rows_in_view(state: &AppState) -> Vec<(usize, Option<usize>)> {
    let mut out = Vec::new();
    let q = search_lowercase(state);
    for (gi, g) in state.session_groups.iter().enumerate() {
        if !group_matches_search(g, q.as_deref()) {
            continue;
        }
        out.push((gi, None));
        if state.expanded_groups.contains(&g.cwd.to_string_lossy().to_string()) {
            for si in 0..g.sessions.len() {
                out.push((gi, Some(si)));
            }
        }
    }
    out
}

fn search_lowercase(state: &AppState) -> Option<String> {
    state.search.as_ref().filter(|s| !s.is_empty()).map(|s| s.to_lowercase())
}

/// True if a group matches the current search: either its cwd contains the
/// query, or any session in it does (by id, cwd, or summary/intent).
pub fn group_matches_search(g: &crate::model::SessionGroup, q: Option<&str>) -> bool {
    let Some(q) = q else { return true; };
    if g.cwd.to_string_lossy().to_lowercase().contains(q) {
        return true;
    }
    g.sessions.iter().any(|s| session_matches_search(s, q))
}

fn session_matches_search(s: &Session, q: &str) -> bool {
    match s {
        Session::BackgroundJob { id, intent, cwd, .. } => {
            id.to_lowercase().contains(q)
                || cwd.to_string_lossy().to_lowercase().contains(q)
                || intent.as_deref().map(|i| i.to_lowercase().contains(q)).unwrap_or(false)
        }
        Session::Interactive { id, summary, cwd, .. } => {
            id.to_lowercase().contains(q)
                || cwd.to_string_lossy().to_lowercase().contains(q)
                || summary.to_lowercase().contains(q)
        }
    }
}

fn move_in_sessions_view(state: &mut AppState, delta: i32) {
    let rows = visible_rows_in_view(state);
    if rows.is_empty() {
        return;
    }
    let cur_idx = state
        .selected_in_view
        .and_then(|sel| rows.iter().position(|&r| r == sel))
        .unwrap_or(0) as i32;
    let new_idx = (cur_idx + delta).clamp(0, rows.len() as i32 - 1) as usize;
    state.selected_in_view = Some(rows[new_idx]);
}

fn current_session_in_view<'a>(state: &'a AppState) -> Option<&'a Session> {
    let (gi, si_opt) = state.selected_in_view?;
    let g = state.session_groups.get(gi)?;
    g.sessions.get(si_opt?)
}

/// True iff the user has a group header (not a session) selected.
fn current_selected_is_group_header(state: &AppState) -> Option<usize> {
    match state.selected_in_view {
        // Bounds-check: a stale index left over from a shrunk group list must
        // not reach the `session_groups[gi]` indexes on the render path.
        Some((gi, None)) if gi < state.session_groups.len() => Some(gi),
        _ => None,
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
    // Sessions view: act on the selected session's cwd; --resume if interactive.
    if state.view == crate::model::ViewMode::Sessions {
        let sess = current_session_in_view(state)?;
        return Some(match sess {
            Session::Interactive { id, cwd, .. } => {
                actions::launch_command_for(cwd, Some(id), dangerous)
            }
            Session::BackgroundJob { cwd, .. } => {
                actions::launch_command_for(cwd, None, dangerous)
            }
        });
    }

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

