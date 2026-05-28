# wt: Worktree & Session Dashboard

**Status:** Draft for review
**Date:** 2026-05-16
**Author:** Jane Brent (designed with Claude)

## Purpose

A lightweight terminal dashboard that lets Jane keep track of git worktrees across all her projects and the Claude Code sessions attached to them. The primary use case is fast jump-to-session from a phone over SSH; desktop use is a co-equal target. The dashboard occupies one terminal tab and stays running; it does not replace `cd` or `tmux`.

## Non-Goals

- Not a general repo manager (no fetch, pull, push, branch creation, stash actions).
- Not a job manager (no killing background jobs, no log tailing in v1).
- Not a Claude launcher (does not spawn `claude` itself; surfaces commands for the user to run elsewhere).
- Not configurable in v1 (sane defaults; no config file).
- Not portable across hosts (single-host dashboard; reads only the local filesystem).

## Stack

- **Language:** Rust (single static binary, a few MB stripped).
- **TUI:** `ratatui` (latest 0.28+) on `crossterm`.
- **Concurrency:** synchronous, single-threaded; one short-lived thread for the 10-second background tick. No tokio.
- **Git access:** shell out to the `git` CLI via `std::process::Command`. No `libgit2`/`git2` crate (binary bloat, build complexity).
- **Direct dependencies (target: under 10):** `ratatui`, `crossterm`, `anyhow`, `serde`, `serde_json`, `time` (or `jiff`), `dirs`.

## Performance Targets

These are hard targets, not aspirations:

- **First render: under 50 ms** typical for ~12 projects. Initial render shows discovered worktrees with empty status columns; enrichment happens in parallel and populates progressively.
- **Full status enrichment: under 1 s** typical. Per-repo git calls (`status --porcelain=v2` + `log -1`) run in a bounded thread pool so total time is roughly max-per-repo rather than sum.
- Stripped release binary: under 5 MB.
- Resident memory: under 20 MB.
- Background tick CPU: imperceptible (reads ~40 small files every 10 s).

## Layout

Vertical stack. Single surface. Top pane = worktrees, bottom pane = sessions for the focused worktree, plus a one-line status/keymap footer.

```
┌─ wt ─────────────────────────┐
│ WORKTREES                    │
│  ▸ gd2md2html    main   ●    │
│  ▼ example-project        main   ●3   │
│    └ wt-tui      wt-tui ●    │
│  ▸ zele          main        │
│  ▸ voxtral       main   ↑2   │
├──────────────────────────────┤
│ SESSIONS · example-project/main       │
│  ⚙ bg  fe9c…  2m  running    │
│  💬 int 8a3f…  1h  compact    │
│  💬 int 2c11…  3d  archived   │
│  Last: a3f8c "fix gmail…"    │
├──────────────────────────────┤
│ ↑↓ ↵ Tab c o r / a g q       │
└──────────────────────────────┘
```

**Vertical split:** worktree pane gets 60% of available rows, sessions pane gets the rest. Status/key footer is always one row.

**Responsive collapse by terminal width:**

| Width    | Behavior                                                                |
|----------|-------------------------------------------------------------------------|
| 60 col + | Full: name, branch, status icons, session count badge.                  |
| 40-59 col | Drop the branch column; status icons remain.                            |
| 30-39 col | Truncate names to about 12 chars with ellipsis; status icons compact to one. |
| under 30 col | Render but warn in footer ("narrow"); no further degradation.        |

Width is sampled on every redraw, so rotating a phone reflows correctly.

## Discovery

**Worktree pane sources, in this order:**

1. Every immediate non-hidden subdirectory of `~/projects/` that contains a `.git` (file or dir). Dot-prefixed names (e.g., `.cache`) are skipped.
2. For each project root above, `git -C <root> worktree list --porcelain` adds linked worktrees.
3. Each linked worktree appears nested under its project root in the tree view.

**Session sources:**

1. **Background jobs:** scan `~/.claude/jobs/*/` for metadata JSON. Match each job's working directory back to a worktree path. Job status (running, completed, failed) comes from the metadata.
2. **Interactive sessions:** scan `~/.claude/projects/<encoded-cwd>/*.jsonl`. The encoded-cwd is the absolute path with `/` replaced by `-`. For each candidate jsonl: read only the first and last lines (cheap), extract the session id, last-modified timestamp, and a short summary line.
3. Cap interactive sessions at the **last 30 days and the most recent 5 per worktree**, sorted by modified time descending.

**Filtering ("active only" default):**

