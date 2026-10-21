# Background jobs

**Every rule in this file binds each change to the TUI's background jobs.** A background job is a `Pending` worker that runs the outside calls of one TUI task. An observation is one tmux listing of every session.

**No background job blocks input.**

- A pull request lookup never shortens the event loop's wait: one lookup can take 20 seconds per repo.
- A usage measurement never shortens the event loop's wait: one probe can take 30 seconds.
- The TUI observes on its own thread before its first frame. Without that observation, the first frame and the keys typed at launch see no sessions.
- An observation on the TUI's own thread drops any observation in flight. An observation listed before a delete would otherwise bring the deleted session back.
- A background observation never saves: the TUI lists it before taking the write lock.
