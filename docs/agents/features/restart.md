# Restart

**Every rule in this file binds each change to restart and restart all.**

**Restart revives an Exited or Stopped session in its worktree.**

| State | Restart | The agent starts with |
| --- | --- | --- |
| Exited | Respawns the agent in the session's tmux session | The agent's `args` |
| Stopped | Recreates the session's tmux session | The agent's `args`, then its `resume_args` |

- A Stopped session lost its tmux server, such as to a reboot. `resume_args` resumes the conversation that the lost server cut off.
- Never add `resume_args` to a respawn: a respawn is the fallback for a resume that found no conversation and exited.
- Create never adds `resume_args`.

**Restart all is a restart of every Exited or Stopped session, one at a time, in name order.** `ringleader restart --all` and the `S` key start a restart all.

- Never let one reconciliation serve several restarts in a restart all. Each recreation saves the registry from its reconciliation, so a shared reconciliation drops the sockets of earlier recreations.
- A failed restart never stops the rest. Restart all reports each failure.
- `restart --all` exits with the code of its first failed restart. If no restart failed, it exits 0.
- If a signal arrives during a restart all, the current restart finishes and Ringleader skips the rest.
