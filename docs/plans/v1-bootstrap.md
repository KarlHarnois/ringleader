# Plan: Ringleader, a tmux session manager for Claude and opencode

> Status: active. Flip to `completed` when v1 ships. This file is the historical plan, not living documentation; `AGENTS.md` is the living documentation.

## 0. Ground rules for the implementing agent

- Load the clean-code skill before writing any code.
- Comments are treated as failures to express intent in code. Every comment is a signal, not documentation: revisit the code it explains and improve it until the comment is unnecessary, then delete the comment. Rename the variable, extract the function, introduce the type, or restructure the control flow that made the explanation feel needed. Never add a comment to justify messy code, and never leave a comment as a substitute for a better name. The only surviving comments are the clean-code exceptions: legal headers, non-obvious intent that cannot be encoded in structure, warnings of real consequences, and TODOs that reference a tracked issue. A surviving comment is never announced in a commit body. Instead, the implementing agent stops and justifies the comment to the project owner before committing. If the justification against an exception is not convincing, delete the comment and fix the code instead. This rule outranks convenience; a commit that adds an unjustified comment is not done.
- Write object-oriented Rust: state in structs with private fields, behavior in `impl` blocks, traits as interfaces, composition over inheritance, dependencies injected through constructors. No free functions except `main` and `#[test]` functions, which Rust requires to be free.
- One primary type per file, file named after it in snake case. The tree in section 3 is the file contract: every listed file exists and holds its one type. Types absent from the tree are support types (error enums, builders, request structs, iterators, parser results, dialog state) and fold into their owner's file. Unit tests live inline in `#[cfg(test)] mod tests`. `mod.rs` declares and re-exports only. Source files target under 200 lines excluding tests.
- Commit after every milestone item. Capital first letter, title under 72 chars, imperative body.
- Build, test, and run every tool inside `nix develop`. The host PATH may carry other versions of `cargo`, `rustc`, and `tmux`, including a tmux newer than the pinned one, so never rely on what the host provides.
- Every ad-hoc tmux command, such as a milestone 6 check or a binding experiment, runs as `env -u TMUX -u TMUX_PANE tmux -u -S <tempdir>/x.sock -f /dev/null ...` and ends with `kill-server` on that socket. `nix develop` passes `$TMUX` through, so a bare `tmux` command reaches the user's live server. Never touch the default server or the server named in `$TMUX`. Keep the socket path under 100 bytes, because tmux rejects a longer one with `File name too long`. Use a directory from `mktemp -d` under `/tmp`, or `cd` into the scratch directory and pass a relative path such as `-S ./x.sock`.
- The private GitHub repository exists and is configured as `origin`. Do not create another repository or add another remote. Push only when the project owner asks.
- Keep the repo consumer-agnostic. Never reference or assume any specific downstream setup, local directory layout, or personal configuration. Examples and fixtures use generic names such as `example-project`.
- Human-facing prose uses "Ringleader". Identifiers stay lowercase: binary, crate, repo, flake input, config paths, tmux and branch prefixes, JSON fields.
- This file is the source of truth. If something here conflicts with an instruction received later, stop and ask.

## 1. Naming

| Item | Value |
|---|---|
| Repo / directory | the repository root |
| Remote | `github:KarlHarnois/ringleader`, private, configured as `origin` |
| Binary | `ringleader` |
| Config | `$XDG_CONFIG_HOME/ringleader/config.toml`, override `$RINGLEADER_CONFIG` |
| State | `$XDG_STATE_HOME/ringleader/` for `sessions.json`, `bindings.json`, `app.log`, `lock` |
| Data | `$XDG_DATA_HOME/ringleader/worktrees` |
| Launch handoff | `$XDG_RUNTIME_DIR/ringleader/handoff/`, falling back to `$XDG_STATE_HOME/ringleader/handoff/` |
| tmux session prefix | `ringleader-` |
| Branch prefix | `ringleader/` |
| Nix flake input | `ringleader` |
| License | MIT |

Examples: branch `ringleader/fix-auth`, tmux session `ringleader-fix-auth`, worktree `~/.local/share/ringleader/worktrees/example-project/fix-auth`.

## 2. Locked decisions

- Rust, single binary crate, synchronous event loop with polled input, no async runtime.
- `ratatui` + `crossterm`.
- Hexagonal architecture: `ui` and `adapters` depend on `application`, `application` depends on `domain`, `domain` depends on nothing. `main.rs` is the only composition root.
- Object-oriented design throughout, in Rust terms: state lives in structs with private fields, behavior in `impl` blocks, traits define the ports (interfaces), composition over inheritance. Constructors (`new`) and dependency injection through function arguments. No free functions except `main` and `#[test]` functions. No `pub` data fields, access is through methods. This applies to domain, application, adapters, and UI equally.
- One primary type per file, named after it in snake case. The tree in section 3 is the file contract, and every listed file holds its type. Types not in the tree are support types that fold into their owner's file. Unit tests live inline in `#[cfg(test)] mod tests`. `mod.rs` files declare and re-export only. Source files target under 200 lines excluding tests; when one outgrows that, extract its largest behavior-bearing type into its own file and update section 3.
- Repo discovery is contextual. The picker scans from the directory the app was launched from. Inside a git repo, the scope is that repo. When the launch directory is below that repo's top level, a bounded walk of the launch directory also runs, and its repos merge with the enclosing one. Outside a repo, the scope is a bounded walk of the launch directory. Explicit `[[repos]]` entries remain as overrides for custom names, custom `base_branch`, and repos that must always be visible. Explicit entries win on path collision.
- The session list is never filtered by scan scope. Existing sessions always show regardless of cwd.
- The scan walk is bounded by `scan_depth` (default 3), pruning `node_modules`, `target`, `vendor`, `.cache`, and `dist`. A `.git` directory marks a repo and stops descent; a `.git` file means a worktree or submodule and is skipped. Symlinks are not followed. Permission errors are ignored. Worktrees and submodules never appear as repos.
- Delete removes the worktree and deletes the branch. It warns and double-confirms when the worktree is dirty or the branch has commits that exist on no other ref, and it requires explicit confirmation while the agent is `Working`.
- Worktree removal uses `git worktree remove --force` only. There is no recursive-delete fallback. When git refuses, Ringleader reports git's stderr and removes nothing.
- Rename renames the tmux session, the registry entry, and the git branch by default. `--keep-branch` and a dialog toggle opt out. The worktree directory name does not change.
- Colors use ANSI named colors and modifiers only. Base style `Color::Reset`. Selection `Modifier::REVERSED`. Diff green/red. No RGB anywhere.
- Worktrees live under `$XDG_DATA_HOME/ringleader/worktrees/<repo>/<slug>`, outside the repos.
- The app owns its config file. No config file is managed by Nix.
- MSRV 1.95, edition 2024, matching the rustc shipped in nixpkgs 26.05 (rustc 1.95.0, verified). Milestone 1 confirms this against the dev shell before any code is written.
- Session state is observed from tmux, not inferred from agent internals. A pure classifier turns observations into states. Activity comes from `window_activity` of the agent window, an epoch-seconds format variable that updates within 1 second of pane output. It covers every pane in the agent window, so output in a pane the user splits off the agent window also counts as agent activity and can hold the session in `Working`. tmux 3.6a has no per-pane activity time, and `pane_last_output_time` is not available.
- The agent pane is tagged with the pane user option `@ringleader_agent 1` (`set-option -p`) at creation. It is a pane option rather than a window option, so a pane the user splits off the agent window never carries the tag. Observation, preview capture, attach, and respawn use only the tagged pane, addressed by its `pane_id`. Activity and the attached-client count are the exceptions, since tmux tracks both per window. Other panes and windows in a managed session, including the `t` shell window and windows the user opens, are ignored.
- A user `after-new-session` or `after-new-window` hook runs before the next command in the same invocation. A hook that splits the new window makes `=<session>:` name the hook's pane, so the agent tag would land on the wrong pane. `new-session` and the agent `new-window` therefore pass `-P -F '#{pane_id}'`, and the same invocation ends with `display-message -p -t =<session>: '#{pane_id}'`. When the two ids differ, a second invocation unsets `@ringleader_agent` and `remain-on-exit` on the second pane and sets both on the created `pane_id`. `destroy-unattached off` is already set by then, so the second invocation cannot destroy the session. Window options such as `@ringleader_shell` are unaffected, since the split stays in the new window. Notification hooks such as `session-created` run after the invocation and do not affect the tag.
- Every tmux target that Ringleader builds is exact: `=<session>:` for sessions, a `window_id` such as `@4` for windows, and a `pane_id` such as `%3` for panes. Used as a window or pane target, `=<session>:` means the session's current window or pane, so it addresses a particular one only inside the invocation that just created it. A bare `-t <name>` prefix-matches other sessions on tmux 3.6a (`kill-session -t ringleader-fix` kills `ringleader-fix-auth`), so it is never used. Milestone 6 confirms the exact form per command on the pinned tmux.
- Quitting with `q` exits the TUI only. Managed tmux sessions keep running in the background and are rediscovered on the next launch.
- Session metadata is mirrored into tmux session user options so a live managed session can be adopted back after a restart or registry loss.
- Managed tmux sessions are created and configured in a single tmux invocation (`new-session ... ; set-option ...`). Setting options in a second call can destroy the session when the user's global `destroy-unattached` is on. The command separator is a standalone `;` argv element, never a shell-escaped `\;` from Rust. tmux treats any argv element ending in `;` as a separator, so every other dynamic argument in a tmux invocation (paths, names, option values) that ends in `;` is escaped by inserting a backslash before the final `;`.
- `TmuxCommand` passes `-u` on every tmux invocation. Without it, a client whose locale is not UTF-8, such as one in the Nix build sandbox with no `LANG`, gets every tab and every non-ASCII byte of command output replaced with `_`. That breaks the tab-separated `list-panes` rows, the metadata JSON, and saved key bindings.
- Every tmux child process that Ringleader spawns gets exactly the injected `ProcessEnvironment` snapshot with the configured agent `env` names removed. A new tmux server copies its first client's whole environment into its global environment, so without the removal a server that Ringleader starts would hand the agents' secrets to every pane on it, including `t` shells and the user's own windows.
- Every `new-session` and `new-window` that Ringleader issues passes `-c <worktree path>`. Without it, tmux starts the pane in the calling process's directory. For a missing `-c` directory, tmux silently uses `$HOME` and still exits 0, so tmux success never proves the worktree exists. tmux expands formats in the `-c` value, so `TmuxCommand` doubles every `#` in the path. Without that, a path containing `#{` starts the pane in another directory and still exits 0, and a path containing `#(` runs a shell command.
- Managed tmux sessions set `destroy-unattached off`, so detaching, including via Ctrl-Q, never destroys them, even if the user's global config enables it.
- Managed tmux sessions set `remain-on-exit on` on the agent pane (`set-option -p -t =<session>: remain-on-exit on`), so an agent exit leaves a dead pane that `restart` can revive. The window form would also keep every pane the user splits off the agent window open as a dead pane after it exits. The pane setting survives `respawn-pane`. `remain-on-exit` is a window and pane option, not a session option.
- Detached sessions get an explicit size (`-x 200 -y 50` by default, configurable) so previews are not limited to 80x24. The size holds only until a client first attaches. tmux's default `window-size latest` then keeps the last client's size after it detaches, so the preview crops whatever size the pane has.
- Agents launch through a hidden `ringleader __exec-agent --handoff <path>` subcommand. Environment values and the agent argv never appear on a tmux command line. The creating process writes a handoff file holding the agent argv, the environment values, and the worktree path, and tmux runs only the launcher with the file's path. The launcher reads the file, deletes it, runs `env_clear`, sets exactly the environment from the file, changes into the worktree path, and execs the agent. A missing worktree path makes the launcher print an error and exit nonzero, so the dead pane shows the failure instead of an agent running in another directory. That environment is a fixed baseline (section 8) plus the agent's configured `env` names, with values taken from the creating process only. The launcher also keeps `TERM`, `COLORTERM`, `TMUX`, and `TMUX_PANE` from its own pane environment, read from the `ProcessEnvironment` snapshot that `main` captures, and sets `GPG_TTY` to its own terminal. A name the creating process lacks is absent for the agent even if the tmux server's environment has it, so server environment drift never reaches the agent. The launcher never reads `config.toml`. Only variable names are configured and logged, never values.
- `main` captures the process environment once, with `std::env::vars_os`, into a `ProcessEnvironment` snapshot and injects it into `TmuxCommand`, `AgentLauncher`, `TomlConfigSource`, and `SystemHostProbe`, which resolves agent commands on the snapshot's `PATH`. `main` also passes the snapshot's `$SHELL`, with `sh` as fallback, to `SessionAccess`, which uses it for the shell window and the ephemeral shell request. `main` passes `TmuxCommand` the union of every configured agent's `env` names at startup, and `SessionSpec` carries that union from the current config, so a name added by `r` is still removed from the `new-session` call that can start a server. Nothing below `main` reads the process environment, so tests pass fixture snapshots and never call `std::env::set_var`, which is `unsafe` in edition 2024 and racy across test threads. `vars_os` also never panics on a non-UTF-8 variable, which `std::env::vars` does.
- Base branch resolution never fetches. It reads local refs only. On a repo with an unborn HEAD and no resolvable base, creation runs `git worktree add --orphan -b <branch> <path>`, which needs git 2.42 or newer. Branches are always created with `--no-track`, so an agent branch never gets an upstream. The orphan form is the one exception: git rejects `--no-track` with `--orphan`, and an unborn branch has no upstream to set.
- Session names use one canonical slug function for the registry name, tmux session name, worktree directory, and branch, with a 64-character cap and uniqueness checked on the slug. The slug alphabet (section 4) makes every slug a legal tmux name and a legal git ref component by construction, so the domain validates without calling git.
- Attach behavior depends on `$TMUX`, which `main` reads once and injects into the tmux adapters and the `App`. Outside tmux, `tmux attach-session` runs as a child under the terminal guard. Inside tmux, `tmux switch-client` switches the current client and the TUI keeps running. A child process cannot use `attach-session` from inside tmux; it fails with "sessions should be nested with care".
- Only the TUI installs the Ctrl-Q return binding. The CLI never touches tmux key bindings.
- Locks are taken once per process by the entry point, never inside `application`. `main` takes the TUI's lifetime lock before building the `App`, and `Cli` takes the write lock for mutating commands.
- Mouse support is out of scope for v1.
- Terminal bell on `Working` to `Waiting` is config-gated, off by default. Outside tmux, the TUI writes BEL to its own terminal while the home screen shows. It does not poll during an attach, so no bell fires then. Inside tmux, BEL goes to the tty of every client on the server, so it reaches the user in any session, even when the TUI's pane is hidden. A session whose agent window a client shows never rings, since the user is already looking at it.
- No short alias next to `ringleader` unless asked later.
- MIT license, matching the project owner's other personal tools.

