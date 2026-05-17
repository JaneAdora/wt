# wt — Worktree Wizard 🧙

A lightweight terminal dashboard for git worktrees and Claude Code sessions across `~/projects/`, `~/Projects/`, and `~/Projects/clients/`. Built for one terminal tab; designed for mobile SSH (Blink, Termius) with OSC 52 clipboard and width-collapsing.

```
WORKTREES (30 of 48 wt · 29 sess) · filter: active           WT Wizard 🧙
  ▼ ignis (6 of 7 · 9 sess) ●3
    └ ignis           main             ●5
    └ guide-lp        worktree-gu…     ●     ●4
    ...
  ▼ skai-work (4 · 5 sess) ●3
    ↳ skai-work       feat/gdocs-…    ●5

SESSIONS · skai-work/skai-work
  ▸ 💬 int ebe16487… May 16  active   I have a new project for you
    💬 int 8a3f0a12… 1h      active   Continuing from the morning…

RECENT
  fff9cb9  3h        Merge worktree-feat-thelma-check…
  aecdc83  May 14    feat(skai): add thelma check/reply
  e9a2e96  May 13    fix(otter): collapse whitespace…
```

## Install

```
cargo install --path .
```

The binary lands at `~/.cargo/bin/wt`. Run `wt` from anywhere.

## Use

Press `?` inside `wt` for the full keymap. The most common keys:

| Key | What it does |
|---|---|
| `1`, `2` | Focus WORKTREES / SESSIONS pane |
| `Tab` | Cycle focus |
| `v` | Toggle Tree view ↔ Sessions view |
| `↑`/`↓` / `j`/`k` | Move selection |
| `←`/`→` / `h`/`l` | Tree nav (expand/collapse, parent/child) |
| `Enter` | Project: expand/collapse · Worktree: focus sessions · Session: open detail modal |
| `c` | Copy launch command to clipboard (OSC 52) |
| `o` | **Open**: copy + print to terminal + exit |
| `d` | Same as `o` plus `--dangerously-skip-permissions` |
| `r` | Refresh (preserves selection/filter/expansion) |
| `/` | Substring filter (cwd + session content) |
| `a` | Toggle active-only filter |
| `t` | Cycle interactive-session window: 30d → all → 7d |
| `g` | Commit log modal (last 20, word-wrapped) |
| `e` | Expand-all / collapse-all |
| `x` | Soft-delete selected bg job (two-press confirm) |
| `X` | Bulk-delete completed/failed bg jobs in current worktree |
| `u` | Undo last soft-delete (LIFO) |
| `?` | Help overlay |
| `q` / `Ctrl-C` | Quit (saves UI state) |

## Two views

**Tree view (default)** — top pane lists projects and their git worktrees; bottom pane shows Claude sessions attached to the selected worktree plus a RECENT commits block.

**Sessions view (`v`)** — top pane lists every Claude session on the machine grouped by cwd; bottom pane shows a live content preview of the selected session (last 3 user/assistant messages for interactive sessions; recent timeline for background jobs).

Group headers in Sessions view carry markers:
- `●` (pink) = wt found a git worktree for this cwd
- `○` (dim) = orphan dir (sessions exist but no worktree was discovered)

## Discovery scope

Scans the immediate non-hidden subdirs of:

- `~/` (catches repos at home root, e.g., `~/peon-ping`)
- `~/projects/`
- `~/Projects/`
- `~/Projects/clients/`

Each dir is included only if it contains a `.git`. Linked worktrees are picked up via `git worktree list` per repo, so they show up wherever they live on disk.

Override with the `WT_ROOTS` env var, colon-separated like `PATH`:

```
WT_ROOTS=~/projects:~/Projects:~/code wt
```

## Persistent state

UI preferences (filter, expansion sets, search, selection, time window) are saved to `~/.config/wt/state.json` on quit and restored on launch.

Soft-deleted background jobs move to `~/.config/wt/trash/<id>.<unix-ts>/`. Press `u` in the current session to restore the most recent one. Trash entries persist across launches; the `u` undo only works within one session for now.

## Shell wrapper (optional)

To make `o` actually `cd` your shell into the selected worktree:

```bash
# in ~/.bashrc
wtcd() {
    local cmd
    cmd="$(wt)" || return 1
    [ -z "$cmd" ] && return 0
    eval "$cmd"
}
```

Then `wtcd`, navigate, press `o`. Your shell ends up in the chosen dir with `claude` running.

## CLI flags

```
wt              Launch the dashboard
wt --help       Print usage
wt --version    Print version
```

## Architecture

- `discovery` walks the configured roots and `git worktree list`s each repo; `enrich_with_status` runs `git status v2` + `git log -15` per worktree in a thread pool.
- `sessions` reads `~/.claude/jobs/<id>/state.json` (matched to worktrees via `worktreePath`/`cwd`) and `~/.claude/projects/<encoded-cwd>/*.jsonl` (interactive sessions). For Sessions view, `scan_all_interactive` extracts the real cwd from inside the jsonl so orphan dirs show up.
- `actions` emits OSC 52 escape sequences for clipboard copy.
- `ui` renders via `ratatui` with width-aware column collapse and tree-style auto-scroll.
- `app` owns the event loop, key dispatch, and `AppState` mutation. A separate `tick` thread refreshes background-job state every 10 seconds, tagged with a generation counter to drop stale messages after a manual refresh.

## Specs and roadmap

- Design spec: `docs/specs/2026-05-16-wt-dashboard-design.md`
- Implementation plan: `docs/plans/2026-05-16-wt-dashboard-impl.md`
- Format findings: `docs/notes/format-findings.md`
- Roadmap (what's shipped, what's next): `docs/ROADMAP.md`

## License

MIT — see [LICENSE](LICENSE).
