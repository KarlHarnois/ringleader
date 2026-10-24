# Write lock

**Every rule in this file binds each change to one of these:**

- what a Ringleader process saves to its state directory or its config
- when a process takes or releases a lock

**The write lock is the lock on `<state>/lock`.**

- Take the write lock for one change at a time. Release the write lock when the change ends. A process that keeps the write lock between changes blocks every CLI command until that process exits.
- After taking the write lock, reread the registry before the change. A save rewrites the whole registry, so a stale copy erases another process's change.
- Before each TUI change, reread the config too: the config can change while the TUI runs.
- Never save an observation listed before the process took the write lock. Such a listing can predate another process's rename or delete, and saving the listing undoes that change.
- If another process holds the write lock, a TUI observation saves nothing. If another process holds the write lock, a TUI observation NEVER waits.
- A TUI save outside the write lock applies only the TUI's change to a fresh read of the file. Several TUIs can run at once, so a save of the TUI's whole copy erases another TUI's change.