A worktree is "active" if any of: has 1 or more sessions in scope (bg or interactive), is dirty, or has commits ahead/behind a configured upstream. A worktree with no upstream branch and no other activity counts as inactive. Press `a` to toggle off the filter and see all projects.

## State Model

```rust
struct AppState {
    projects: Vec<Project>,      // discovery output, refreshed on `r`
    selected: TreePath,          // which row in the worktree pane has focus
    focus: Pane,                 // Worktrees | Sessions
    filter: ActiveFilter,        // ActiveOnly | All
    search: Option<String>,      // `/` filter substring
    width: u16, height: u16,     // last-known terminal dims
    last_refresh: Instant,
    status: StatusLine,          // transient message ("copied", "error: ...")
}

struct Project {
    name: String,
    root: PathBuf,
    worktrees: Vec<Worktree>,    // includes the main worktree
}

struct Worktree {
    path: PathBuf,
    branch: Option<String>,
    dirty: bool,
    ahead: u32,
    behind: u32,
    last_commit: Option<CommitSummary>,
    sessions: Vec<Session>,
}

enum Session {
    BackgroundJob { id: String, status: JobStatus, age: Duration },
    Interactive   { id: String, summary: String, age: Duration, state: SessionState },
}
```

`TreePath` identifies a row by `(project_idx, Option<worktree_idx>)`. The selection survives a refresh by path-matching rather than index, so re-ordered worktrees do not dislocate the cursor.

## Refresh Model

- **Snapshot on launch.** Full discovery + git status for every worktree.
- **Manual full refresh** on `r`. Re-runs discovery and git status.
- **Background tick every 10 s** updates only the cheap things: re-reads `~/.claude/jobs/*/` metadata and recomputes the active-session badge. Does **not** call `git status`. This is the part that has to stay imperceptible on mobile.
- Background tick runs in one thread, communicates back via an `mpsc::channel<UpdateMsg>` that the main event loop drains alongside crossterm events. No async runtime needed.

## Actions

| Key   | Action                                                                                      |
|-------|---------------------------------------------------------------------------------------------|
| `up/down` or `j/k` | Move selection in the focused pane.                                            |
| `Tab` / `Shift-Tab` | Switch focus between worktree and session panes.                              |
| `Enter`            | On a project row: expand/collapse. On a worktree row: focus sessions pane.    |
| `c`   | **Copy launch command for selected row to system clipboard via OSC 52.** Primary action.    |
| `o`   | **Print launch command to stdout and exit.** For shell-wrapper use; desktop escape hatch.   |
| `r`   | Full refresh.                                                                               |
| `/`   | Begin substring filter; Esc/Enter to confirm.                                               |
| `a`   | Toggle active-only filter.                                                                  |
| `g`   | Show last 20 commits for the selected worktree in a modal popup.                            |
| `q`   | Quit.                                                                                       |

**Launch command format:**

- Selected = worktree row: `cd <abs-path> && claude`
- Selected = background job: `cd <job-cwd> && claude` (resuming a bg job from outside its harness is not generally meaningful; this falls back to opening Claude in the same directory).
- Selected = interactive session: `cd <session-cwd> && claude --resume <session-id>`

**Clipboard via OSC 52** writes a single escape sequence to stdout: `\x1b]52;c;<base64>\x07`. Blink and Termius both honor this; on desktop terminals that do not (rare), we surface "clipboard unavailable; press `o` to print" in the status line. Detection is best-effort: we always emit OSC 52 and let the terminal honor or drop it. If the user reports no paste, `o` is the fallback.

## Architecture

```
src/
  main.rs           // arg parsing, terminal setup, event loop wiring
  app.rs            // AppState, event handlers, state transitions
  discovery.rs      // walk ~/projects/, find git roots
  git.rs            // shell-out wrappers: worktree list, status v2, log
  sessions.rs       // ~/.claude/jobs + ~/.claude/projects readers
  model.rs          // Project, Worktree, Session, TreePath
  ui/
    mod.rs          // layout dispatch by width
    worktrees.rs    // top pane renderer
    sessions.rs     // bottom pane renderer
    modal.rs        // commit log popup
    theme.rs        // Rep Cap colors
  actions.rs        // clipboard (OSC 52), print-and-exit launcher
```

**Module boundaries:**

- `discovery`, `git`, `sessions` are pure I/O modules that return owned data; no UI, no state.
- `model` defines types shared between I/O and UI.
- `ui/*` reads `&AppState` and produces ratatui widgets; never mutates.
- `app` is the only mutator of `AppState`.
- `actions` is side-effecting (writes stdout, exits) but stateless.

