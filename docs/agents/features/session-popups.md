# Session popups

**Every rule in this file binds each change to the session finder or the session menu.** The session finder is the popup that a key in `finder_keys` opens on the private tmux server. The session menu is the popup that a key in `menu_keys` opens on the private tmux server.

## Entry points

**The hidden `__find-session` path runs in the session finder.** It takes no lock. It writes no first-run config.

**The hidden `__session-menu` path runs in the session menu.** It takes no lock. It writes no first-run config.

**The session menu NEVER deletes or creates a session.** It hands each delete and each create to the TUI.

- A delete kills the session's tmux session, which kills the popup.
- A create runs under the write lock.

## Bindings

**Each `finder_keys` and `menu_keys` binding names the paths of Ringleader's executable and its tmux and git programs.** The config hash covers those paths. A config that another build applied therefore reads as stale. Two builds that share one private tmux server rebind each such key in turn.
