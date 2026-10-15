# Agent hooks

**Every rule in this file binds each change to one of these:**

- `AgentHooks`
- the exit wait for its typing job
- the check that an agent is at its prompt

**After create and restart, `AgentHooks` schedules the agent's `startup_input` in the background.** The use case returns once the tmux session exists. A typing job is the background work that types one agent's `startup_input`.

- One typing job runs per session at a time. `AgentHooks` skips a second start for the same session.
- The job finds the agent pane once, when the job starts. Before each line, the job checks only the prompt conditions below.
- The job never rechecks the session's name or registry entry. A rename during the job's wait therefore does not reach the job.
- The job waits up to 60 seconds for the first prompt.
- Before each later line, the job pauses briefly so the agent can take the previous line. The job then waits up to 2 seconds for the prompt to come back.
- The job sends each line as its text, then sends Enter 300 milliseconds later. An agent reads text and an Enter that arrive together as a paste. The agent then keeps the Enter in its draft and never submits the line.
- The TUI polls the job while the job runs.
- If the agent is at its prompt right after a rename, `AgentHooks` types the agent's `rename_input`. Otherwise, `AgentHooks` skips the `rename_input`.
- Ringleader logs a skipped or failed typing. The typing never fails the use case.

**On exit, the CLI and the TUI each run these steps.** The CLI runs them after its command, and the TUI runs them after restoring the screen.

1. Release the lock.
2. Note the wait on stderr.
3. Wait for the typing job. If a signal arrives, stop waiting.

**If all three conditions below hold, the agent is at its prompt:**

| Condition | Reason |
| --- | --- |
| The pane is out of copy mode. | Copy mode would take the keys instead of the agent. |
| The pane shows the cursor. | Claude hides the cursor in its trust and permission dialogs. |
| The text before the cursor on its row is a bare prompt marker and a blank. That text holds no letters or digits. | An agent still starting has drawn no marker. An unsent draft puts its text before the cursor. |
