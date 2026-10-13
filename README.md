# Ringleader

[![CI](https://github.com/KarlHarnois/ringleader/actions/workflows/ci.yml/badge.svg)](https://github.com/KarlHarnois/ringleader/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue)](LICENSE)
[![Nix flake](https://img.shields.io/badge/nix-flake-5277C3?logo=nixos&logoColor=white)](#install)

```text
    ____  _____   __________    _________    ____  __________
   / __ \/  _/ | / / ____/ /   / ____/   |  / __ \/ ____/ __ \
  / /_/ // //  |/ / / __/ /   / __/ / /| | / / / / __/ / /_/ /
 / _, _// // /|  / /_/ / /___/ /___/ ___ |/ /_/ / /___/ _, _/
/_/ |_/___/_/ |_/\____/_____/_____/_/  |_/_____/_____/_/ |_|
```

A tmux session manager for Claude and opencode. Each session gets its own git worktree, branch, and tmux session. A terminal UI lists the sessions with their state, their diff, and a live preview of the agent.

Linux only.

## Install

```sh
nix run github:KarlHarnois/ringleader
nix profile add github:KarlHarnois/ringleader
```

As a flake input:

```nix
inputs.ringleader.url = "github:KarlHarnois/ringleader";
# then use inputs.ringleader.packages.${system}.default
```

Nix older than 2.30 spells `nix profile add` as `nix profile install`.

## First run

Run `ringleader` in a directory that holds your repos. It opens the TUI and writes a commented config to `~/.config/ringleader/config.toml`, with Claude Code and opencode as agents. Its comments show how to pass variables to an agent, for example to run Claude Code under a second account.

Sessions run on Ringleader's own tmux server. It ignores your `~/.tmux.conf`, and its sessions do not show in your `tmux ls`. Open extra shells with `t` instead of splitting the agent window, because output in a split pane counts as agent activity.

## Keys

| Key | Action |
|---|---|
| `n` | create a session |
| `d` | delete the selected session |
| `R` | rename the selected session |
| `Enter` | attach to the selected session |
| `Ctrl-Q` | detach from a session back to the TUI |
| `t` | open a shell in the selected session's worktree |
| `s` | restart an exited or stopped session |
| `r` | reload the config and the registry and refresh diffs |
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

Every command accepts `--config <path>` and `--log-level <level>`.

## Exit codes

| Code | Meaning |
|---|---|
| 0 | success |
| 1 | invalid input or a failure |
| 2 | no session with that name |
| 3 | conflict with existing state, such as a taken name or branch, a running session, a refused delete, or a held lock |
