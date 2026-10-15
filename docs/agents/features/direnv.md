# direnv launches

**Every rule in this file binds each change to one of these:**

- how create or restart launches an agent
- which variables the agent gets

**A direnv launch starts the agent as `direnv exec <worktree> <agent command> <agent args>`.** A plain launch starts the agent without direnv.

Create and restart choose the launch by the first case below that matches. Restart includes the respawn of an Exited session and the recreation of a Stopped one.

1. If the worktree has no `.envrc`, or `direnv` is not on Ringleader's `PATH`, do a plain launch with no warning.
2. If direnv reports the worktree's `.envrc` as allowed, do a direnv launch.
3. If direnv reports the worktree's `.envrc` as denied, do a plain launch with no warning.
4. If both conditions below hold, run `direnv allow <worktree>`, then do a direnv launch.
   - The worktree's `.envrc` is byte-identical to the `.envrc` of the session's repo path.
   - direnv reports the repo path's `.envrc` as allowed.
5. Otherwise, do a plain launch and warn the user. The warning names the worktree and says that its `.envrc` is not allowed. The warning says that the agent started without that `.envrc`. The warning tells the user to review the `.envrc` and run `direnv allow <worktree>`.

- If `direnv` is missing, `DirenvDirectoryEnvironment::when_installed` gives `CompositionRoot` a `NullDirectoryEnvironment`.
- Never allow a denied `.envrc`: `direnv deny <worktree>` is how a user keeps one session's agent off that `.envrc`.
- **Never run `direnv allow` on an `.envrc` that differs by any byte from the repo path's `.envrc`.** A branch can change its `.envrc`, and allowing the changed file runs code the user has not reviewed.
- A repo path with no `.envrc` differs from every worktree `.envrc`.
- If `direnv allow` fails, do a plain launch and warn with direnv's error.
- A warning never fails the create or the restart. `new` and `restart` print the warning on stderr and exit 0. The TUI shows the warning in the status bar.

## direnv variables

| Term | Meaning |
| --- | --- |
| Baseline variables | The variables every agent starts with, including `HOME` and `XDG_*` |
| Inherited variables | The baseline variables, plus each variable in the agent's `env` list that Ringleader's environment sets |
| Explicit variables | The variables the second launcher sets on top of the `.envrc` environment, listed below |

- Ringleader's `direnv status` and `direnv allow` calls run with only the baseline variables. An `.envrc` the user allowed in their own shell therefore counts as allowed.
- The `direnv exec` in the agent's pane runs with the inherited variables. The `.envrc` load therefore sees the agent's `env` values.
- If the agent's `env` list names a set `DIRENV_CONFIG`, the `.envrc` load reads that direnv config. Ringleader's `direnv status` and `direnv allow` calls do not read it.

The explicit variables are:

- each variable in the agent's `env` list that Ringleader's environment sets
- the `SHELL` Ringleader started with
- the agent's `set_env` values
- the `TMPDIR` of the scratch directory

## direnv launch steps

1. The launcher in the agent pane writes a second handoff file.
2. The launcher execs `direnv exec <worktree> ringleader __exec-agent --handoff <second file>` with only the inherited variables.
3. The second launcher runs inside the environment the `.envrc` produced. The second launcher execs the agent with the explicit variables on top.

- Each step execs the next, so the agent stays the pane's own process.
- The explicit variables keep `SHELL`: a dev shell exports its own non-interactive bash, which would replace the user's shell.
- Every other variable, including `PATH`, keeps the value the `.envrc` produced.
- Every handoff write, CLI command, and TUI start deletes the handoff files older than 60 seconds.
- The cleanup keeps a second handoff file while the process that wrote the file still runs. `direnv exec` keeps that process through the `.envrc` load, which can outlast 60 seconds.
- The `t` shell never loads the `.envrc`.
- Rename keeps the running agent and its environment.
