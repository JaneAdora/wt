# wt

Terminal dashboard for git worktrees and Claude Code sessions across `~/projects/`. Designed to live in a dedicated terminal tab; works equally well over SSH on a phone (Blink, Termius) thanks to OSC 52 clipboard support and responsive width-collapsing.

## Install

```
cargo install --path .
```

Drops `wt` on your `PATH`.

## Use

```
wt
```

| Key | Action |
|---|---|
| `↑` `↓` or `j` `k` | Move selection |
| `Tab` / `Shift-Tab` | Switch focus between worktree and session panes |
| `Enter` | On project row: expand/collapse. On worktree row: focus sessions pane. |
| `c` | Copy `cd PATH && claude [--resume ID]` to system clipboard via OSC 52 |
| `o` | Print launch command to stdout and exit (for shell wrapper) |
| `r` | Refresh (preserves selection, filter, search) |
| `/` | Substring filter; Esc to cancel, Enter to apply |
| `a` | Toggle active-only filter |
| `g` | Show last 20 commits for selected worktree |
| `q` or `Ctrl-C` | Quit |

## Shell wrapper (optional)

To use `o` to `cd` your current shell into the selected worktree and start Claude:

```bash
# in ~/.bashrc or equivalent
wtcd() {
    local cmd
    cmd="$(wt)" || return 1
    [ -z "$cmd" ] && return 0
    eval "$cmd"
}
```

Then `wtcd`, navigate, press `o`. Your shell ends up in the chosen dir with `claude` running.

## Architecture

- `discovery` walks `~/projects/` for git repos, runs `git worktree list` per repo, then enriches each worktree with `git status v2` + `git log -1` in a `std::thread::scope` so the per-repo wall time is the max single repo, not the sum.
- `sessions` reads `~/.claude/jobs/<id>/state.json` for background jobs (using `worktreePath` as the canonical match key) and `~/.claude/projects/<encoded-cwd>/*.jsonl` for interactive sessions (capped to last 30 days, 5 per worktree). The encoding `/` → `-` and `.` → `-` is irreversible; we only encode known worktree paths and look up matching directories.
- `actions` emits OSC 52 escape sequences to copy launch commands to the system clipboard.
- `ui` renders the dashboard with `ratatui`; layout collapses gracefully at 60/40/30 column breakpoints.
- `app` owns the event loop, key dispatch, and `AppState` mutation. A separate `tick` thread refreshes background-job state every 10 seconds, tagged with a generation counter so stale messages can be dropped after a manual refresh.

## See also

- Design spec: `docs/specs/2026-05-16-wt-dashboard-design.md`
- Implementation plan: `docs/plans/2026-05-16-wt-dashboard-impl.md`
- Format findings: `docs/notes/format-findings.md`
