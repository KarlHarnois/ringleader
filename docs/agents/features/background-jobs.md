# Background jobs

**Every rule in this file binds each change to the TUI's observations, preview captures, diff measurements, and pull request lookups.**

**No background job blocks input.**

A background job is a `Pending` worker that runs the tmux, git, or gh calls of one TUI task. The task is one of these:

- an observation
- a preview capture
- a diff measurement
- a pull request lookup

An observation is one tmux listing of every session. The TUI reconciles each observation with the registry. A background job is in flight from its start until the event loop collects it.

- The event loop collects finished observations, preview captures, and diff measurements on every wake.
- The event loop starts and collects pull request lookups only on its 100 millisecond tick.
- While an observation, a preview capture, or a diff measurement is in flight, the event loop wakes every 16 milliseconds.
- A pull request lookup never shortens the event loop's wait: one lookup can take 20 seconds per repo.
- The TUI shows a preview capture only under the row the capture was taken for.

## Synchronous observations

**A synchronous observation is an observation the TUI runs on its own thread instead of as a background job.**

- The TUI runs a synchronous observation before its first frame. Without that observation, the first frame and the keys typed at launch see no sessions.
- The TUI runs a synchronous observation after every attach, shell, reload, and blocking operation.
- A synchronous observation drops any observation in flight: an observation listed before a delete would bring the deleted session back.
- A synchronous observation restarts the `state_poll_ms` interval.

## Diff measurements

- Diff measurements read git through their own `GitWorkspace`. `CompositionRoot` hands that `GitWorkspace` to `RepoPorts::with_diff_workspace`.
- Every read of the diff `GitWorkspace` has the 5 second diff deadline.
- A slow repo therefore keeps a measurement in flight for at most 5 seconds per git call. The event loop stays on its 16 millisecond wakes for that time.
- Create, pull, and the other use cases read under the 60 second deadline.