This separation matters because the I/O modules are the ones with real branching logic and deserve real tests; the UI is mostly layout glue and will be eye-tested.

## Error Handling

Discovery and git are best-effort. Failures degrade gracefully, never panic:

- Missing `~/projects/`: empty list, status bar "no projects found".
- `git` binary missing: discovery still finds dirs; branch/status columns show `?`.
- A specific repo's `git status` fails: that row shows `?` for status; other rows continue.
- Malformed job metadata or jsonl: skip that file, increment a "skipped N" counter shown in `g`-modal.
- Clipboard OSC 52 cannot be verified; failure is silent (terminal-dependent).
- Terminal resize below 20 cols: render a minimal "too narrow" message rather than panic.

## Theme

Night-mode-forward, all-foreground accents. No background fills (those compete with terminal themes and look heavy on mobile). The dashboard assumes a dark terminal background but degrades cleanly on light themes because every accent is a foreground color on the terminal's own background.

**Palette:**

| Role                              | Color                          | Weight   |
|-----------------------------------|--------------------------------|----------|
| Pane headers ("WORKTREES", etc.)  | Lavender `#c5a3ff`             | Bold     |
| Active/focused row                | Rep Cap pink `#e88b9f`         | Bold + `▸ ` prefix marker |
| Dirty / ahead / behind indicators | Rep Cap pink `#e88b9f`         | Regular  |
| Session count badge (e.g. `●3`)   | Magenta `#ff6ec7`              | Regular  |
| "Last commit" footer line         | Lavender `#c5a3ff`, dim        | Dim      |
| Borders, separators, body text    | Terminal default               | Default  |
| Status line ("copied", errors)    | Magenta `#ff6ec7` (transient)  | Regular  |

**Why the three accent colors:**

- Lavender (`#c5a3ff`) for structural elements (headers, footer line). Quieter than the pink, so it recedes.
- Rep Cap pink (`#e88b9f`) for content emphasis (active row, status icons). Brand-faithful.
- Magenta (`#ff6ec7`) for transient feedback (session count badge, "copied" toast). Brightest of the three, so it draws the eye.

Rep Cap dark purple `#2e2769` is deliberately not used as a foreground; it's invisible on dark terminal backgrounds. It can return in a future "light mode" theme as a header color.

**Selection vs focus:** the focused pane's border is bold (terminal default); the selected row within it gets the pink fg + `▸ ` marker. This works without any background fill.

## Testing

- **Unit tests:**
  - `git::parse_worktree_list`: given fixture porcelain output, returns expected `Vec<Worktree>`.
  - `git::parse_status_v2`: dirty / clean / ahead / behind permutations.
  - `sessions::scan_jobs`: given a fixture `jobs/` tree, returns expected jobs.
  - `sessions::scan_interactive`: encoded-cwd decoding; cap-by-recency.
  - `ui::layout::choose_columns`: given width, returns expected column set.
  - `actions::osc52_encode`: given a string, produces the expected escape sequence.
- **Integration tests:** none in v1. The full flow is short enough to manually validate.
- **Snapshot tests on rendering:** deferred to v2 (`insta`).

## Build & Install

- `cargo build --release` produces `target/release/wt`.
- `cargo install --path .` installs to `~/.cargo/bin/wt`.
- No system dependencies beyond `git` already being on PATH.

## Open Items (Pre-Implementation)

- **Binary name:** `wt` is short and memorable. There is no collision on Linux (Windows Terminal uses it on Windows only). Pre-implementation Jane can rename to `wtd`, `worktrees`, or anything else with a one-line `Cargo.toml` edit.
- **Encoded-cwd format:** assumed to be absolute path with `/` replaced by `-`. The implementation plan needs to confirm this against actual files in `~/.claude/projects/` before relying on it.
- **Job metadata schema:** the implementation plan needs to inspect a few real `~/.claude/jobs/*/` directories to confirm the field names for `working_directory`, status, and timestamps.

These three items are pinned for the implementation plan to verify; none of them require design changes.

## Out of Scope (v1, may revisit)

- Spawning a new terminal window/tab from inside the dashboard.
- Killing or restarting background jobs from the dashboard.
- Tailing job stdout/stderr in a pane.
- A configuration file (paths, theme, refresh interval).
- Multi-host dashboards (e.g., showing Muthur's worktrees from a different machine).
- Search across session contents (use `cc-session-index` for that).
