# Ringleader

```text
    ____  _____   __________    _________    ____  __________
   / __ \/  _/ | / / ____/ /   / ____/   |  / __ \/ ____/ __ \
  / /_/ // //  |/ / / __/ /   / __/ / /| | / / / / __/ / /_/ /
 / _, _// // /|  / /_/ / /___/ /___/ ___ |/ /_/ / /___/ _, _/
/_/ |_/___/_/ |_/\____/_____/_____/_/  |_/_____/_____/_/ |_|
```

A tmux session manager for Claude and opencode. Each session gets its own git worktree, branch, and tmux session. A terminal UI lists the sessions with their state, their diff, and a live preview of the agent.

Linux only. Needs tmux 3.2 or newer and git 2.42 or newer.

## Install

```sh
nix run git+https://github.com/KarlHarnois/ringleader
nix profile add git+https://github.com/KarlHarnois/ringleader
```

As a flake input:

```nix
inputs.ringleader.url = "git+https://github.com/KarlHarnois/ringleader";
# then use inputs.ringleader.packages.${system}.default
```

While the repository is private, `git+https` authenticates through your git credential helper, and `git+ssh://git@github.com/KarlHarnois/ringleader` works with an SSH key registered on GitHub. The `github:KarlHarnois/ringleader` form needs a GitHub token in the `access-tokens` setting of `nix.conf` and otherwise fails with `HTTP error 404`. Nix older than 2.30 spells `nix profile add` as `nix profile install`.

## First run

Run `ringleader` with no arguments in a directory that holds your repos. It opens the TUI and writes a commented config to `~/.config/ringleader/config.toml`, with Claude Code and opencode as agents. Press `n` to create a session, pick a repo and an agent, and optionally give an initial prompt.

Sessions run on Ringleader's own tmux server, so they do not show in your own `tmux ls`. The return key, Ctrl-Q unless `return_key` in the config names another, detaches from any session back to the TUI. A changed `return_key` takes effect after `r` in the TUI, or at the next `ringleader new` or `restart`. Sessions have no prefix key, the mouse is on, and each keeps 25,000 lines of scrollback. Inside your own tmux a session opens nested, so your tmux must not use the return key as its prefix or bind it in its root table. Open extra shells with `t` instead of splitting the agent window.

To inspect the sessions with tmux itself, run `tmux -S "$XDG_RUNTIME_DIR/ringleader/tmux/tmux.sock" ls`. Without `XDG_RUNTIME_DIR`, the socket is under `~/.local/state/ringleader/tmux/`. The help screen (`?`) shows the path.

Ringleader owns the configuration of its tmux server, as it owns the rest of its interface. Your own `~/.tmux.conf` never loads there, and Ringleader reads no other tmux file.

## Agent environment

An agent starts with a fixed baseline of variables such as `PATH` and `HOME`. Its `env` list copies more variables from the environment Ringleader runs in, and its `set_env` table sets fixed values. A `set_env` value is used as written, except that a leading `~/` expands to your home. Variables such as `$HOME` are not expanded. The values sit in plain text in the config, so pass secrets through `env` instead. To run Claude Code under two accounts, add one agent per account and pick it when you create a session:

```toml
[[agents]]
name = "claude-work"
command = "claude"
set_env = { CLAUDE_CONFIG_DIR = "~/.claude-work" }
prompt_args = ["{prompt}"]
```

## Keys

| Key | Action |
|---|---|
| `n` | create a session |
| `d` | delete the selected session |
| `R` | rename the selected session |
| `Enter` | attach to the selected session |
| `t` | open a shell in the session's worktree |
| `s` | restart an exited or stopped session |
| `r` | reload the config and the registry |
| `↑` `↓` `j` `k` `h` `l` | move the selection |
| `?` | help |
| `q` | quit, the sessions keep running |

## CLI

```
ringleader                                   open the TUI
ringleader list [--json]
ringleader new --name <n> --repo <r> [--agent <a>] [--base-branch <b>] [--prompt <text>]
ringleader delete <name> [--force] [--forget]
ringleader attach <name>
ringleader rename <old> <new> [--keep-branch]
ringleader restart <name>
ringleader repo add <path> [--name <n>] [--base-branch <b>]
ringleader repo remove <name>
ringleader repo list
ringleader doctor [--json]
```

Every command accepts `--config <path>` and `--log-level <level>`. The log is at `~/.local/state/ringleader/app.log`.

## Exit codes

| Code | Meaning |
|---|---|
| 0 | success |
| 1 | invalid input or a failure |
| 2 | no session with that name |
| 3 | conflict: a refused guard, an unmanaged session, a session that is live and healthy, or a held lock |

## Git credentials

Git can never prompt in the terminal during a Ringleader operation. Credentials that git needs during checkout must come from a credential helper, an askpass program, or `ssh-agent`.

## License

MIT
