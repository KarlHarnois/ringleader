# Scratch directories

**Every rule in this file binds each change to a session's scratch directory.** Unless the agent's `set_env` sets `TMPDIR`, a session's scratch directory is its agent's `TMPDIR`.

- Keep the scratch directory's path short: tools put unix sockets under `TMPDIR`. A socket path cannot pass 107 bytes on Linux or 103 bytes on macOS.
- The scratch root is never `/tmp` or Ringleader's own `TMPDIR`, for these reasons:
  - systemd ages `/tmp` under an idle agent.
  - `/tmp` can be `noexec` or small.
  - `TMPDIR` differs between the process that creates a session and the process that deletes it. Every agent's `TMPDIR` is its own scratch directory, and `nix develop` exports a new `TMPDIR` per shell.
- Delete keeps a scratch directory that holds a mount point: removing it would delete the files of the mounted filesystem.
- If the mount table cannot be read, delete keeps every scratch directory.
- Forget keeps the worktree and the branch. Its scratch directory becomes orphaned, so the same delete retires it.

## Orphaned scratch directories

**An orphaned scratch directory is a scratch directory that no registry entry and no live tmux session claims.** A stopped session keeps its registry entry, so its scratch directory and every file in it stay.

- Decide which scratch directories are orphaned only under the write lock, from the registry reread under that lock. A listing taken before the lock can predate another process's create.
- Keep an orphaned scratch directory that a live process runs in.
- Retire an orphaned scratch directory under the write lock, and purge retired directories outside the write lock. Removing several gigabytes under the write lock blocks every CLI command.
- A purge keeps a retired directory that holds a mount point.
- If the mount table cannot be read, a purge keeps every retired directory.
