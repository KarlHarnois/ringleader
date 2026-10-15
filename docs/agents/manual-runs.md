# Manual runs

**This file is the procedure for a manual run.**

**Every manual run uses throwaway state and fixture repos.** `main` resolves the real XDG paths, which hold the private tmux server's socket.

**The run prefix is the command prefix below, followed by the hermetic git environment.** The prefix's `HOME=<tempdir>/home` is the hermetic git environment's `HOME`.

```sh
env -u TMUX -u TMUX_PANE -u RINGLEADER_CONFIG \
  XDG_CONFIG_HOME=<tempdir>/config XDG_STATE_HOME=<tempdir>/state \
  XDG_DATA_HOME=<tempdir>/data XDG_CACHE_HOME=<tempdir>/cache \
  XDG_RUNTIME_DIR=<tempdir>/run HOME=<tempdir>/home SHELL=sh
```

- Each throwaway XDG directory keeps the run off the project owner's real one, including the real private tmux server.
- `HOME`, `SHELL`, and the hermetic git environment keep the project owner's shell and git startup files out of the run.

To make a manual run:

1. Make `<tempdir>` with `mktemp -d /tmp/rl.XXXXXX`.
2. In `<tempdir>`, run `mkdir -p config/ringleader state data cache run repos home`.
3. Write `<tempdir>/config/ringleader/config.toml` with one stand-in agent: `name = "stand-in"` and `command = "sh"`. Never configure `claude` or `opencode` as the agent.
4. Build fixture repos under `<tempdir>/repos` with the hermetic git environment.
5. Start an outer tmux server under the ad-hoc tmux command rules. The outer server stands in for the user's tmux server. Drive the outer server with `send-keys`.
6. Run each check in the table below in a pane of the outer server. Run the binary by absolute path from `<tempdir>/repos`, so no real repo is in scope.
7. After each check, assert that the outer server's `list-keys` output and session list are unchanged.
8. Run `kill-server` on the outer server. Then run `kill-server` with `-S <tempdir>/run/ringleader/tmux/tmux.sock`.
9. Remove `<tempdir>`.

| Check | How to run the binary |
| --- | --- |
| Plain | In a plain pane, with the run prefix |
| Nested | In the same pane, with the run prefix minus `-u TMUX -u TMUX_PANE`, so the client opens nested |
| Shell | In a `t` shell of a session. The binary MUST refuse with exit code 3. |

**If a check fails, run steps 8 and 9, then report the failure to the project owner.** A failed check is one of these:

- a check whose step 7 assertion fails
- a Shell check whose binary does not refuse with exit code 3

Send every tmux command for a manual run's private tmux server with the run prefix and `-S <tempdir>/run/ringleader/tmux/tmux.sock`.
