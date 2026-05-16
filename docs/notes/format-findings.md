# Format Findings: Claude Jobs, Projects, Git Porcelain

Date verified: 2026-05-16. Inspected against real files on Muthur.

## Job Metadata (`~/.claude/jobs/<id>/state.json`)

The metadata file is named `state.json`, not `metadata.json`. Real keys observed:

| Field | Type | Notes |
|-------|------|-------|
| `state` | string | Observed values: `done`, `working`, `blocked`. Other values likely exist; parse defensively. |
| `tempo` | string | Observed values: `active`, `idle`. |
| `cwd` | string | Shell cwd at spawn (absolute). May not be the worktree path. |
| `worktreePath` | string \| null | **Canonical worktree path when set.** Prefer this over `cwd` for matching jobs to worktrees. Null for non-worktree jobs. |
| `worktreeBranch` | string \| null | Branch name of the worktree, when applicable. |
| `sessionId` | string | UUID of associated Claude Code session. |
| `resumeSessionId` | string \| null | UUID being resumed, if any. |
| `intent` | string | Original user prompt / task description. Good fallback display value. |
| `name` | string | Job display name. |
| `detail` | string | Human-readable status detail. |
| `createdAt` | string | ISO 8601 with `Z` suffix. |
| `updatedAt` | string | ISO 8601 with `Z` suffix. Use for "age". |
| `firstTerminalAt` | string \| null | ISO 8601, when first output was rendered. |
| `inFlight` | boolean | Whether the job is currently executing. |
| `backend` | string | Model backend (e.g., `haiku`, `sonnet`, `opus`). |
| `template` | string | Job template name. |
| `output` | string | Last emitted output content. |
| `children` | array | IDs of child jobs. |
| `cliVersion` | string | Claude Code CLI version. |
| `daemonShort` | string | Daemon identifier. |
| `linkScanOffset` | number | Byte offset for streaming output. |
| `linkScanPath` | string | Path of file being scanned for links. |
| `nameSource` | string | How `name` was derived. |
| `respawnFlags` | array | Respawn behavior flags. |

**Implications for the dashboard:**

- To match a job to a worktree: prefer `worktreePath`, fall back to `cwd`. Both are absolute paths.
- Status mapping for `JobStatus`: map `working` and `inFlight: true` to `Running`; `done` to `Completed`; `blocked` and `error` (if present) to `Failed`; anything else to `Unknown`. Use `serde(default)` so unknown enum values do not fail parsing.
- Age can be computed from `updatedAt` (preferred) or `firstTerminalAt`. The file's mtime is also a reasonable proxy.

## Interactive Sessions (`~/.claude/projects/<encoded-cwd>/<session-uuid>.jsonl`)

**Encoded-cwd format (irreversible):**

The encoding replaces both `/` and `.` with `-`. Examples:

| Source path | Encoded directory name |
|---|---|
| `/home/jane` | `-home-jane` |
| `/home/jane/projects/thelma` | `-home-jane-projects-thelma` |
| `/home/jane/projects/thelma/.claude/worktrees/thelma-design` | `-home-jane-projects-thelma--claude-worktrees-thelma-design` |

Note the **double dash** (`--`) where `/.` appears in the source: the `/` becomes `-` and the `.` also becomes `-`, producing `--`.

Because both `/` and `.` collapse to `-`, this encoding is **not reversible**: an encoded dir name `-a-b-c` could come from `/a/b/c`, `/a.b/c`, `/a/b.c`, `/a.b.c`, etc. The implementation must only ever go path → encoded → directory lookup, never the reverse.

**Per-session file:** `<session-uuid>.jsonl`. One event per line, append-only. UUIDs are standard 8-4-4-4-12 hex format.

**Line `type` values observed:**
- `user` — a user prompt event; `.message.content` is a string with the prompt text
- `assistant` — assistant response event
- `system` — system message event
- `permission-mode` — permission mode change marker
- `attachment` — file or media attachment event
- `file-history-snapshot` — periodic file-history record
- `ai-title` — auto-generated session title (when present)
- `last-prompt` — marker for the most recent user prompt

