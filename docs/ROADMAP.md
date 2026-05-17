# wt — Worktree Wizard Roadmap

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                            WT WIZARD ROADMAP                                │
│                                                                             │
│  Project:    wt — Terminal dashboard for git worktrees + Claude sessions    │
│  Owner:      Jane                                                           │
│  Started:    2026-05-16                                                     │
│  v0 status:  shipped — design → 17-task TDD plan → daily-driver in one day  │
│  Phase 1:    closed — UI iteration from real phone use                      │
│  Phase 2:    closed — feature expansion (D-key, time-window, persistence)   │
│  Phase 3:    closed — soft-delete + sessions-first view + branding          │
│  Phase 4:    closed — pre-ship polish + session content preview             │
│  Phase 5:    planning (queued ideas below)                                  │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## What's Shipped

### Phase 0: Design and v0 build

Spec → plan → 17-task TDD implementation, shipped in one day. Vertical 60/Min/1 layout, Rust + ratatui + crossterm, scans `~/projects`, `~/Projects`, `~/Projects/clients` for git repos + linked worktrees, reads `~/.claude/jobs/` + `~/.claude/projects/` for Claude sessions.

| Component                              | Path                                                  |
|----------------------------------------|-------------------------------------------------------|
| Design spec                            | `docs/specs/2026-05-16-wt-dashboard-design.md`        |
| Implementation plan (17 TDD tasks)     | `docs/plans/2026-05-16-wt-dashboard-impl.md`          |
| Format findings (verified ground truth)| `docs/notes/format-findings.md`                       |
| Domain types                           | `src/model.rs`                                        |
| Git porcelain parsers + shell-out      | `src/git.rs`                                          |
| Project + worktree discovery (parallel)| `src/discovery.rs`                                    |
| Session readers (jobs + interactive)   | `src/sessions.rs`                                     |
| OSC 52 clipboard + launch commands     | `src/actions.rs`                                      |
| Rep Cap pink/lavender/magenta theme    | `src/ui/theme.rs`                                     |
| Width-responsive column collapse       | `src/ui/layout.rs`                                    |
| Tree-view top + bottom panes           | `src/ui/worktrees.rs`, `src/ui/sessions.rs`           |
| Modal renderer (log/help/detail)       | `src/ui/modal.rs`                                     |
| App state + event loop + key dispatch  | `src/app.rs`                                          |
| 10-second background tick (jobs only)  | `src/tick.rs`                                         |

**Independent code review caught a real headline bug** before launch: `c` never produced `claude --resume <id>` for interactive sessions. Fixed before iter 0 closed.

### Phase 1: UI iteration from real phone use

Live testing on Termius over SSH surfaced visual + interaction issues invisible on desktop. Closed in three rounds.

| Change                                                      | Why                                              |
|-------------------------------------------------------------|--------------------------------------------------|
| Width-aware session-row truncation                          | Long summaries clipped at the right edge          |
| Date column ("May 14") replacing bare relative age          | More scannable for older items                   |
| Bg-job intent surfaced as description                       | `⚙ bg fe9c… 5m running` was opaque               |
| Vertical layout reflow at 60/40/30-col breakpoints          | Phone portrait was getting clobbered             |
| Log modal full-screen + word wrap + scroll keys             | Modal was overlapping pane content on phone      |
| `?` help overlay with full keymap                           | Footer keymap alone wasn't discoverable          |
| Worktree pane auto-scroll on selection                      | Selection went off-screen with 30+ projects      |
| Counts in titles + project headers                          | Aggregate visibility                             |
| `WT Wizard 🧙` brand in top-right of WORKTREES border       | Cute identity in the TUI                         |
| Tab title = "Worktree Wizard"                               | Identifies the tab in the terminal               |

### Phase 2: Feature expansion

Larger features driven by real workflow needs after the dashboard became daily-driver.

| Feature                                                | Notes                                                  |
|--------------------------------------------------------|--------------------------------------------------------|
| `D`/`d` print + exit with `--dangerously-skip-permissions` | The "do it" key for vetted launches                |
| `t`/`T` cycle interactive-session window (30d/all/7d)  | Time filter                                            |
| Persistent UI state (`~/.config/wt/state.json`)        | Filter, expansion, search, selection, window          |
| `x` soft-delete bg job (two-press confirm)             | Housekeeping for stale jobs                            |
| `o` also copies to clipboard before exiting            | Both clipboard + terminal stdout                       |
| `←` `→` `h` `l` tree navigation                        | Tree-explorer ergonomics                               |
| `1` and `2` direct pane focus                          | Termius-friendly alternative to Tab                    |
| Help reorganized: layout/panes leading                 | "How do I switch views" was unclear                    |
| `visible of total` in titles when filter hides         | Made hidden worktrees visible without flipping filter |

### Phase 3: Soft-delete · sessions-first view · branding refinement

Reworked deletion to be safe + reversible, added the alternative view, and pulled in `jiff` for proper timezone-aware timestamps.

| Feature                                                | Notes                                                  |
|--------------------------------------------------------|--------------------------------------------------------|
| Soft-delete to `~/.config/wt/trash/`                   | `std::fs::rename` instead of `rm -rf`                  |
| `u` undo last soft-delete (LIFO, cap 50)               | Session-local stack                                    |
| `X` bulk-delete completed/failed bg jobs               | With two-press confirm; running jobs skipped           |
| Bytes-on-disk column per bg job                        | Know what you'd free before deleting                   |
| `~/` added to default scan roots                       | Catches `~/peon-ping` etc.                             |
| Sessions-first view (`v` key)                          | All Claude sessions grouped by cwd, regardless of worktree |
| ● / ○ markers for worktree presence per group           | Visual cue on cwds with/without git worktrees          |
| Collapsible groups in Sessions view                    | Same expand/collapse model as projects                 |
| `/` search filters Sessions view too                   | Matches cwd OR session content                         |
| `jiff` for local-tz timestamps                         | `2026-05-16 14:23 CDT` instead of UTC                  |
| Help styling: keys bold pink, headers bold lavender    | Left column jumps out on phone                         |