## 3. Repo layout

```
.
├── Cargo.toml              # rust-version = "1.95", edition = "2024"
├── Cargo.lock
├── rustfmt.toml            # max_width = 100
├── flake.nix
├── flake.lock
├── .github/workflows/ci.yml
├── README.md               # human readers, terse
├── AGENTS.md               # agent repo documentation, living
├── docs/
│   └── plans/
│       └── v1-bootstrap.md  # this plan, historical record
├── tests/fixtures/         # fake config, registry, tmux and git output
└── src/
    ├── main.rs             # composition root
    ├── cli.rs              # Cli struct, clap definitions, dispatch
    ├── domain/
    │   ├── mod.rs
    │   ├── session.rs            # Session
    │   ├── session_name.rs       # SessionName
    │   ├── session_state.rs      # SessionState
    │   ├── session_metadata.rs   # SessionMetadata
    │   ├── session_observation.rs # SessionObservation
    │   ├── session_classifier.rs # SessionClassifier, the pure state classifier
    │   ├── diff_stat.rs          # DiffStat
    │   ├── session_summary.rs    # SessionSummary
    │   ├── agent.rs              # Agent
    │   ├── repo.rs               # Repo
    │   ├── repo_name.rs          # RepoName
    │   ├── base_branch.rs        # BaseBranch
    │   └── config.rs             # Config, the parsed config.toml
    ├── application/
    │   ├── mod.rs
    │   ├── session_service.rs    # SessionService, the facade the UI and CLI call
    │   ├── session_lifecycle.rs  # SessionLifecycle: create, delete, rename, restart
    │   ├── session_monitor.rs    # SessionMonitor: reconcile, observe, diffs, preview, bell
    │   ├── session_access.rs     # SessionAccess: attach, open_shell, return binding
    │   ├── repo_catalog.rs       # RepoCatalog: discover, list, add, remove repos
    │   ├── doctor.rs             # Doctor
    │   └── ports/
    │       ├── mod.rs
    │       ├── session_registry.rs   # SessionRegistry trait
    │       ├── workspace.rs          # Workspace trait
    │       ├── multiplexer.rs        # Multiplexer trait
    │       ├── return_binding.rs     # ReturnBinding trait
    │       ├── config_source.rs      # ConfigSource trait
    │       ├── repo_scanner.rs       # RepoScanner trait
    │       ├── host_probe.rs         # HostProbe trait
    │       └── lock.rs               # Lock trait
    ├── adapters/
    │   ├── mod.rs
    │   ├── agent_launcher.rs     # AgentLauncher, the hidden __exec-agent mode, owns AgentHandoff and the exec seam
    │   ├── process_environment.rs # ProcessEnvironment, the environment snapshot main captures once
    │   ├── directory_repo_scanner.rs # DirectoryRepoScanner
    │   ├── toml_config_source.rs # TomlConfigSource
    │   ├── json_session_registry.rs # JsonSessionRegistry
    │   ├── file_lock.rs          # FileLock
    │   ├── git_workspace.rs      # GitWorkspace
    │   ├── system_host_probe.rs  # SystemHostProbe
    │   ├── tmux_command.rs       # TmuxCommand, socket and config args, `-u`, child environment, exact targets, `;` escaping
    │   ├── tmux_multiplexer.rs   # TmuxMultiplexer
    │   └── tmux_return_binding.rs # TmuxReturnBinding, owns bindings.json
    ├── ui/
    │   ├── mod.rs
    │   ├── app.rs                  # App
    │   ├── event_loop.rs           # EventLoop
    │   ├── terminal_guard.rs       # TerminalGuard
    │   ├── home_view.rs            # HomeView
    │   ├── help_view.rs            # HelpView
    │   ├── status_bar.rs           # StatusBar
    │   ├── theme.rs                # Theme
    │   └── dialogs/
    │       ├── mod.rs
    │       ├── new_session_dialog.rs    # NewSessionDialog
    │       ├── delete_session_dialog.rs # DeleteSessionDialog
    │       ├── rename_session_dialog.rs # RenameSessionDialog
    │       └── add_repo_dialog.rs       # AddRepoDialog
    └── test_support/               # #[cfg(test)] doubles shared by unit tests
        ├── mod.rs
        ├── fake_session_registry.rs # FakeSessionRegistry
        ├── fake_workspace.rs        # FakeWorkspace
        ├── fake_multiplexer.rs      # FakeMultiplexer
        ├── fake_return_binding.rs   # FakeReturnBinding
        ├── fake_config_source.rs    # FakeConfigSource
        ├── fake_repo_scanner.rs     # FakeRepoScanner
        ├── fake_host_probe.rs       # FakeHostProbe
        └── fake_lock.rs             # FakeLock
```

Rules for `AGENTS.md`:

- `domain` never imports `crossterm`, `ratatui`, `std::process`, or any adapter.
- `application` defines traits and use cases, never shells out, and never touches the filesystem directly.
- `ui` calls the `SessionService` facade only. `SessionLifecycle`, `SessionMonitor`, `SessionAccess`, `RepoCatalog`, and `Doctor` are composed by the facade and are not called from `ui` or `cli.rs`.
- OOP in Rust: all state in structs with private fields, all behavior in `impl` blocks, traits as interfaces, composition over inheritance, one primary type per responsibility, dependencies injected through constructors. No public data fields, no free functions except `main` and `#[test]` functions.
- One primary type per file, named after it in snake case. Supporting types owned by the primary type live in the same file, unit tests live inline. `mod.rs` declares and re-exports only. The tree in section 3 is the file contract; do not merge primary types into one file.
- `cli.rs` owns the clap argument structs (`ListArgs`, `NewArgs`, and so on) as support types of `Cli`. They get no files of their own.
- Why the tree splits what it splits: `Session`, `SessionName`, and `SessionState` each get a file because each is used independently across modules, carries behavior or invariants, and changes for its own reasons. Merging files later is mechanical; splitting a file that grew is disruptive, so prefer the separate file. The reverse is also true: do not invent files for types the tree does not list.
- `src/test_support/` is `#[cfg(test)]` only and must not appear in the binary. It exists so unit tests across modules share one set of hand-written doubles.
- Other support types, such as a parser result, a doctor finding, a session spec, or a dialog's in-progress state, live in their primary type's file and get no tree entry.
- No code below `main` calls `std::env::current_exe`, reads the process environment (including `$TMUX`, `$TMUX_PANE`, `$HOME`, and `$SHELL`) or the process cwd, or resolves XDG paths. `main` reads them once and injects them, with environment values in the `ProcessEnvironment` snapshot.
- Comments: the repository is comment-hostile. Every comment is presumed unnecessary until proven otherwise. The fix for "this needs explaining" is better code: rename, extract, type, restructure. When a comment seems to survive, stop and justify it to the project owner before committing; never announce comments in the commit body. Doc-comments are comments and follow the same rule; do not document what a well-named item already says.
- Human-facing prose uses "Ringleader". Identifiers stay lowercase.
- Do not create another repository or remote. Push only when the project owner asks.

## 4. Domain

- `SessionName`: one canonical slug function used for registry name, tmux session name, worktree directory, and branch. The slug lowercases the input, maps every character outside `[a-z0-9-]` to `-`, collapses runs of `-`, trims `-` at both ends, truncates to 64 characters, and trims again. An empty result is a validation error (exit 1). The alphabet makes every slug a legal tmux session name (no `:` or `.`) and a legal git ref component, so `SessionName` validates in pure Rust and never calls git. Uniqueness is global across repos and checked on the slug, so two names that slug to the same string are rejected. Availability beyond the registry (worktree directory and branch) is checked by `SessionLifecycle` through the ports.
- `RepoName`: the same alphabet and slug function as `SessionName`. Names derived from a directory basename are slugged. A repo name is a path segment under `worktree_root`, so the alphabet also rules out `/` and `..`.
- `Agent`: name, launch command, environment variable names the launcher restores, and `prompt_args`, the argument template used to pass an initial prompt. The name is the lookup key used by `--agent`, the registry, and `@ringleader_meta`, compared exactly, and is also what the UI displays. Values are never stored in the domain.
- `SessionState`: `Working`, `Waiting`, `Exited`, `Stopped`, `Unmanaged`. `SessionState` owns the bell edge predicate: it is true only for a previous `Working` followed by a current `Waiting`.
- `Session`: name, repo name, repo path, worktree path, branch, base commit, agent, tmux session name, tmux socket path, created timestamp (epoch seconds). The base commit is the start-point SHA resolved at creation, empty for an orphan worktree. The socket path is the server's `#{socket_path}`, read in the creating invocation. With ten fields, `Session` is built through a builder or a parameter struct, since a ten-argument `new` fails `clippy::too_many_arguments` under `-D warnings`.
- `SessionMetadata { repo_name, repo_path, branch, base_commit, worktree_path, agent }`, the copy mirrored into tmux.
- `SessionObservation { exists, attached, pane_dead, activity_at, created_at, socket_path, metadata: Option<SessionMetadata> }`, built from the tagged agent pane only. `attached` is true when any client shows the agent window, read from `#{window_active_clients}`. A client on the `t` shell window of the same session does not count. `socket_path` is the server's `#{socket_path}`. A live session with metadata but no tagged agent pane is observed with `pane_dead` true.
- `Config`: parsed config.toml, with explicit repo overrides, agents, prefixes, refresh intervals, and the bell flag.
- `RepoScope`: the resolved directory to scan, produced by `RepoScanner`. Support type on `Repo`. The launch directory enters through `main` and is threaded through the service, so neither domain nor application reads the process cwd.
- `SessionClassifier`, pure, takes `now` as an argument, never reads the clock itself. No stored history:
  1. session missing, `Stopped`
  2. no valid metadata (malformed, absent, or rejected by the adoption checks in section 8), `Unmanaged`
  3. `pane_dead`, `Exited`
  4. `now - activity_at < activity_idle_seconds`, `Working`
  5. otherwise, `Waiting`
