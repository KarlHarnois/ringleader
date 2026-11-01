# Scratch directories

**Every rule in this file binds each change to a session's scratch directory.** Unless the agent's `set_env` sets `TMPDIR`, a session's scratch directory is its agent's `TMPDIR`.

- Keep the scratch directory's path short: tools put unix sockets under `TMPDIR`. A socket path cannot pass 107 bytes on Linux or 103 bytes on macOS.
- The scratch root is never `/tmp` or Ringleader's own `TMPDIR`, for these reasons:
  - systemd ages `/tmp` under an idle agent.
  - `/tmp` can be `noexec` or small.
  - `TMPDIR` differs between the process that creates a session and the process that deletes it. Every agent's `TMPDIR` is its own scratch directory, and `nix develop` exports a new `TMPDIR` per shell.
- Delete keeps a scratch directory that holds a mount point: removing it would delete the files of the mounted filesystem.
- If the mount table cannot be read, delete keeps every scratch directory.

## Orphaned scratch directories

**An orphaned scratch directory is a scratch directory that no registry entry and no live tmux session claims.**

- Retire only a directory found orphaned under the write lock, from the registry reread under that lock. A listing taken before the lock can predate another process's create.
- Purge retired directories outside the write lock. Removing several gigabytes under the write lock blocks every CLI command.

## Tool temp directories

**A tool temp directory is a folder that a tool leaves directly inside a scratch directory.** An owned tool temp directory names its owner, the process that made it. An ownerless tool temp directory names no process. A process uses every path that holds its working directory or one of its temp directory variables.

- List the tool temp directories before reading the process table: a folder made after the read has no user yet.
- Keep a tool temp directory modified in the last ten minutes. A launch through direnv makes its folder before any process uses it.
- If an owner uses a path in its scratch directory outside every ownerless tool temp directory, keep all of them. An idle interactive shell hides its own folder. It can also inherit a path into another folder.