### Phase 4 (this round): Pre-ship polish + session content preview

| Feature                                                | Notes                                                  |
|--------------------------------------------------------|--------------------------------------------------------|
| Session content preview in Sessions-view bottom pane   | Interactive: last 3 user/assistant turns. Bg job: intent + tail of timeline.jsonl |
| Group preview when group header selected               | Lists most-recent session ids inside the group         |
| Empty-state hints in Sessions view                     | "Press t to widen / Esc to clear filter"               |
| `--version` and `--help` CLI flags                     | Standard CLI polish                                    |
| `LICENSE` file (MIT)                                   | Was declared in Cargo.toml, no file before             |
| README refreshed with full keymap + architecture       | Useful for anyone wt is shared with                    |

**Status as of this commit:** binary is ~1.4 MB; 54 unit tests pass; daily-driver across Tree and Sessions views; persistent UI state; 7-day to all-time session window; full search; soft-delete with undo; OSC 52 mobile clipboard; full help and `?` modal.

---

## What's Next

### Phase 5: Queued

Items considered but deferred for after first-real-use period. Each is independent; pick whichever feels most valuable next.

```
┌─────────────────────────────────────────────────────────────────────────────┐
│  PHASE 5 — QUEUED                                                           │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  1. Spawn-in-new-tab via OSC 7 / shell integration                          │
│     Currently `o`/`d` exit, you paste in another tab. Some terminals       │
│     (Ghostty, kitty, iTerm) support requesting a new tab via escape         │
│     codes; could detect + use. Termius/Warp don't support this — fall      │
│     back to current behavior. Saves the paste step on supported terms.     │
│                                                                             │
│  2. Global status bar                                                       │
│     A one-row strip with `12 dirty · 3 running · 47 sess this week`.        │
│     Always visible regardless of view. Could live above the footer or      │
│     replace part of the pane title. Need to decide where.                  │
│                                                                             │
│  3. Favorites                                                               │
│     `f` toggles a star on the selected worktree or group; starred items    │
│     bubble to the top. Persisted to `~/.config/wt/state.json` (already     │
│     there for filter/expansion).                                            │
│                                                                             │
│  4. Regex / fuzzy search                                                    │
│     `/` is substring today. Add `r:` prefix for regex or `~:` for fuzzy.  │
│     Useful with the new sessions view where you might want                  │
│     `r:guide-lp-v[0-9]` to match version variants.                          │
│                                                                             │
│  5. Background-job log tail in detail modal                                 │
│     Enter on a bg job currently shows fields. Could also include the       │
│     last N lines of timeline.jsonl below the fields. The Sessions-view    │
│     bottom pane already shows tail; this would mirror that into the       │
│     full-screen modal.                                                      │
│                                                                             │
│  6. Configuration file                                                      │
│     `~/.config/wt/config.toml` for: scan roots, default window, default   │
│     view, theme, custom key bindings. Currently roots are env-var          │
│     override only.                                                          │
│                                                                             │
│  7. Trash management surface                                                │
│     `u` undo only works in-session. After quitting, trash persists but    │
│     there's no UI for it. A `T` key could open a trash-browser modal       │
│     showing soft-deleted items with timestamps, with restore + permanent-  │
│     delete actions.                                                         │
│                                                                             │
│  8. Search saved as named filters                                           │
│     `:save name` while a search is active saves it. `F1`-`F9` (or          │
│     numerical hotkeys) recall named filters. Persisted to config.         │
│                                                                             │
│  Estimated effort: each is 1-3 hours of focused work. None block any       │
│  other. Sequence based on which lands first in real use feedback.          │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### Phase 6: Bigger features (probably defer)

```
┌─────────────────────────────────────────────────────────────────────────────┐
│  PHASE 6 — MAYBE LATER                                                      │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  • Multi-host. Show worktrees + sessions from a remote machine via SSH.    │
│    Useful if repos live on multiple machines (Jane has muthur + laptops).  │
│    Needs a transport design (ssh stdio? rest? mosh-style?).                │
│                                                                             │
│  • Git write operations. Ctrl-f fetch, Ctrl-p pull, push, etc. Crosses    │
│    the read-only safety threshold; needs confirmation + dry-run + undo    │
│    where possible. Bigger design pass.                                    │
│                                                                             │
│  • Daemon mode that watches ~/.claude/jobs and pushes notifications       │
│    when a bg job completes. Mostly redundant if wt is running, but        │
│    useful for off-tab notifications.                                       │
│                                                                             │
│  • Web UI. Same data, different front-end. TUI is the form factor for    │
│    now; revisit if a use case appears.                                    │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### Out of scope (not on any phase)

- **Plugin marketplace.** Wrong shape for a personal tool.
- **Multi-user / hosted.** Single-user single-host by design.
- **Mobile native.** Termius/Blink already work over SSH.

---

## Working notes

- Spec: `docs/specs/2026-05-16-wt-dashboard-design.md`
- Plan: `docs/plans/2026-05-16-wt-dashboard-impl.md`
- Format findings: `docs/notes/format-findings.md`
- Persistent UI state: `~/.config/wt/state.json`
- Soft-delete trash: `~/.config/wt/trash/<id>.<unix-ts>/`
- All Phase 1-4 changes shipped on branch `worktree-spec-dashboard-design`; ready to merge to `main` when convenient.