- `DiffStat { files, insertions, deletions, untracked }`, computed per request, not stored. It measures the worktree against the session's base commit: everything the agent committed plus uncommitted tracked changes. For an orphan worktree the base is the empty tree, obtained with `git -C <worktree> hash-object -t tree /dev/null` so SHA-256 repositories work too. Run outside the repository, the same command prints the SHA-1 empty tree. `untracked` counts files git would add but does not track yet. A binary file counts as one file with zero insertions and deletions. It is computed with `git diff-index --numstat <base>` and `git ls-files --others --exclude-standard -z`, both run with `GIT_OPTIONAL_LOCKS=0`. Porcelain `git diff` refreshes and rewrites the worktree's index under `index.lock`, even with optional locks off, which makes the agent's own `git add` fail while a poll runs. `diff-index` writes nothing and never detects renames, so user config cannot change the totals.
- `SessionSummary { session, state, diff: Option<DiffStat> }` is what the service returns to the UI. `diff` is `None` until computed and always `None` for `Unmanaged` sessions.

`Waiting` is the green bullet: the pane is running and quiet, so the agent is at its prompt. `Exited` requires `remain-on-exit on` on the agent pane, set at creation, so the pane survives agent exit and `respawn-pane` can restart it.

## 5. Ports and use cases

Ports:

