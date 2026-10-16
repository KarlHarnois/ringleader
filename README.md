# Ringleader

[![CI](https://github.com/KarlHarnois/ringleader/actions/workflows/ci.yml/badge.svg)](https://github.com/KarlHarnois/ringleader/actions/workflows/ci.yml)
[![Version](https://img.shields.io/badge/version-0.1.0-orange)](Cargo.toml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue)](LICENSE)
[![Nix flake](https://img.shields.io/badge/nix-flake-5277C3?logo=nixos&logoColor=white)](#install)

```text
    ____  _____   __________    _________    ____  __________
   / __ \/  _/ | / / ____/ /   / ____/   |  / __ \/ ____/ __ \
  / /_/ // //  |/ / / __/ /   / __/ / /| | / / / / __/ / /_/ /
 / _, _// // /|  / /_/ / /___/ /___/ ___ |/ /_/ / /___/ _, _/
/_/ |_/___/_/ |_/\____/_____/_____/_/  |_/_____/_____/_/ |_|
```

Ringleader is a tmux session manager for Claude Code and opencode. Each session runs an agent in its own git worktree and branch. Ringleader lists the sessions grouped by repo, with each session's state, such as working or waiting for input.

Ringleader runs only on Linux.

## Install

**Install Ringleader with [Nix](https://nixos.org/download):**

```sh
nix profile add github:KarlHarnois/ringleader
```

**If your Nix is older than 2.30, run `nix profile install` instead of `nix profile add`.**

## First run

**Run `ringleader` in a directory that holds your repos.** When you create a session, Ringleader offers the repos it finds under that directory.

**If a session fails to start, run `ringleader doctor`.** `ringleader doctor` checks tmux, git, the config, and every session.

The first run writes a commented config to `~/.config/ringleader/config.toml`, with Claude Code and opencode as agents. The config's comments show how to set an agent's environment variables. For example, an `[[agents]]` entry that sets `CLAUDE_CONFIG_DIR` can run Claude Code under a second account.

Sessions run on Ringleader's own tmux server. That server ignores your tmux config. `tmux ls` does not list Ringleader's sessions.

**Press `Ctrl-Q` to leave a session and return to Ringleader.** Press `?` in Ringleader to list its keys.

## CLI

```
ringleader
ringleader list [--json]
ringleader new --name <n> --repo <r> [--agent <a>] [--base-branch <b>] [--prompt <text>]
ringleader delete <name> [--force] [--forget]
ringleader attach <name>
ringleader rename <old> <new>
ringleader restart (<name> | --all)
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
| 1 | invalid input or any other failure |
| 2 | no session or `[[repos]]` entry with that name |
| 3 | conflict with existing state |

Examples of a conflict with existing state:

- a taken name or branch
- a restart of a session that is still running
- a delete that would lose work
- a lock that another Ringleader process holds
