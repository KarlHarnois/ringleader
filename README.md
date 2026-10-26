# Ringleader

[![CI](https://github.com/KarlHarnois/ringleader/actions/workflows/ci.yml/badge.svg)](https://github.com/KarlHarnois/ringleader/actions/workflows/ci.yml)
[![Version](https://img.shields.io/github/v/tag/KarlHarnois/ringleader?label=version&color=orange)](https://github.com/KarlHarnois/ringleader/tags)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue)](LICENSE)
[![Nix flake](https://img.shields.io/badge/nix-flake-5277C3?logo=nixos&logoColor=white)](#install)
[![Homebrew](https://img.shields.io/badge/homebrew-tap-FBB040?logo=homebrew&logoColor=white)](#install)

```text
    ____  _____   __________    _________    ____  __________
   / __ \/  _/ | / / ____/ /   / ____/   |  / __ \/ ____/ __ \
  / /_/ // //  |/ / / __/ /   / __/ / /| | / / / / __/ / /_/ /
 / _, _// // /|  / /_/ / /___/ /___/ ___ |/ /_/ / /___/ _, _/
/_/ |_/___/_/ |_/\____/_____/_____/_/  |_/_____/_____/_/ |_|
```

<img width="2254" height="1456" alt="image" src="https://github.com/user-attachments/assets/f71fa5c3-3187-484d-b474-caef09f5e08f" />

Ringleader is a tmux session manager for Claude Code and opencode. Each session runs an agent in its own git worktree and branch, or in the repo's main checkout. Ringleader's TUI lists the sessions grouped by repo and shows how each one is doing.

Ringleader runs on Linux and Apple Silicon macOS.

## Install

**Install Ringleader with [Homebrew](https://brew.sh) on Apple Silicon macOS:**

```sh
brew install KarlHarnois/tap/ringleader
```

**Install Ringleader with [Nix](https://nixos.org/download/):**

```sh
nix profile add github:KarlHarnois/ringleader
```

**If your Nix is older than 2.30, run `nix profile install` instead of `nix profile add`.**

Both installs bring Ringleader's own tmux and git, and leave yours untouched.

## First run

**Run `ringleader` in a directory that holds your repos.** The command opens the TUI. If no config exists, the command writes a commented config with Claude Code and opencode as agents.

Sessions run on Ringleader's own tmux server. The server ignores your tmux config, and its sessions do not show in your `tmux ls`. Ctrl-Q detaches you from a session and returns you to the TUI or the shell you attached from.

## CLI

```
ringleader
ringleader list [--json]
ringleader new --name <n> --repo <r> [--agent <a>] [--base-branch <b> | --main-checkout] [--plan] [--prompt <text>]
ringleader delete <name>... [--force] [--forget]
ringleader attach <name>
ringleader send <name> <prompt>
ringleader rename <old> <new>
ringleader restart (<name>... | --all)
ringleader stop (<name>... | --repo <r>)
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
| 2 | no session or `[[repos]]` entry with the given name |
| 3 | conflict with existing state |

A command given several names exits with the code of the first name that failed.

Conflicts with existing state include these:

- a taken name or branch
- a running session
- a delete that would lose work
- a held lock
- a `send` to a stopped session, or to an agent away from its prompt

## Skills

Unless an agent's config turns them off, each Claude Code and opencode session gets these skills:

| Skill | What it does |
|---|---|
| `managing-ringleader-sessions` | starts and prompts other sessions, and manages them through the CLI |

Claude Code lists each skill as `/ringleader:<skill>`.