- `SessionRegistry`: `load`, `save`.
- `Workspace`: `create_worktree(repo, path, branch, start_point)`, `remove_worktree`, `list_worktrees(repo)`, `list_worktree_dirs(root)`, `delete_branch`, `rename_branch`, `branch_exists`, `branch_tip(repo, branch)`, `list_branches(repo, prefix)`, `resolve_start_point(repo, base_branch)`, `dirty_state`, `unmerged_commits(repo, branch, worktree)`, `diff_stat(worktree, base_commit)`, `validate_repo`, `version`. `resolve_start_point` returns either a commit SHA or the orphan case. `list_worktrees` entries carry the path, branch, HEAD, and the locked and prunable flags from `git worktree list --porcelain`. git prints every path with symlinks resolved, even for a missing worktree, so every comparison between a stored worktree path and a listed path first canonicalizes the stored path the way `delete_session` step 3 does.
- A branch is unborn when a `list_worktrees` entry names it but `git rev-parse --verify -q refs/heads/<branch>` fails, which is an orphan session's state before its first commit. A branch is missing when no entry names it and the same check fails. A delete retried after step 7 finds its branch missing. So does an orphan session's delete retried after step 6, because removing the worktree drops the only record of an unborn branch, and so does a branch the user deleted by hand. For an unborn or missing branch, `branch_tip` is `None`. `branch_exists` is true for an unborn branch and false for a missing one.
- `unmerged_commits` counts commits reachable from the branch tip or from the worktree's HEAD that no other branch, tag, or remote-tracking ref reaches: `git rev-list --count refs/heads/<branch> <worktree HEAD> --not --exclude=<branch without refs/heads/> --branches --tags --remotes`. Including the worktree HEAD counts commits made on a detached HEAD. The HEAD comes from the `list_worktrees` entry, which still reports it when the worktree directory is missing. Each positive argument is left out when it does not resolve: the branch when it is unborn or missing, and the HEAD when the entry shows the all-zero HEAD of an unborn branch or no entry lists the worktree. With neither left, the count is 0 without running git. Never use `--all`. It includes the HEAD of every worktree, and every session's branch is checked out in its own worktree, so the count would always be 0. With `--branches`, the exclude pattern must drop the `refs/heads/` prefix, or it excludes nothing.
- `dirty_state` runs `git status --porcelain=v1 -z --untracked-files=all --no-renames` with `GIT_OPTIONAL_LOCKS=0`, so user config can neither hide new files nor merge a rename into one entry. Every record then holds one path, and every file inside an untracked directory counts. It reports a missing worktree directory as `Missing`, never as an error. Ignored files are not counted, yet `git worktree remove` deletes them, so every delete warning says so.
- `Multiplexer`: `create_session(spec)`, `kill_session`, `has_session_on(socket, name)`, `caller_session`, `observe_all`, `attach(target)`, `capture_pane`, `rename_session(old, new, metadata)`, `respawn_agent`, `ensure_shell_window`, `ring_bell`, `server_version`, `version`. `create_session` takes a `SessionSpec` support type (tmux session name, worktree path, size, metadata, agent argv, environment names, and the union of every configured agent's `env` names to remove from the tmux client) and is used both for new sessions and for recreating a `Stopped` session in its existing worktree. It returns the server's socket path. `attach(target)` runs `attach-session` or `switch-client` based on the injected inside-tmux flag. Its target is an `AttachTarget` support type. `AttachTarget::Agent(session)` resolves to the tagged agent `pane_id`, which selects the agent window even after `t` made the shell window current. Only a session with no tagged agent pane is attached by `=<session>:`. `AttachTarget::Shell(window_id)` takes the `window_id` that `ensure_shell_window` returns. `attach`, `capture_pane`, and `respawn_agent` resolve the tagged agent pane with a fresh `list-panes -s -t =<session>:` lookup rather than an id from an earlier poll. `rename_session` runs `rename-session` and the `@ringleader_meta` update in one invocation, which tmux stops at the first failure. `caller_session` returns the session that holds the injected `TMUX_PANE`, or `None` outside tmux. `has_session_on` runs `has-session -t =<session>:` against a recorded socket with `-S`. Exit 0 means live. `can't find session`, `no current target`, `no server running on <path>`, and `error connecting to <path> (No such file or directory)` mean not live. Any other failure, such as `Permission denied`, refuses the action with an adapter error. `ring_bell` is the inside-tmux half of the bell (section 6). `server_version` reads the running server's `#{version}`, which can differ from the client's `tmux -V`.
- `ReturnBinding`: `install`, `restore`, `repair_stale`. See section 6, Attach and Ctrl-Q.
- `ConfigSource`: `load`, `add_repo`, `remove_repo`.
- `RepoScanner`: `resolve_scope(launch_directory) -> RepoScope` and `scan(scope) -> Vec<Repo>`, returning canonical paths. The nearest-`.git` walk and the bounded directory walk both live in the adapter, keeping the service free of filesystem logic.
- `HostProbe`: `resolve_command(name)`, `is_writable(path)`, `path_exists(path)`, `is_file(path)`, `canonicalize_lenient(path)`. `delete_session` step 4 uses `path_exists` on the worktree directory and `is_file` on its `.git`, which is false both for a missing `.git` and for a `.git` directory. `canonicalize_lenient` canonicalizes the nearest existing ancestor of a path and appends the remaining components. `Doctor` and config validation use it. `SessionLifecycle`, `SessionMonitor`, and `SessionAccess` also take it, for the containment check that `delete_session` step 3 and reconciliation share and for every missing-worktree check that must run before tmux.
- `Lock`: `acquire_lifetime`, `acquire_for_write`. Used by `main` and `Cli` only.

Application structure: `SessionService` is a thin facade that composes `SessionLifecycle`, `SessionMonitor`, `SessionAccess`, `RepoCatalog`, and `Doctor`, each with only the ports it needs. The use cases below are the facade's methods.

Use cases:

- `observe(persist)`: one `observe_all` call, reconcile, classify, no git. Returns summaries with no diffs. The TUI calls it on the `state_poll_ms` cadence with `persist` true, and it writes the registry only when reconciliation changed something. `list` passes false.
- `reconcile`: the step inside `observe` that merges the registry with live tmux sessions. Adopt live sessions matching `tmux_prefix` that carry valid metadata but have no registry entry. Keep registry entries with no live session as `Stopped`.
- `diff_stats(names)`: diffs for the given sessions only. The TUI passes one queued visible row per call (section 6), and `list` passes every session.
- `preview(name)`: capture the agent pane of one session, on the `preview_refresh_ms` cadence.
- `reload()`: reload config and registry from disk, used by `r`.
- `discover_repos(launch_directory)`: `RepoScanner.resolve_scope`, then `scan`, then merge explicit config overrides keyed by canonical path with explicit winning, dedupe, and return. Never filters sessions. Cached in memory for the life of the picker; `r` re-scans. The launch directory is passed in from `main`, so the service never reads cwd itself. Changing the scope in the picker calls it again with the new path.
- `list_repos(scope, include_all)`, `add_repo(path, name, base_branch)`, `remove_repo(name)`: back the `n` dialog's "Add repo" row and the `repo` subcommands.
- `create_session(request)`:
  1. Validate the request. Without an agent, use the first `[[agents]]` entry.
  2. Check availability. The slug is unused by registry entries and live tmux sessions, `<worktree_root>/<repo>/<slug>` does not exist and is no registry entry's worktree path, and `<branch_prefix><slug>` does not exist (`branch_exists`). All three are checked before any git call that changes state.
  3. Resolve the start point lazily, for this repo only (section 8).
  4. Create the worktree and branch with `--no-track`, or `--orphan` for an unborn HEAD. If `git worktree add` fails, remove the worktree with `git worktree remove --force` when `list_worktrees` shows the target path, then delete the branch if git created it. A failing post-checkout hook in the user's repo makes git exit nonzero after creating both, and it counts as a creation failure.
  5. Create the detached tmux session in the worktree in one invocation. It runs the launcher with a handoff file, tags the agent pane `@ringleader_agent`, sets `remain-on-exit on` on the agent pane, sets `destroy-unattached off` on the session, writes `@ringleader_meta`, applies the `-x/-y` size, reads the server's `#{socket_path}`, and prints the created and the current `pane_id`. When a user hook split the window and the two ids differ, a second invocation moves the tag and `remain-on-exit` as section 2 describes. tmux stops a command sequence at the first failing command without undoing earlier ones, so a failure can leave the new session running. On failure, first run `kill-session -t =<session>:`, ignoring a missing session, then remove the worktree and branch from step 4.
  6. Persist.
  The agent argv is built by expanding `prompt_args` with the request's prompt. The prompt is not stored in the session or registry. `create_session` never touches key bindings.
- `delete_session(name, confirmation)`: `confirmation` is a `DeleteConfirmation { dirty, unmerged, working }` support type.
  1. Refuse `Unmanaged` sessions with a conflict. Refuse with a conflict when `caller_session` is the target, because killing it would end the deleting process partway through. Refuse with a conflict when a `Stopped` session is live on its recorded socket (`has_session_on`, section 8).
  2. Re-check the guards at execution time: dirty file count, `unmerged_commits` count, and whether the state is `Working`. A missing worktree counts as zero dirty files. Any condition that is present but not confirmed refuses with a conflict naming every unconfirmed condition, so a check that went stale while a dialog was open never deletes unseen work.
  3. Refuse filesystem and branch operations when the worktree path is not strictly inside the canonical `worktree_root`, or the branch does not start with `branch_prefix`. Containment canonicalizes the nearest existing ancestor of `worktree_root` and of the worktree path through `HostProbe::canonicalize_lenient`, appends the remaining components of each, and rejects any `..` component, so a missing worktree directory or a missing worktree root still passes.
  4. Check that git will accept the removal before touching anything. Refuse when `list_worktrees` shows the path as the main worktree or a locked worktree, when the path exists but git does not list it, or when the branch is checked out in a worktree other than the session's. Also refuse when the worktree directory exists but its `.git` is missing or is not a file, since `git worktree remove --force` refuses both. `list_worktrees` marks a worktree whose `.git` is missing as `prunable` even though its directory exists, so this step checks the directory on disk, and no step reads the `prunable` flag. Record the branch tip SHA and the worktree HEAD SHA for step 7.
  5. Kill the tmux session if it is live.
  6. Remove the worktree with `git worktree remove --force`. On git 2.42 and newer it also unregisters a worktree whose directory is already gone, so Ringleader never runs a repo-wide `git worktree prune`. When `list_worktrees` no longer lists the path, skip this step. If git refuses, stop with its stderr, and keep the branch and the registry entry.
  7. Log the branch tip SHA, and the worktree HEAD SHA when it differs, at info level, then delete the branch with `-D`. An unborn branch has no ref to delete, and an already-missing branch is tolerated, so both skip `-D` and continue.
  8. Remove the registry entry and persist.
  Return the deleted tip SHA, plus the worktree HEAD SHA when it differs, so the CLI can print them and the user can recover with `git branch <name> <sha>`. An unborn branch returns no SHA, and the CLI prints that the session had no commits. A missing branch also returns no SHA, and the CLI prints that the branch was already gone.
- `rename_session(old, new, keep_branch)`: refuse `Unmanaged` sessions. Validate the new name for availability on the slug and the tmux session name, and, unless `keep_branch`, fail early if the target branch exists. Rename the branch first, in the worktree, or with `git -C <repo>` when the worktree is missing. For a live session, then call `rename_session`, which renames the tmux session and refreshes `@ringleader_meta` in one invocation. A `Stopped` session has no tmux session, so it skips that step. Persist the registry last. On tmux failure, roll the branch rename back so the pair stays consistent. A `Stopped` session that is live on its recorded socket refuses with a conflict (`has_session_on`, section 8). Never touch the worktree directory.
- `attach(name, return_mode)`: `return_mode` says whether to ensure the Ctrl-Q return binding first; the TUI passes `InstallReturnBinding`, the CLI passes `None`. Then `Multiplexer::attach`, which targets the tagged agent pane. The facade reports the injected inside-tmux flag, and the UI wraps the call in `TerminalGuard` only when it is false. The CLI calls `attach` directly. Unmanaged sessions listed in the UI are attachable, not restartable or deletable.
- `open_shell(name, return_mode)`: if the tmux session is live, `ensure_shell_window` creates or finds a window identified by the window user option `@ringleader_shell` (not by name, since `allow-rename` can rename it), running `$SHELL` in the worktree, and returns its `window_id`. A new shell window is created without `-d` and tagged in the same invocation, so `=<session>:` resolves to it, and it prints its id with `new-window -P -F '#{window_id}'`. `Multiplexer::attach` then targets `AttachTarget::Shell(window_id)`, with the same `attach-session` or `switch-client` split as `attach`. A missing worktree fails before tmux runs, pointing to `doctor`. The window persists like any other. If the tmux session is stopped, return an ephemeral shell request (`$SHELL`, cwd = worktree) for the UI to run under the terminal guard. Unmanaged sessions have no trusted worktree path, so `t` is disabled for them with a status-bar message.
- `restart(name)`:
  - `Exited`: `respawn_agent` writes a fresh handoff file and runs `respawn-pane` on the tagged agent pane by `pane_id`, passing the launcher command explicitly and never relying on the pane's original invocation. If the session has no tagged agent pane, `respawn_agent` creates a new window running the launcher, without `-d`, and in the same invocation tags its pane and sets `remain-on-exit on`, so `=<session>:` resolves to the new window.
  - `Stopped` with the tmux session gone: `create_session(spec)` recreates it in the existing worktree with the same single-invocation configuration. The registry entry records the socket path that `create_session` returns and is persisted. A missing worktree fails with a message pointing to `doctor`. The check runs through the `HostProbe` port before tmux runs, since tmux would start the pane in `$HOME` and exit 0. A `Stopped` session that is live on its recorded socket refuses with a conflict (`has_session_on`, section 8).
  - `Working` and `Waiting` refuse with a conflict, and `Unmanaged` sessions cannot be restarted.
  Restart never replays the initial prompt, even if one was used at creation. If the session's agent no longer exists in config, fail with a clear message and a doctor finding.
- `ring_bell()`: delegates to `Multiplexer::ring_bell`.
- `repair_return_binding()` and `release_return_binding()`: called by the TUI at startup and exit.
- `doctor()`: see section 7.

## 6. UI

Layout target at 100x28, and 80x24 is the smallest Full tier. The implementing agent must match this composition: fixed left table, preview panel on the right, one status bar, borders drawn with the terminal's box-drawing style. Widths are proportional: the table takes about half the width, the preview the rest, and only the selected row is reverse video.

```
┌ Sessions ──────────────────────────────────────┐┌ Preview ───────────────────────────────────────┐
│  Name          Branch               Diff       ││ ringleader-fix-auth · working · claude         │
│⠹ fix-auth      ringleader/fix-auth  +124 -31 ?2││ …ringleader/worktrees/example-project/fix-auth │
│⠹ spike-tui     ringleader/spike-tui +0 -0 ?0   ││ ────────────────────────────────────────────── │
│● dark-mode     ringleader/dark-mode +12 -4 ?0  ││ ✻ Thinking…                                    │
│✗ docs          ringleader/docs      +57 -2 ?0  ││                                                │
│○ flaky-test    ringleader/flaky-test +3 -1 ?0  ││   Read src/ui/home_view.rs                     │
│? mystery       ringleader/mystery   —          ││   ⎿  Read 214 lines                            │
│                                                ││                                                │
│                                                ││   Edited src/domain/session_name.rs            │
│                                                ││   ⎿  Added 12, removed 3                       │
│                                                ││                                                │
│                                                ││ ✻ Cogitating… (12s · ↑ 1.2k tokens)            │
│                                                ││                                                │
└────────────────────────────────────────────────┘└────────────────────────────────────────────────┘
 n new   d delete   R rename   Enter attach   t shell   s restart   r refresh   ? help   q quit
```

- The selected row uses `Modifier::REVERSED`, not a marker glyph, so selection is visible on any theme.
- Preview header line one: tmux session name, state, agent name. Line two: worktree path, truncated from the left. Then captured pane content.
- The unmanaged row shows `—` for diff, because it has no trusted worktree.

Empty state on first launch:

```
┌ Sessions ──────────────────────────────────────┐┌ Preview ───────────────────────────────────────┐
│  Name          Branch               Diff       ││                                                │
│                                                ││                No sessions yet.                │
│                                                ││             Press n to create one.             │
│                                                ││                                                │
│                                                ││                                                │
└────────────────────────────────────────────────┘└────────────────────────────────────────────────┘
```

Responsive layout tiers. The home screen adapts to the terminal size on every render. The tier is derived by `HomeView` from the current terminal dimensions, as a support type in `home_view.rs`. No config key, no persistence.

| Tier | Condition | Layout |
|---|---|---|
| Full | width >= 80 and height >= 24 | Table and preview side by side |
| Compact | width >= 60 and height >= 12 | Table full width, preview hidden |
| Minimal | width >= 36 and height >= 8 | Table full width, columns reduced to state and name |
| TooSmall | below 36 wide or 8 tall | Centered `Terminal too small` |

Compact tier at 60 wide, preview hidden, branch and diff kept:

```
┌ Sessions ────────────────────────────────────────────────┐
│  Name            Branch                 Diff             │
│⠹ fix-auth        ringleader/fix-auth    +124 -31 ?2      │
│● dark-mode       ringleader/dark-mode   +12 -4 ?0        │
│✗ docs            ringleader/docs        +57 -2 ?0        │
│○ flaky-test      ringleader/flaky-test  +3 -1 ?0         │
└──────────────────────────────────────────────────────────┘
 n new   d delete   Enter attach   ? help   q quit
```

Minimal tier at 36 wide, state and name only:

```
┌ Sessions ────────────────────────┐
│⠹ fix-auth                        │
│● dark-mode                       │
│✗ docs                            │
│○ flaky-test                      │
└──────────────────────────────────┘
 n new  Enter attach  ? help  q quit
```

- Dialogs render centered when they fit inside the current tier. On Compact and Minimal they use the full frame with one row of margin, and their field labels abbreviate rather than wrapping.
- Status bar display order is fixed: `n`, `d`, `R`, `Enter`, `t`, `s`, `r`, `?`, `q`. Entries are separated by three spaces. As width shrinks, drop the lowest priority first: `r`, `s`, `t`, `R`, `d`. `n`, `Enter`, `?`, and `q` are always shown; when they do not fit with three-space gaps, the gaps shrink to two spaces.
- The table drops columns in priority order when space is tight: branch first, then diff. Name truncates with an ellipsis at a minimum of 12 columns. State and name never drop.
- `Event::Resize` forces a full redraw so tiers recalculate instantly.

Home screen:

- Left table columns: state indicator, name, branch, diff `+x -y ?u`.
- State indicator: `Working` animated Braille spinner, `Waiting` green `●`, `Exited` red `✗`, `Stopped` dim `○`, `Unmanaged` yellow `?`.
- Spinner frame derives from the app tick counter, not per-session state.
- Navigation with arrows and `h`/`j`/`k`/`l`. Selection is tracked by `SessionName`, never by row index, so rows changing state do not move the cursor.
- Right panel: live preview via `preview(name)`, which runs `tmux capture-pane -e -p` on the tagged agent pane on its own `preview_refresh_ms` timer, independent of the state poll, and again immediately when the selection changes. There is no `-J`, so every captured row is at most the pane's width. The preview trims trailing blank rows, shows the last rows that fit, anchored to the bottom where agents keep their prompt, and clips on the right without wrapping. Render ANSI through `ansi-to-tui`, fall back to plain text if parsing fails. On capture failure show session metadata.
- Diffs are computed only for visible rows, through `diff_stats(names)`, never for the whole registry on every tick. The event loop ticks every 100 ms. It polls input until the next tick deadline, then runs the timers that are due. Git runs on the event-loop thread. Every `diff_refresh_ms`, the TUI queues the visible rows. Each tick, `App` passes queued rows to `diff_stats` one name per call, at most two per tick, and makes no further call once 50 ms have passed in the tick. A pass over twenty rows then spreads over several ticks instead of freezing input. One slow row still blocks input for its own duration. Section 16 names the worker thread to add if that stalls input on large repos. Twenty sessions must not spawn forty git processes per tick.
- Status bar lists keys and transient errors with the fixed order and width-based dropping described in the responsive tiers: `n` new, `d` delete, `R` rename, `Enter` attach, `t` shell, `s` restart, `r` refresh, `?` help, `q` quit.
- Create, delete, rename, and restart block the event loop while git and tmux run, so the UI first draws a frame with a busy message in the status bar.
- Sort order: state priority `Working`, `Waiting`, `Exited`, `Stopped`, `Unmanaged`, then name. Stable across refreshes because selection is name-based.
- Terminal-too-small guard: the TooSmall tier below 36x8, see the responsive tiers above. There is no fixed 80x24 cutoff; tiers scale all the way down.
- `n` dialog: name field, repo picker, agent picker, an optional initial prompt field, and an "Add repo" row. The agent picker preselects the first configured agent. The prompt field is single-line and submitted with the session; leaving it empty creates the session with no initial message. The picker lists `discover_repos` for the launch scope, with explicit config overrides merged in and marked. The resolved scope path is shown at the top of the picker and can be changed with a path input.
- Empty picker state: when the scope contains no repos, show "no repos found in <scope>" with two actions: "Add repo" and "Change scope". No silent fallback to `$HOME`.
- "Add repo" row: opens a path input and validates it with `git -C <path> rev-parse --path-format=absolute --show-toplevel --git-dir --git-common-dir --show-superproject-working-tree`. It rejects a linked worktree, where the git dir differs from the common dir, and a submodule, where the superproject is not empty, because `--show-toplevel` alone accepts both. Outside a submodule git prints no superproject line, so the output has three lines for a repository and four for a submodule. It stores the canonical top level rather than the typed path, derives the name from the top level's slugged basename, appends an explicit override to the config with `toml_edit`, reloads the picker, selects the new repo. This is the fix for the claude-squad pain point. Adding is optional: scanned repos already work without config entries.
- `d` dialog: shows name, branch, worktree, diff. A dirty worktree shows "N uncommitted files will be lost". A branch or worktree HEAD with commits on no other ref shows "N commits exist on no other ref and will be lost". Either one requires a second confirmation. A `Working` session additionally warns "agent is actively working". The dialog always states that ignored files such as `.env` are deleted with the worktree, since the dirty count leaves them out. Deleting the session the TUI itself runs in is refused with a status-bar message. Confirming passes a `DeleteConfirmation` with a flag set for each warning that was shown. If the service refuses because the state changed after the dialog opened, the dialog reopens with fresh numbers.
- `R` dialog: old and new name fields plus a "rename branch" toggle, on by default. The dialog shows the branch rename that will happen.
- `r`: `reload()` config and registry from disk, refresh diffs and observations.
- `t`: open a terminal in the selected session's worktree. If the tmux session is live, create or select the shell window (identified by `@ringleader_shell`, so a user rename cannot break it) in that session, running `$SHELL` in the worktree directory, and attach the client to it, so it persists like any other tmux window. If the tmux session is stopped, suspend the TUI and run an ephemeral `$SHELL` in the worktree, returning to the TUI on exit. Uses the same terminal guard as attach. `$SHELL` is the value `main` passes to `SessionAccess`, with `sh` as fallback. Disabled for unmanaged sessions. With no session selected, `t` shows a status-bar message and does nothing.
- `s`: restart the selected session. `Exited` sessions get `respawn-pane`; `Stopped` sessions get their tmux session recreated in the existing worktree. Disabled for `Working`, `Waiting`, and `Unmanaged` with a status-bar message.
- `q`: quit the app. tmux sessions keep running in the background; the next launch reconciles and lists them again.
- `?` help dialog: key bindings, config and log paths, version, and the Ctrl-Q behavior including the inside-tmux caveat. It also tells the user to open extra shells with `t` instead of splitting the agent window, since output in a split pane counts as agent activity. Centered overlay, closed with `?`, `Esc`, or `q`.
- Bell: when `bell_on_waiting` is enabled, `App` keeps the previous state per session and asks `SessionState`'s edge predicate whether the new state is a `Working` to `Waiting` transition. On that edge, it rings unless a client shows the agent window. A client on the `t` shell window of the same session does not count, because typing there moves only the shell window's `window_activity`. Outside tmux, `App` writes BEL to its own terminal. Inside tmux, it calls `ring_bell()`, which writes BEL to the tty of every client on the server (`list-clients -F '#{client_tty}'`), so it reaches the user in any session, even when the TUI's pane is hidden. It skips control-mode clients, whose tty is empty. A client on the agent window suppresses the bell because typing into the agent also moves `window_activity` and the user is already looking at it. No bell for `Exited`. While an outside-tmux attach child runs, the TUI is suspended and does not poll, so the bell fires only inside tmux or while the home screen is shown. The help dialog states this.
- Event loop drains `Event::Resize` and forces a full redraw, so the too-small guard clears and recalculates when the terminal grows.

Attach and Ctrl-Q:

- `main` detects `$TMUX` once. Outside tmux: terminal guard leaves raw mode and alternate screen, runs `tmux attach-session -t <agent pane_id>` as a child, and after it returns re-enters raw mode and the alternate screen and calls `terminal.clear()`. Re-entering the alternate screen blanks it while ratatui still holds the previous frame and repaints only changed cells, so without the clear the home screen comes back empty. The ephemeral `t` shell resumes the same way.
- Inside tmux: run `tmux switch-client -t <agent pane_id>`. The TUI keeps running and polling. `attach-session` cannot be used from inside tmux by a child process; it fails with "sessions should be nested with care".
- `t` uses the same paths: live sessions get the shell window and the outside/inside attach split, stopped sessions get an ephemeral `$SHELL` under the terminal guard. Ctrl-Q returns to the TUI from the shell window too, since the binding matches on managed session names.
- Ctrl-Q is chosen as the return key over Ctrl-D because Ctrl-D is meaningful to the agents themselves, such as EOF to exit Claude Code, so intercepting it would break the session.
- Only the TUI installs the binding. CLI `new` and `attach` never touch bindings, so a session attached from the CLI receives Ctrl-Q as a normal key unless a TUI is running.
- The binding embeds its return target, so it never depends on `client_last_session`, which changes as soon as the user switches sessions inside tmux:
  - Outside tmux: `bind-key -T root C-q if-shell -F '#{m:<prefix>*,#{session_name}}' 'detach-client' 'send-keys C-q'`. Detaching ends the attach child and the TUI resumes.
  - Inside tmux: `bind-key -T root C-q if-shell -F '#{m:<prefix>*,#{session_name}}' 'switch-client -t <tui_pane_id>' 'send-keys C-q'`, where `<tui_pane_id>` is the injected `$TMUX_PANE` (such as `%5`). A pane target selects the TUI's window and pane and survives a rename of the TUI's session. It also works when the TUI runs in a window of a managed session, where a session target would switch to the session already shown. Before installing, Ringleader confirms the pane with `display-message -p -t <tui_pane_id> '#{pane_id}'` and requires the output to equal the pane id, because `display-message` exits 0 with empty output for a missing target. When `$TMUX` is set but `$TMUX_PANE` is not, as in a `display-popup`, the TUI installs no binding and says so in the status bar.
  - Each branch is passed as one quoted command string. The brace form (`{ ... }` passed as separate arguments) fails on 3.6a with `unknown command` or `too many arguments`; do not use it. The prefix pattern is built from `tmux_prefix` at runtime, which config validation restricts to `[a-z0-9-]` so it cannot break the format string. `send-keys C-q` does not recurse.
- Install timing: `start-server` cannot keep a server alive with no sessions (`exit-empty`), so there is no server bootstrap step. The TUI calls `install` immediately before every attach, switch, or shell open, when the target session guarantees a running server. `install` is idempotent: when the current root C-q is exactly the binding it would install, it changes nothing and saves nothing. A binding that is recognizably Ringleader's, because its condition is the managed prefix match, but runs another command, such as a stale pane target or the other attach mode, is replaced without being saved as the user's binding. A fresh server started after the TUI launched gets the binding on the next attach.
- Saving the previous binding: before the first install on a server, `TmuxReturnBinding` runs `list-keys` with no arguments and selects the line that starts with `bind-key`, optionally `-r`, then `-T root C-q`, allowing any run of whitespace between tokens. That line is saved verbatim. No matching line means unbound. A nonzero exit aborts the install with a status-bar error. The idempotence check in `install` and the check in `repair_stale` read the current root C-q the same way. Filtering the listing is not portable. On a 3.7 server, `list-keys -T root C-q` and `list-keys -T root -1 C-q` print nothing and exit 0 whether or not C-q is bound, and `list-keys -T root` prints nothing when the root table holds exactly one binding. The behavior follows the server's version, not the client's. The record in `$XDG_STATE_HOME/ringleader/bindings.json` holds the server's `#{socket_path}` and `#{pid}` with the saved binding.
- Restoring: an unbound record runs `unbind-key -T root C-q`. A bound record writes the saved line to a temp file in the state directory and runs `tmux source-file` on it, since the saved line uses tmux quoting and cannot be replayed as argv. After `source-file`, Ringleader lists the bindings again and requires the C-q line to equal the saved line. On a nonzero exit or a mismatch, it unbinds C-q, logs the saved line, and prints it on exit so the user can rebind it by hand. tmux 3.6a lists a multi-line brace binding with a doubled `\;\;` that `source-file` rejects. `list-keys` without `-N` never prints a binding's note, so a restored binding loses it. The TUI's binding guard restores on exit and on panic, before the terminal guard runs.
- Crash safety: at startup, `repair_stale` reads `bindings.json`. When the recorded server is still running (same socket path and pid) and its root C-q is still recognizably Ringleader's, it restores the saved binding. Otherwise it discards the record, so a binding from an older server never overwrites the current server's config. This covers SIGKILL and power loss, which the panic hook and signal handling cannot.
- The per-session `key-table` alternative is rejected, not deferred. Testing on 3.6a showed it bypasses root bindings entirely and swallows unbound keys rather than forwarding them to the pane, and it replaces the user's whole root table for managed sessions. Do not revisit it.
- Verification criteria: C-q returns to the home page from managed sessions in both outside-tmux and inside-tmux contexts, including after the user switches between two managed sessions first, and when the TUI itself runs in a window of a managed session. C-q still reaches non-managed sessions. All pre-existing root bindings are restored on exit and on crash, including after two installs in one TUI run, on both a tmux 3.6 and a tmux 3.7 server.

## 7. CLI

- `list [--json]` reconciles in memory only and never writes. It computes diffs for every session. It is safe to run while the TUI is running. Only the TUI and mutating commands persist the registry. JSON output is an array of objects with `name`, `repo`, `branch`, `worktree`, `agent`, `state`, and `diff` (`files`, `insertions`, `deletions`, `untracked`, or `null` for unmanaged sessions).
- `new --name <n> --repo <r> [--agent <a>] [--base-branch <b>] [--prompt <text>]`. Without `--agent`, the first `[[agents]]` entry is used. `--prompt` passes an initial message to the agent via the agent's `prompt_args`. `--repo` accepts a discovered repo name or a path, resolved against `discover_repos` and then validated like the "Add repo" row (section 6), so a linked worktree or a submodule is rejected.
- `delete <name> [--force]`. The CLI never prompts. Without `--force`, a dirty worktree, unmerged commits, or a `Working` agent exits 3 and prints what would be lost. With `--force`, all three are confirmed and the warnings go to stderr. Either way it warns that ignored files such as `.env` are deleted with the worktree. Deleting the session the command runs in exits 3. On success it prints the deleted branch tip SHA, the worktree HEAD SHA when it differs, that the session had no commits, or that the branch was already gone.
- `attach <name>`. Runs `attach-session` or `switch-client` only. It takes no lock and never touches key bindings.
- `rename <old> <new> [--keep-branch]`.
- `restart <name>`, restarts an `Exited` or `Stopped` session.
- `restart`, `delete`, and `rename` exit 2 for an unknown name and 3 for an `Unmanaged` session. `restart` exits 3 for a `Working` or `Waiting` session and 1 when the session's agent is missing from config.
- `repo add <path> [--name <n>] [--base-branch <b>]` writes an explicit override, `repo remove <name>` removes an override and reports when the repo is only discovered by scanning, `repo list [--all]` lists the repos discovered for the current scope and marks explicit overrides, with `--all` also listing overrides outside the scope. `repo remove` refuses while any registry session references the repo, naming the sessions and suggesting `delete` first.
- `__exec-agent --handoff <path>`, hidden. Invoked by tmux through the launcher path that `main` injects (`current_exe` in production). Reads the handoff file, deletes it, `env_clear`s, sets the environment from the file plus the kept pane variables and `GPG_TTY`, changes into the worktree path from the file, and execs the agent argv from the file. A missing worktree path exits nonzero before exec. Never reads `config.toml`. Not documented in README.
- `doctor [--json]`, checks and reports:
  - tmux present, version >= 3.2, checked for the client (`tmux -V`) and, when a server is running, for the server (`#{version}`). The two can differ, since the Nix wrapper's client can drive a server that another tmux started, and the server's version decides behavior.
  - git present, version >= 2.42 required for `worktree add --orphan`, `worktree`, and `branch -m` in worktrees
  - each configured repo path resolves via `git rev-parse` and canonicalizes
  - each agent command resolves on the app's own `PATH`, which is the `PATH` the launcher passes to the agent
  - each agent with `prompt_args` has a template containing `{prompt}`
  - config path writable, or reported read-only because it resolves into `/nix/store`; state directory writable
  - registry readable, with a corrupt file reported on stderr
  - registry entries whose worktree, branch, or tmux session is missing. An unborn branch checked out in the session's worktree counts as present.
  - registry entries whose agent no longer exists in config
  - orphaned worktrees under the worktree root that no registry entry claims
  - orphaned branches matching `branch_prefix` with no worktree
- Exit codes, documented in README and AGENTS.md: 0 success, 1 user or validation error, 2 not found, 3 conflict, including a refused guard, an `Unmanaged` target, a session that is live and healthy, or the lock being held.
- JSON output uses stable lowercase state strings: `working`, `waiting`, `exited`, `stopped`, `unmanaged`.
- `--config <path>` and `$RINGLEADER_CONFIG` override for tests.
- Locking: `new`, `delete`, `rename`, `restart`, `repo add`, and `repo remove` take the write lock. `list`, `doctor`, `attach`, and `repo list` take no lock.

## 8. Config, state, locking

Config is app-owned at `$XDG_CONFIG_HOME/ringleader/config.toml`. First run writes a commented template with `create_new`, so it never overwrites an existing file. The template is the block below as written: `worktree_root` and the `[[repos]]` example stay commented out, so the worktree root defaults to `$XDG_DATA_HOME/ringleader/worktrees` and no example repo is configured. Writes go through `toml_edit` so user comments and formatting survive. The config path is canonicalized before any write. If the canonical path is inside `/nix/store`, report "config is Nix-managed, cannot write" and continue read-only, never fail. Otherwise writes are atomic and go to the canonical target (temp file next to it, then rename), so a symlinked config stays a symlink. An existing config that fails to parse is never rewritten: the TUI starts with defaults plus a status-bar error, and mutating commands exit 1.

```toml
# Defaults to $XDG_DATA_HOME/ringleader/worktrees.
# worktree_root = "~/.local/share/ringleader/worktrees"
branch_prefix = "ringleader/"
tmux_prefix = "ringleader-"
state_poll_ms = 1000
preview_refresh_ms = 750
diff_refresh_ms = 3000
activity_idle_seconds = 3
session_size = "200x50"
bell_on_waiting = false
scan_depth = 3

# Optional. Repos found by scanning the launch directory need no entry here.
# Use [[repos]] only to pin a custom name, a custom base_branch, or a repo
# that should always appear in the picker. Explicit entries win on path collision.
# [[repos]]
# name = "example-project"
# path = "~/projects/example-project"
# base_branch = "main"

# The first agent is the default for new sessions.
[[agents]]
name = "claude"
command = "claude"
env = ["ANTHROPIC_API_KEY"]
prompt_args = ["{prompt}"]

[[agents]]
name = "opencode"
command = "opencode"
env = ["OPENAI_API_KEY"]
prompt_args = ["--prompt", "{prompt}"]
```

- `base_branch` optional. Resolution ladder, each step checked with `git rev-parse --verify -q`: `git symbolic-ref -q refs/remotes/origin/HEAD`, then `refs/heads/main`, then `refs/heads/master`. When `git rev-parse --verify -q HEAD` fails because HEAD is unborn and no base resolves, creation runs `git worktree add --orphan -b <branch> <path>`. Otherwise a clear error asking for an explicit `base_branch`. The resolved commit SHA becomes the session's base commit. Worktrees are always created with `git worktree add --no-track -b <branch> <path> <start>`. Local refs only, no fetch ever. Resolution is lazy, only for the repo the user actually picks.
- `state_poll_ms` drives observation and classification. The spinner advances once per tick (section 6). `preview_refresh_ms` runs the preview on its own timer, faster than the state poll by default, because it captures a single pane. `diff_refresh_ms` is the slow timer, and diffs are computed for visible rows only.
- Repo scanning: scope resolves from the launch directory by walking up to the nearest `.git` directory, or the launch directory itself when there is none. When the launch directory is inside a repo but is not that repo's top level, the bounded walk also runs from the launch directory, and its results merge with the enclosing repo. Without that, an ancestor repo such as a dotfiles repo at `$HOME` would hide every repo below the launch directory. The same rule applies when the user changes the scope in the picker. If the walk-up meets a `.git` file at a linked worktree, it resolves the worktree to its main repository (`git rev-parse --git-common-dir`) and uses that repo as the scope, so launching from a managed worktree or a `t` shell still finds the repo. The walk is depth-limited by `scan_depth` (default 3), prunes `node_modules`, `target`, `vendor`, `.cache`, and `dist`, never enters `/proc`, `/sys`, `/dev`, or `/nix/store`, treats a `.git` directory as a repo and stops descending, skips `.git` files (worktrees and submodules), does not follow symlinks, and ignores permission errors. It stops after visiting 5,000 directories and shows a status-bar notice that the scan was cut short. Results are canonical paths, deduped, and cached in memory per picker session; `r` re-scans.
- `env` lists variable names. They add to a fixed baseline and never replace it. The baseline is `PATH`, `HOME`, `USER`, `LOGNAME`, `SHELL`, `LANG`, `LC_*`, `TZ`, `TMPDIR`, `XDG_*`, `SSH_AUTH_SOCK`, `TERMINFO`, `TERMINFO_DIRS`, `LOCALE_ARCHIVE`, `DISPLAY`, `WAYLAND_DISPLAY`, `DBUS_SESSION_BUS_ADDRESS`, `BROWSER`, `EDITOR`, `VISUAL`, `HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY`, and `NO_PROXY` with their lowercase forms, `SSL_CERT_FILE`, `SSL_CERT_DIR`, `NIX_SSL_CERT_FILE`, `NODE_EXTRA_CA_CERTS`, and `GNUPGHOME`. Without the display, browser, proxy, and CA names, agents lose browser login, clipboard access, and API access behind a proxy or a TLS-inspecting CA. `GPG_TTY` is never copied, since it would name the wrong terminal. The launcher sets it to its own. At session creation and on restart, the tmux adapter reads the values of the baseline and the configured names from the injected `ProcessEnvironment` snapshot and writes them, with the agent argv and the worktree path, into the handoff file through `AgentHandoff`. Values never pass through domain or application. Missing variables produce a warning, not a failure. Values are never logged.
- Handoff files live in `$XDG_RUNTIME_DIR/ringleader/handoff/`, or `$XDG_STATE_HOME/ringleader/handoff/` when there is no runtime directory. The directory is mode 0700, and each file is created with `create_new` and mode 0600. The launcher deletes its file before exec. Startup deletes handoff files older than 60 seconds left behind by a failed launch.
- `prompt_args` is the argument template for an initial prompt, with `{prompt}` substituted as one argv element. Claude Code uses `["{prompt}"]` (positional), opencode uses `["--prompt", "{prompt}"]`. Both were verified against the installed versions (Claude Code positional prompt; opencode 1.18.30 `--prompt`). A prompt containing multiple words, quotes, or a trailing `;` stays one argv element, never split or shell-evaluated, because the argv travels in the handoff file and never on a tmux command line. When the new-session prompt field is empty, no prompt arguments are appended, regardless of the template. An agent without `prompt_args` cannot receive an initial prompt and the UI says so.
- `~` in `worktree_root` and `Repo.path` expands to the home directory at load. `TomlConfigSource` then canonicalizes repo paths; when canonicalization fails it keeps the expanded path and `doctor` reports it. Resolution order for the config file: `--config` flag, then `$RINGLEADER_CONFIG`, then `$XDG_CONFIG_HOME/ringleader/config.toml` (falling back to `~/.config`). The first match wins, no merging.
- Config validation on load: `tmux_prefix` must match `^[a-z0-9-]+-$`, `branch_prefix` must match `^[a-z0-9-]+/$`, repo names use the `RepoName` alphabet, repo and agent names are unique, worktree root writable, or, when it does not exist yet, its nearest existing ancestor writable, since `git worktree add` creates the missing directories. An agent `env` name must not be a baseline name, which already reaches the agent, or one of `KRB5CCNAME`, `MSYSTEM`, `SSH_ASKPASS`, `SSH_AGENT_PID`, `SSH_CONNECTION`, `WINDOWID`, and `XAUTHORITY`. Those names and several baseline names are in tmux's default `update-environment`. A creating or attaching client that lacks such a name makes tmux mark it removed in that session's environment, so the `t` shell and every later window in the session would lose it. No repo is required, since scanning supplies them. An empty or first-run config is valid and must not block startup; strict validation applies to mutating commands. First run seeds the template with the two default agents, `claude` and `opencode`, so `n` works immediately.
- Registry at `$XDG_STATE_HOME/ringleader/sessions.json` with a schema version field. Writes are atomic: write a temp file in the same directory, `sync_all` it, rename it over `sessions.json`, then `sync_all` the directory. Schema policy: refuse a newer version than the binary with a clear message. A corrupt file is moved aside as `sessions.json.corrupt-<timestamp>` only by a lock holder, which then continues with an empty registry plus a status-bar warning. `list` and `doctor` report a corrupt registry on stderr, exit 1, and change nothing. No migration machinery in v1, since there is no older schema yet.
- The registry is an index, not the source of truth. tmux user options are the source of truth for a live session:
  - `@ringleader_meta` holds one compact JSON object with repo name, repo path, branch, base commit, worktree path, and agent.
  - `create_session` writes it, and `rename_session` updates it in the same invocation as the tmux rename.
- `observe_all` makes one call: `list-panes -a -F` with `#{session_name}`, `#{socket_path}`, `#{session_created}`, `#{window_active_clients}`, `#{pane_id}`, `#{@ringleader_agent}`, `#{pane_dead}`, `#{window_activity}`, and `#{@ringleader_meta}`, tab-separated, with `-u` so the tabs survive any locale. Session user options expand on every pane row; this was verified on tmux 3.6a. A pane user option expands only on its own pane's row. `observe_all` keeps the row of the tagged agent pane for each session. A session with no tagged pane keeps one row for its metadata and is observed as described in section 4. `window_activity` is epoch seconds with 1-second granularity. `pane_last_output_time` is unavailable on tmux 3.6a; `pane_activity` is not a format variable.
- Startup reconciliation merges registry entries and live tmux sessions matching `tmux_prefix`:
  - live session, valid metadata, no registry entry: adopt into the registry. The adopted name is the tmux session name with `tmux_prefix` stripped, `created_at` is `#{session_created}`, and the socket path is the observed `#{socket_path}`.
  - metadata is valid only if it parses, `branch` starts with `branch_prefix`, and `worktree_path` passes the containment check of `delete_session` step 3, which a missing worktree directory still passes. Anything else is missing or malformed.
  - a live session whose valid metadata claims the worktree path of exactly one registry entry under a different name takes over that entry, but only when no live session carries that entry's own tmux session name and no other live session claims the same worktree path. Otherwise the claiming session is `Unmanaged`. The next persisting reconcile re-keys a taken-over entry to the live name and updates it from the metadata, including the observed socket path. This repairs a rename whose registry write failed after the tmux rename, and a registry entry is never shown as `Stopped` while a live managed session claims its worktree path.
  - otherwise a live session is adopted only when its stripped name is a valid slug that no registry entry uses, and no other registry entry claims its worktree path or branch. A registered live session is checked against the other entries only, never against its own. Creation's availability rules do not apply, since an adoptable session's own name, worktree, and branch always exist.
  - live session with missing or malformed metadata, or one that fails these checks: state `Unmanaged`, attachable only, never edited, restarted, or deleted; warned about in the status bar
  - registry entry with no live session: keep, shown as `Stopped`, restartable in place
  - each registry entry records its server's socket path. A session observed as `Stopped` may still be live on another tmux server, such as a `-L` server, another `TMUX_TMPDIR`, or the other side of inside versus outside tmux. Before restart, rename, or delete acts on a `Stopped` session, it runs `has_session_on` against the recorded socket, and a live session there refuses the action with a conflict that names the socket. It also refuses when any live session on the server, with or without `tmux_prefix`, carries valid metadata that claims the entry's worktree path, such as a managed session the user renamed out of the prefix with tmux. `observe_all` already lists every session.
- `observe_all` treats a missing or empty tmux server as zero sessions, not an adapter error. tmux reports that three ways: `no server running on <path>` for a stale socket file, `error connecting to <path> (No such file or directory)` when the socket file is absent, which is the normal state after a reboot, and `no current target` from `list-panes -a` on a live server with no sessions (`exit-empty off`). Any other failure, including `Permission denied` and `server exited unexpectedly`, is an adapter error. On the zero-session path every registry session is simply `Stopped`.
- Locking with `std::fs::File::try_lock` (stable since Rust 1.89) on `$XDG_STATE_HOME/ringleader/lock`. Two locks on the same file conflict even inside one process, so each process takes the lock exactly once, at its entry point:
  - `main` takes an exclusive lifetime lock before starting the TUI. A second TUI exits with a clear message naming the lock path. The TUI persists under this lock and never takes the write lock.
  - `Cli` takes the write lock for mutating commands (section 7) before loading the registry or config, and holds it until the command exits. If the TUI holds it, the command exits 3 with "another instance is running".
  - Read-only commands such as `list` and `doctor` do not lock and do not write the registry.
  - `SessionService` receives no `Lock`.
- The registry write path is single-writer: the TUI lifetime lock is held while it persists, and CLI mutations persist under the write lock. `list` never persists, eliminating the race between a lock-free read and a writer.
- Dependencies: `ratatui`, `crossterm`, `serde`, `serde_json`, `toml`, `toml_edit`, `thiserror`, `anyhow` (main only), `clap`, `dirs`, `log`, `env_logger`, `ansi-to-tui`, `signal-hook`. Dev: `tempfile`. No mocking crate.

## 9. Logging and error handling

- `log` + `env_logger`, default level info, file at `$XDG_STATE_HOME/ringleader/app.log`, `--log-level` flag. If the file exceeds 5 MB at startup, truncate it. No rotation machinery in v1.
- Adapter errors are wrapped with `thiserror` and surfaced in the status bar. The UI never panics on adapter failure. tmux and git adapter errors carry the subcommand name, exit status, and stderr, never the argument vector.
- Panic hook restores the terminal, releases the lock, and writes the panic to the log. Terminal state, the return binding, and the lock live in RAII guards so normal exit always restores them. The binding guard runs before the terminal guard and ignores terminal errors.
- SIGHUP, SIGTERM, and SIGQUIT are registered as flags with `signal-hook`. The event loop checks them every tick and returns normally, so the guards run. SIGHUP is the usual way the TUI ends when its tmux pane or terminal window closes. Mutating CLI commands register the same flags for their whole run, so a closed terminal never stops one between two steps of a use case. They write to stdout and stderr with `writeln!` and ignore write errors, because `println!` and `eprintln!` panic once the terminal is gone. Every git and tmux child that does not take over the terminal starts in its own process group with `CommandExt::process_group(0)`. A closed terminal sends SIGHUP to the whole foreground process group, and the flags protect only Ringleader's own process, so a child killed mid-command would still stop the use case between two steps.

## 10. Testing and fixtures

- Domain: classifier boundaries (missing, dead, malformed metadata as `Unmanaged`, activity exactly at threshold, stale, fresh), slugging including the 64-char cap, the alphabet mapping, an input that slugs to empty, and slug collisions, `RepoName` slugging, diff accumulation including a binary file, and the `SessionState` bell edge predicate (`Working` to `Waiting` only).
- Application: hand-written `FakeSessionRegistry`, `FakeWorkspace`, `FakeMultiplexer`, `FakeReturnBinding`, `FakeConfigSource`, `FakeRepoScanner`, `FakeHostProbe`, `FakeLock` implementing the ports, shared from `src/test_support/`. Create, duplicate rejection on the slug, on an existing worktree directory, and on an existing branch, branch cleanup when worktree creation fails, default agent selection, dirty delete guard, unmerged-commit delete guard on a clean worktree, `Working` delete guard, a guard that appears after confirmation refusing the delete, delete refusing a worktree path outside the root, a failed worktree removal keeping the branch and registry entry, rename with branch, rename rollback, restart for `Exited` and `Stopped`, restart of a session with no tagged agent pane, restart refusal for live and unmanaged, doctor findings, CLI service paths. Prompt expansion: template with and without prompt, multi-word prompt kept as one argv element, missing `prompt_args` refused, restart never replays the prompt. The `SessionSpec` carries environment names and never values. Reconciliation: adoption of an unknown live session with name and `created_at` derived from tmux, malformed metadata becomes `Unmanaged` and is never persisted as managed, metadata pointing outside `worktree_root` or at a branch without `branch_prefix` becomes `Unmanaged`, registry-only entries stay `Stopped`, adoption respects uniqueness. `open_shell` returns an attach for live sessions and an ephemeral request for stopped ones, and is refused for unmanaged sessions. `attach` from the CLI never calls `FakeReturnBinding`. Discovery: scope from a repo subdirectory, scope from a plain directory, explicit overrides winning on canonical path, scanned and explicit entries deduped, sessions never filtered by scope. Lifecycle edge cases: deleting an orphan session with no commits returns no SHA, a delete retried after the branch is gone completes, a delete of a session whose worktree directory is missing completes, a delete is refused when `caller_session` is the target, a delete that git would refuse (a locked worktree) leaves the session running, a `Stopped` session is renamed without tmux, a rename whose registry write failed is repaired by re-keying, a create whose tmux step fails kills the session before removing the worktree, a `Stopped` session live on another socket refuses restart and delete, a registered live session is never `Unmanaged` because of its own entry, a session with a missing worktree is still adopted, a live session that claims the worktree path of an entry whose own name is live stays `Unmanaged`, and a `Stopped` entry whose worktree path a renamed-out live session claims refuses restart and delete. Also tested: the busy message drawn before a blocking operation, `doctor` reporting a server version that differs from the client's, a mutating CLI command finishing its use case after a raised SIGHUP flag, restart of a `Stopped` session with a missing worktree failing before tmux runs, and rename refused for a `Stopped` session live on another socket.
- Adapters with fixtures under `tests/fixtures/`, fake data only:
  - `config.toml`, `sessions.json`
  - `tmux-list-panes.txt` for the `observe_all` parser and reconciliation, including a dead agent pane next to a live shell pane in the same session, a split agent window where only the agent pane carries the pane tag, `window_active_clients` values, a `socket_path` column, a `window_activity` value, a valid metadata record, a malformed one, and a session with no tagged agent pane
  - `tmux-list-windows-shell.txt` for `ensure_shell_window`, including an existing `@ringleader_shell` window and a renamed shell window
  - `tmux-list-clients.txt` for `ring_bell`, including a control-mode client with an empty `#{client_tty}`
  - `tmux-bindings.txt` for saving and restoring the previous root C-q binding: full `list-keys` listings from a 3.6 and a 3.7 server with a user C-q binding, with a `-r` C-q binding, with no C-q binding, with C-q bound only in other tables, and with a brace binding as a 3.6 server lists it. Both versions pad the full listing with runs of spaces between tokens.
  - `tmux-capture-pane-ansi.txt` for the ANSI preview fallback, with trailing blank rows and colored lines wider than the panel, for trimming, bottom anchoring, and clipping
  - `git-numstat.txt`, including a binary `-\t-` row, and `git-untracked.txt` for diff parsing
- Git adapter tests run real git in `tempfile` repos, hermetically. Every git command in tests runs with `GIT_CONFIG_GLOBAL=/dev/null`, `GIT_CONFIG_NOSYSTEM=1`, `HOME=<tempdir>`, and `GIT_AUTHOR_NAME`, `GIT_AUTHOR_EMAIL`, `GIT_COMMITTER_NAME`, `GIT_COMMITTER_EMAIL` set on the `Command`, never through `std::env::set_var`. `GitWorkspace` takes this environment override through its constructor; production passes none. Fixture repos are created with `git init -b <branch>`, never relying on `init.defaultBranch`. Config, state, and data paths are injected, never resolved with `dirs` inside tests. Git tests cover: a cloned repo whose new branch has no upstream, the orphan path on an unborn HEAD, `unmerged_commits` returning 0 once another branch points at the tip, `remove_worktree` refusing a main worktree, a path outside the root, and a plain directory with nothing deleted, a cross-check of sample slugs against `git check-ref-format --branch <branch_prefix><slug>`, `unmerged_commits` counting 1 for a commit that exists only on a branch checked out in its own linked worktree, 1 for a commit on that worktree's detached HEAD, and 0 once a remote-tracking copy exists, an orphan branch reported as unborn before its first commit, `unmerged_commits` returning 0 for a missing branch that no worktree names, `unmerged_commits` counting a detached commit in a worktree whose directory was deleted, `diff_stat` leaving the worktree index's mtime unchanged on a stat-dirty tree, `dirty_state` still counting untracked files under a fixture global config that sets `status.showUntrackedFiles=no`, a failing post-checkout hook leaving no worktree and no branch after rollback, and repo validation accepting a plain repository and one of its subdirectories, and rejecting a linked worktree and a submodule.
- tmux tests detect the binary and skip when absent. Each tmux test starts its own server with `-S <tempdir>/tmux.sock -f /dev/null`, so tests never share a server, never load the user's tmux config, and never touch the user's default server. `TmuxMultiplexer` and `TmuxReturnBinding` take the socket and config arguments through their constructors (via `TmuxCommand`); production passes none. The inside-tmux flag and `TMUX_PANE` are injected, never read from the developer's environment. A Drop guard runs `kill-server` on the test socket. CI and the Nix `checkPhase` use the same setup. tmux tests cover: exact targets with `ringleader-fix` absent and `ringleader-fix-auth` present, arguments ending in `;` (`fix bug;`, `a\;`, `;`, `x ; y`) arriving byte for byte with an argument after them, `@ringleader_meta` expansion in `list-panes`, respawn of the tagged agent pane while a shell window is current, Ctrl-Q binding install and restore including two installs followed by a restore, a restore after a server restart leaving the new server untouched, a switch between two managed sessions before C-q, a secret check where a fixture secret under a configured agent `env` name sits in the injected snapshot and, after Ringleader starts the server, is absent from the server's `show-environment -g`, `/proc/<pid>/environ`, and `/proc/<pid>/cmdline`, `list-panes` with `LC_ALL=C` in the injected snapshot still returning tabs and UTF-8 metadata byte for byte, the three zero-session cases (absent socket file, stale socket, and an `exit-empty off` server with no sessions), a creation sequence whose later command fails leaving no session, a recreated agent window started from another directory landing in the worktree, `attach` after `open_shell` landing on the agent pane, `open_shell` after `attach` landing on the shell window, a creation under a global `after-new-session` hook that splits the window still tagging the launcher's pane, Ctrl-Q binding install and restore on a server whose root table holds only C-q, no binding installed when `TMUX_PANE` is absent, a stale `TMUX_PANE` aborting the install, and Ctrl-Q with the TUI pane inside a managed session. Tests that need a client hold a control-mode client (`tmux -C attach-session -t =<session>:` with stdin kept open by the test), press keys with `send-keys -K -c <client_name> C-q`, and read the result with `list-clients -F '#{client_name} #{client_session} #{pane_id}'`. The pane id is what proves the switch when the TUI pane sits in a window of the managed session the client is already in, since `client_session` does not change there. The test snapshot carries no `TMUX` or `TMUX_PANE`, so tests run from inside tmux never reach the developer's server.
- The launcher command prefix is a constructor dependency of `TmuxMultiplexer`. `main` passes `[current_exe, "__exec-agent"]`. tmux tests pass a harmless stand-in such as `["sh", "-c", "sleep 30", "--"]`, so panes stay alive and the test harness is never run inside a pane.
- `AgentLauncher` takes an exec seam, a support trait in its file, through its constructor. Production passes one that calls `CommandExt::exec`. Tests pass one that spawns the prepared command with `env` as the agent argv and captures its output, because a unit test in a binary crate cannot exec, has no `CARGO_BIN_EXE_ringleader`, and gets the test harness from `current_exe`. Launcher tests: the exec'd environment contains the baseline, the configured names, the kept pane variables, and `GPG_TTY` and nothing else, a value present only in the inherited environment for an allowlisted name does not reach the agent, the handoff file is deleted before exec, and a missing worktree path fails before exec.
- Repo scanner tests build a `tempfile` directory tree with fixture repos: nested repos, a `.git` file worktree that must be skipped, a pruned directory (`node_modules`), a depth-4 repo that must be excluded at depth 3, a regular directory that is not a repo, a launch from inside a linked worktree that resolves to its main repo, a launch from a plain directory inside an ancestor repo that finds both the ancestor and the repos below, and a tree larger than the visit cap (with the cap injected small). Scanner tests do not touch the real filesystem outside tempdirs.
- Config tests: a symlinked config stays a symlink after a write, an unparseable config is never rewritten, and the first-run template never overwrites an existing file.
- Registry and lock tests: a corrupt registry is moved aside only by a lock holder, and two `FileLock` instances on the same path in one process conflict.
- All tests are unit tests inside `src/` (`#[cfg(test)] mod tests`, or `src/test_support/`). `tests/` holds fixtures only. Do not add `tests/*.rs` integration tests; a single binary crate has no library target for them to link. If that changes, a `lib.rs` must be introduced deliberately.
- UI: `TestBackend` render assertions for the spinner glyph, the `●` bullet, the `Unmanaged` marker, each responsive tier at representative sizes, status bar dropping at 60 and 36 columns, the resize redraw, the help overlay, the delete confirmation, the empty state, preview trimming, bottom anchoring, and clipping, a resume after attach requesting a full redraw, no bell for a session whose agent window a client shows, a bell when a client shows only its `t` shell window, `ring_bell` selecting from a `list-clients` fixture the tty of every client while skipping a control-mode client's empty `#{client_tty}`, and `App` never refreshing more than two rows per tick. One `EventLoop` test: a raised signal flag exits through the normal return path.
- One concept per test, public interfaces only.

## 11. Tooling and CI

- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, all through `nix develop` so local and CI use the same rustc 1.95.
- In a binary crate, anything `main` cannot reach yet is dead code, and most of milestones 2 to 28 build ahead of their wiring. Until then, `main.rs` carries `#![allow(dead_code, unused_imports)]`, unconditionally. A `cfg_attr(not(test), ...)` form does not work, because `--all-targets` also compiles the binary under `cfg(test)`, where unused re-exports and unwired port traits would fail `-D warnings`. Milestone 29 removes it, and `cargo clippy --all-targets -- -D warnings` must pass without it, which it does once everything is reachable from `main`.
- `.github/workflows/ci.yml` on Ubuntu: DeterminateSystems nix installer, then `nix develop -c cargo fmt --check`, `nix develop -c cargo clippy ...`, `nix develop -c cargo test`, and `nix build .#` plus `nix flake check`. No separate toolchain action, the dev shell is the single source of truth.
- The GitHub repository exists, so the workflow runs on GitHub as soon as it is pushed.

## 12. Flake for this repo

- Input `nixpkgs` pinned at `nixos-26.05` for stability.
- `packages.default = rustPlatform.buildRustPackage` with `cargoLock.lockFile`, wrapped with `makeWrapper --prefix PATH : ${lib.makeBinPath [ tmux git ]}` so `nix run` works standalone.
- `doCheck = true` with `nativeCheckInputs = [ git tmux ]`, so `nix build` runs the test suite, including the tmux tests on per-test sockets. The sandbox has no usable `HOME` and no git identity, which the hermetic git environment in section 10 provides.
- `devShells.default`: rustc, cargo, clippy, rustfmt, tmux, git, nixfmt.
- `formatter = nixfmt-tree`, `checks` build the package. Commit `flake.lock`.

## 13. Consumer integration

Out of scope for this repo. The flake's `packages.default` and `devShells.default` are the interface. Any Nix consumer can add the flake as an input and install the binary. Integration work happens outside this repository, so nothing here may reference a specific downstream setup or configuration.

## 14. Documentation split

- `README.md`: for humans. Terse and minimal. What it is, install (`nix run`, `nix profile install`, add as a flake input), first run, key bindings table, CLI synopsis. Generic examples only. No architecture, no agent instructions.
- `AGENTS.md`: the agent repo documentation and the living reference. It must embed a copy of section 3's tree, because it has to be self-contained; `docs/plans/v1-bootstrap.md` is the historical plan and is not a substitute. Layout, the one-primary-type-per-file rule and the fact that the tree is the file contract, port rules, OOP conventions (private fields, `impl` blocks, constructor injection, no free functions), the strict comment policy (comments are failures, fix the code, justify any survivor to the owner before committing, never in the commit body), naming rules, CLI and TUI wiring boundaries, prose capitalization, exact commands (`nix develop`, cargo test/clippy/fmt), test strategy and fixtures, commit conventions, the push-on-request rule, the plan storage convention below, and the rule that README stays terse and human-facing.
- Plan storage convention for future work: one file per effort at `docs/plans/YYYY-MM-DD-slug.md`, with a `> Status:` header (`active`, `completed`, or `abandoned`). `docs/plans/v1-bootstrap.md` is the v1 history.

## 15. Milestones, one commit each

1. Scaffold cargo project, rustfmt, MSRV confirmed against the dev shell, flake dev shell with `doCheck`, CI skeleton, and the temporary dead-code allowance from section 11.
2. Add domain session model, slug rules, and state classifier with tests.
3. Add application ports and fakes.
4. Add the `SessionService` facade and the create, delete, rename, observe, and attach use cases with tests.
5. Add git worktree adapter with hermetic tempfile tests, base branch ladder, `--no-track` and orphan creation, unmerged-commit check, and guarded removal.
6. Add tmux adapter on per-test private sockets, with `-u`, the `ProcessEnvironment` snapshot minus agent `env` names, exact targets, agent pane tagging, `;` escaping, and the zero-session cases, and validate the Ctrl-Q binding forms from section 6 against the pinned tmux. Also check binding save and restore once by hand on a tmux 3.7 server from `nix shell nixpkgs/nixos-unstable#tmux`, after `tmux -V` confirms a 3.7 release, under the ad-hoc tmux rule. Automated tests cover 3.7 only through the `tmux-bindings.txt` fixture.
7. Add JSON registry with atomic synced writes, corruption recovery, schema policy, and single-instance locking.
8. Add the repo scanner adapter with tempfile tree tests, cwd scope resolution, linked-worktree resolution, the visit cap, and explicit override merging.
9. Add a thin vertical slice: minimal session table, attach, terminal guard, signal handling, and `q` quit.
10. Add tmux metadata mirroring, reconciliation with re-keying, adoption with metadata validation, and the recorded socket path with the `has_session_on` check.
11. Add the `__exec-agent` launcher, the handoff file with the baseline environment, and `prompt_args` expansion.
12. Add `Exited` and `Stopped` restart use cases.
13. Add config loader with validation, first-run template, and atomic `toml_edit` writes.
14. Add CLI subcommands list, new, delete, attach, rename, and restart, plus exit codes, write locking, and JSON state strings.
15. Add CLI `repo` and `doctor` subcommands with the host probe adapter.
16. Render working spinner and waiting bullet.
17. Add responsive tiers, resize redraw, status bar priority dropping, column dropping, sort order, name-based selection with arrow and `h`/`j`/`k`/`l` navigation, and the home empty state.
18. Add TUI `s` restart and `r` reload keys.
19. Add the `t` shell action, live via the `@ringleader_shell` window and stopped via ephemeral `$SHELL`.
20. Add new session dialog with repo picker, scope header, agent picker, initial prompt field, and empty picker state.
21. Add "Add repo" row with `git rev-parse` validation.
22. Add delete confirmation with dirty, unmerged-commit, and `Working` guards.
23. Wire the Ctrl-Q return binding into TUI attach, with idempotent install, saved-binding restore, and startup repair.
24. Add `R` rename dialog and rename flow with branch rename and rollback.
25. Refine live preview with `ansi-to-tui`, trimming, bottom anchoring, and clipping, plus round-robin visible-row diff computation.
26. Add logging, status bar error surfacing, and the busy message drawn before blocking operations.
27. Add bell on Working to Waiting transition.
28. Add in-TUI help dialog.
29. Remove the dead-code allowance, then add README and AGENTS.md, with the tree copied into AGENTS.md.
30. Finish GitHub Actions.

## 16. Risks to verify early

- Worktree removal never deletes recursively. `git worktree remove --force` after the guards handles submodule worktrees; anything git refuses (a main worktree, a path that is not a worktree, a locked worktree) is reported and left alone.
- Ctrl-Q root binding conflicts with the user's tmux config. Restore correctness on crash is handled by the saved-binding record keyed by server plus startup repair; `key-table` is rejected and documented as such.
- tmux behavior follows the server's version, which can differ from the pinned client. A 3.7 server prints nothing for `list-keys -T root -1 C-q`, bound or not, and nothing for `list-keys -T root` when the root table holds one binding, so the saved binding comes from the full `list-keys` listing.
- The inside-tmux attach path uses `switch-client`, never `attach-session` from a child process.
- Bare tmux targets prefix-match other sessions. Every target is `=<session>:`, a `window_id`, or a `pane_id`.
- The return binding is installed before each attach rather than at startup, because a server with no sessions exits.
- `window_activity` has 1-second granularity, so `activity_idle_seconds` below 1 is meaningless.
- Detached session sizing (`-x/-y`) holds only until the first attach, after which `window-size latest` keeps the last client's size. The preview crops whatever size it captures.
- Terminal raw mode and alternate screen restore around `tmux attach`, including the full redraw on resume, moved early as the milestone 9 vertical slice.
- Resize events must force a redraw so tiers and the too-small guard recalculate instantly.
- Unborn-HEAD repos use `git worktree add --orphan` instead of erroring on the base-branch ladder; this needs git 2.42.
- Initial prompt argv must stay one element per template entry; a prompt with quotes, shell metacharacters, or a trailing `;` must reach the agent intact, which the handoff file guarantees. Both Claude Code and opencode were verified to accept it, but agents change their flags; `doctor` should warn when a configured `prompt_args` template lacks `{prompt}`.
- The launcher's cleared environment must keep the baseline, or agents fail to resolve, lose their auth, and render without `TERM`.
- Branch rename rollback ordering in `rename_session`.
- Dirty worktree removal, unmerged commits, and branch deletion safety, including the explicit confirmation for a `Working` session and re-checking the guards at execution time.
- Activity heuristic false positives if an agent paints an idle animation, tunable via `activity_idle_seconds`. The user's own typing also counts as activity, so a session the user is typing into can read as `Working`. The bell skips a session whose agent window a client shows for that reason. Output in a pane the user splits off the agent window also counts, since `window_activity` covers the whole window.
- `ansi-to-tui` parsing of capture output. Always keep the plain text fallback.
- Preview and diff load: only the selected session is captured, which is one cheap tmux call even on the fast preview timer. Diffs are computed for visible rows only, on the slower `diff_refresh_ms` timer. Git work per tick is bounded. If round-robin refresh still stalls input on large repos, move git work to one `std::thread` worker over a channel, still without an async runtime.
- Lock contention between a running TUI and CLI mutations, make the message actionable, exit 3.
- User option expansion in `list-panes -F` was verified working; keep the per-session `show-options -qv` fallback as a defensive path, and check session existence with `has-session`, since `show-options -qv` also exits 0 with empty output for a missing session.
- Adoption when the worktree was deleted manually: adopt the session, surface the missing worktree in `doctor` and the status bar.
- Global `destroy-unattached` in the user's tmux config is why creation and configuration happen in a single tmux invocation.
- tmux server environment drift: `-e` does not strip the inherited server environment, so the `__exec-agent` launcher owns the real allowlist, and values travel only in the handoff file. The reverse leak, a new server copying its first client's environment, is handled by removing the configured agent `env` names from every tmux child.
- tmux replaces tabs and non-ASCII bytes in its output with `_` when the client locale is not UTF-8. `-u` on every invocation prevents it, and the Nix build sandbox, which has no `LANG`, exercises that path.
- `git rev-list --all` includes the HEAD of every worktree, which would hide every session's commits from the unmerged-commit guard.
- Porcelain `git diff` rewrites a worktree's index under `index.lock` even with `GIT_OPTIONAL_LOCKS=0`, and `git status` does without it. Both would collide with the agent's own git commands, so DiffStat uses `diff-index` and every read runs with `GIT_OPTIONAL_LOCKS=0`.
- Registry `list` race: `list` reconciles in memory and never persists, single-writer elsewhere.
- Repo scan scope: launched from `$HOME` or `/` the walk could be large. Mitigated by the depth cap, pruning, the skipped system directories, the 5,000-directory cap with a status-bar notice, and the empty state.
- Repo name collisions between scanned and explicit entries, resolved by canonical path with explicit winning; names must still be unique for the picker display.
- Agent command resolution under Nix: the wrapped PATH reaches the agent through the baseline, and absolute commands in config also work.

## 17. Out of scope for v1

- Mouse support.
- Configurable keybindings.
- `doctor --fix` auto-repair.
- Registry schema migration machinery; only refuse-newer and corrupt-file backup.
- Log rotation; truncate on startup instead.
- The per-session `key-table` Ctrl-Q alternative; rejected after testing on 3.6a.
- A recursive-delete fallback for worktrees git refuses to remove.
- Agent-specific activity detectors beyond tmux pane activity.
- Session output logging or transcript search, agent tooling owns that.
