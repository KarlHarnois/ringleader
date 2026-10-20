# tmux commands

**Every rule in this file binds each tmux command that Ringleader runs or binds.**

## Targets

**Every tmux target is exact.** An exact target is one of these:

- `=<session>:` from `TmuxScripts::exact`
- a session id
- a window id
- a pane id

**Never pass a bare `-t <name>`:** tmux resolves a name with no exact match by prefix. A bare `-t fix` would reach `fix-auth`.

- If a session's name starts with `$`, target the session by its id. tmux reads `=$0:` as the session whose id is `$0`.

## Start directories

**Every `new-session`, `new-window`, and `respawn-window` passes `-c` with the worktree.** Without `-c`, tmux starts the pane in the caller's directory and still exits 0.

- A binding in the generated config passes `-c '#{session_path}'`. Ringleader renders the config before any session exists, and `create_session` starts each session in its worktree.
