# Scratch directories

**Every rule in this file binds each change to scratch directories.**

**A session's scratch directory is `$XDG_CACHE_HOME/ringleader/tmp/<hash>`.** The hash is the FNV-1a hash of the recorded worktree path, folded to 8 hex digits.

- Keep the scratch directory's path short: tools put unix sockets under `TMPDIR`, and a socket path cannot pass 107 bytes.
- The scratch root is never `/tmp` or Ringleader's own `TMPDIR`, for these reasons:
  - systemd ages `/tmp` under an idle agent.
  - `/tmp` can be `noexec` or small.
  - `TMPDIR` differs between the process that creates a session and the process that deletes it. Every agent's `TMPDIR` is its own scratch directory, and `nix develop` exports a new `TMPDIR` per shell.
- Create and restart make the scratch directory. Unless the agent's `set_env` sets `TMPDIR`, they start the agent with `TMPDIR` set to the scratch directory.
- If making the scratch directory fails, the agent starts with the inherited `TMPDIR`.
- Delete removes the scratch directory after the worktree. Forget keeps the scratch directory.
- Delete keeps a scratch directory that holds a mount point: removing it would delete the files of the mounted filesystem.
- If the mount table cannot be read, delete keeps every scratch directory.
- Ringleader logs a failure to make or remove a scratch directory. The failure never fails the use case.