**Common keys per line:** `type`, `sessionId`, often `leafUuid`, often `timestamp` (ISO 8601 with `.451Z`-style ms suffix), often `message` (object with `role` + `content`).

**Summary source for the dashboard:**

Prefer in this order:
1. First line with `type == "ai-title"` (if any): use its title field.
2. First line with `type == "user"`: take `.message.content`, truncate to 80 chars.
3. Fall back to `"(no summary)"`.

The first few lines of a jsonl are often non-`user` events (`permission-mode`, `attachment`), so do not assume line 1 is the prompt.

**Session age:** use the file's mtime; the jsonl is append-only so mtime tracks the last event.

## Git Worktree Porcelain (`git worktree list --porcelain`)

Format per entry, blank line between entries:

```
worktree /absolute/path
HEAD 40-char-sha
branch refs/heads/branch-name
```

Variants:
- `bare` line present for bare repositories (replaces `branch`).
- `detached` line present for detached HEAD (replaces `branch`).
- `branch refs/heads/...` present for branch-tracked checkouts.

Real sample (this repo, run from `~/projects/wt/`):

```
worktree /home/jane/projects/wt
HEAD 5ebc9469ae553e73dff54e816eee5c799df55c66
branch refs/heads/main

worktree /home/jane/projects/wt/.claude/worktrees/spec-dashboard-design
HEAD 8ae6851...
branch refs/heads/worktree-spec-dashboard-design
```

The plan's `git::parse_worktree_list` handles this exactly.

## Git Status v2 Porcelain (`git status --porcelain=v2 --branch`)

Header lines (always prefixed with `# `):

```
# branch.oid <40-char-sha>
# branch.head <branch-name>
# branch.upstream <upstream-ref>      (only if upstream is configured)
# branch.ab +<ahead> -<behind>         (only if upstream is configured)
```

Entry lines (one per changed file). The format is documented at `git-status(1)`:

```
1 <XY> <sub> <mH> <mI> <mW> <hH> <hI> <path>        (regular changes)
2 <XY> <sub> <mH> <mI> <mW> <hH> <hI> <Xscore> <path>\t<origPath>  (renamed/copied)
u <XY> <sub> <m1> <m2> <m3> <mW> <h1> <h2> <h3> <path>  (unmerged)
? <path>                                            (untracked, with --untracked=all)
! <path>                                            (ignored, with --ignored)
```

`XY` is two characters: index status / worktree status. Anything starting with `1`, `2`, `u`, `?`, `!` indicates dirtiness for our purposes. The plan's `parse_status_v2` already treats any non-`#`, non-empty line as dirty, which is correct.

When no upstream is set, `# branch.upstream` and `# branch.ab` lines are absent and the parser leaves `upstream = None`, `ahead = 0`, `behind = 0` — that's the desired behavior per the spec's "no-upstream-quiet = inactive" rule.

## Implementation Notes

1. **State enum:** use `serde(default)` and a fallback `Unknown` variant. The observed values (`done`, `working`, `blocked`) do not match the plan's initial `parse_status` matcher (`running`, `completed`, `failed`). Update the matcher when implementing Task 7:
   - `working` or `inFlight: true` → `Running`
   - `done` → `Completed`
   - `blocked`, `error`, `cancelled` (if present) → `Failed`
   - anything else → `Unknown`
2. **Metadata filename:** use `state.json`, not `metadata.json`. Update Task 7's filename candidates to lead with `state.json`.
3. **Worktree match:** Task 7 should match jobs to worktrees by `worktreePath` first, then `cwd`. The plan currently only uses `cwd`. This is a small enrichment to add at Task 7 implementation time.
4. **Encoded-cwd:** the encoding collapses `/` AND `.` to `-`. The `encode_cwd` function in the plan needs to apply both substitutions. The current plan's `encode_cwd` only replaces `/`; that must be updated.
