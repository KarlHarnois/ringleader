# Scratch directories

**Every rule in this file binds each change to a session's scratch directory.** Unless the agent's `set_env` sets `TMPDIR`, a session's scratch directory is its agent's `TMPDIR`.

- Keep the scratch directory's path short: tools put unix sockets under `TMPDIR`, and a socket path cannot pass 107 bytes.
- The scratch root is never `/tmp` or Ringleader's own `TMPDIR`, for these reasons:
  - systemd ages `/tmp` under an idle agent.
  - `/tmp` can be `noexec` or small.
  - `TMPDIR` differs between the process that creates a session and the process that deletes it. Every agent's `TMPDIR` is its own scratch directory, and `nix develop` exports a new `TMPDIR` per shell.
- Delete keeps a scratch directory that holds a mount point: removing it would delete the files of the mounted filesystem.
- If the mount table cannot be read, delete keeps every scratch directory.
