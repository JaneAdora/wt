# Format Findings: Claude Jobs, Projects, and Git Porcelain

## Overview

This document records the actual on-disk formats for Claude Code job metadata, interactive session logs, and git porcelain output. These formats are used by the worktree session analyzer TUI tool.

## Job Metadata Format

Location: `~/.claude/jobs/<id>/state.json`

Job state files contain the following fields (enumerated from inspection of 3 recent jobs):

| Field | Type | Notes |
|-------|------|-------|
| `state` | enum | Values: `running`, `idle`, `done`, `error`, `cancelled` |
| `detail` | string | Human-readable status detail |
| `tempo` | enum | Values: `sync`, `background`, `interactive` |
| `output` | string | Last output content |
| `children` | array | IDs of child jobs (nested execution) |
| `linkScanOffset` | number | Byte offset for streaming output |
| `template` | string | Job template name |
| `respawnFlags` | array | Respawn behavior flags |
| `intent` | string | Original user intent / prompt |
| `sessionId` | string | UUID of associated Claude Code session |
| `resumeSessionId` | string | UUID of resumed session (if applicable) |
| `daemonShort` | string | Daemon identifier |
| `cwd` | string | Absolute working directory path |
| `createdAt` | string | ISO 8601 timestamp |
| `updatedAt` | string | ISO 8601 timestamp |
| `firstTerminalAt` | string | ISO 8601 timestamp of first terminal output |
| `originCwd` | string | Original working directory before job execution |
| `backend` | string | Backend identifier (e.g., "haiku", "opus") |
| `inFlight` | boolean | Whether job is currently executing |

Example:
```json
{
  "state": "done",
  "detail": "completed",
  "tempo": "sync",
  "cwd": "/home/jane/projects/thelma",
  "createdAt": "2026-05-15T14:22:33Z",
  "updatedAt": "2026-05-15T14:23:45Z",
  "sessionId": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
  "state": "done"
}
```

## Interactive Session Format

Location: `~/.claude/projects/<encoded-cwd>/<session-uuid>.jsonl`

Encoded CWD format: Replace `/` with `-` in absolute path. Example: `/home/jane/projects/thelma` becomes `-home-jane-projects-thelma`.

Session files are JSONL (JSON Lines) format. Each line is a complete JSON object representing one event.

First line (session start):
```json
{
  "type": "InteractiveSessionStart",
  "permissionMode": "user",
  "sessionId": "a1b2c3d4-e5f6-7890-abcd-ef1234567890"
}
```

Middle lines (events): UserPromptSubmit, ToolCall, ToolResult, etc.

Last line (session end):
```json
{
  "type": "InteractiveSessionEnd",
  "uuid": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
  "timestamp": "2026-05-15T14:23:45Z",
  "userType": "user",
  "entrypoint": "claude-code"
}
```

Session UUIDs appear in filenames as: `<uuid>.jsonl` (full UUID format, no transformation).

## Git Worktree Porcelain Format

Command: `git worktree list --porcelain`

Format (per worktree):
```
worktree /absolute/path/to/worktree
HEAD <commit-sha>
branch refs/heads/branch-name
detached
```

Notes:
- "worktree" line always present
- "HEAD" line always present with full commit SHA
- "branch" line present for branch-tracked worktrees (omitted for detached)
- "detached" line present for detached HEAD worktrees (omitted for branch-tracked)

## Git Status v2 Porcelain Format

Command: `git status --porcelain=v2 --branch`

Header (always first two lines):
```
# branch.oid <commit-sha>
# branch.head <branch-name>
```

File status lines (one per changed file):
```
1 <status-xy> <submodule> <mode-old> <mode-new> <hash-old> <hash-new> <path>
```

Format legend:
- `1`: Format version
- `<status-xy>`: Two-character status code (e.g., "M." = modified in index, unchanged in worktree)
- `<submodule>`: Submodule mode (N = not a submodule)
- `<mode-old>` / `<mode-new>`: File mode as octal (e.g., "100644")
- `<hash-old>` / `<hash-new>`: Object SHA1 hashes
- `<path>`: File path relative to repo root

Untracked files (if requested with --untracked=all):
```
? <path>
```

## Field Constraints and Validation

- **Timestamps**: ISO 8601 format (e.g., "2026-05-15T14:23:45Z")
- **UUIDs**: Standard UUID format (8-4-4-4-12 hex segments)
- **Paths**: Absolute paths in job metadata; relative in status output
- **Commit SHAs**: Full 40-character SHA1 hex strings

## Notes for Implementation

1. Job state files are updated after each job state change; last modification time reflects the most recent update.
2. Session JSONL files are append-only; new events are appended as lines without modifying existing lines.
3. Git porcelain formats are stable across git versions; use `--porcelain` and `--porcelain=v2` flags to ensure consistent parsing.
4. Encoded CWD format is deterministic; test with paths containing spaces, dashes, and other special characters.
