# direnv launches

**Every rule in this file binds each change to one of these:**

- how create or restart launches an agent
- which variables the agent gets

A direnv launch starts the agent as `direnv exec <worktree> <agent command> <agent args>`.

- **Never allow a denied `.envrc`:** `direnv deny <worktree>` is how a user keeps one session's agent off that `.envrc`.
- **Never run `direnv allow` on an `.envrc` that differs by any byte from the repo path's `.envrc`.** A branch can change its `.envrc`, and allowing the changed file runs code the user has not reviewed.
- Run Ringleader's own `direnv status` and `direnv allow` calls with only the variables every agent starts with. An `.envrc` the user allowed in their own shell then counts as allowed.
- After the `.envrc` loads, set the agent's `SHELL` back to the `SHELL` Ringleader started with. A dev shell exports its own non-interactive bash, which would replace the user's shell.
- The handoff cleanup keeps the handoff file that the launcher writes for `direnv exec` while the launcher's process still runs. `direnv exec` runs in that process through the `.envrc` load, which can outlast the cleanup's age limit.
