# Plan: Ringleader, a tmux session manager for Claude and opencode

> Status: active. Flip to `completed` when v1 ships. This file is the historical plan, not living documentation; `AGENTS.md` is the living documentation.

## 0. Ground rules for the implementing agent

- Load the clean-code skill before writing any code.
- Comments are treated as failures to express intent in code. Every comment is a signal, not documentation: revisit the code it explains and improve it until the comment is unnecessary, then delete the comment. Rename the variable, extract the function, introduce the type, or restructure the control flow that made the explanation feel needed. Never add a comment to justify messy code, and never leave a comment as a substitute for a better name. The only surviving comments are the clean-code exceptions: legal headers, non-obvious intent that cannot be encoded in structure, warnings of real consequences, and TODOs that reference a tracked issue. A surviving comment is never announced in a commit body. Instead, the implementing agent stops and justifies the comment to the project owner before committing. If the justification against an exception is not convincing, delete the comment and fix the code instead. This rule outranks convenience; a commit that adds an unjustified comment is not done.
- Write object-oriented Rust: state in structs with private fields, behavior in `impl` blocks, traits as interfaces, composition over inheritance, dependencies injected through constructors. No free functions except `main`.
- One primary type per file, file named after it in snake case. The tree in section 3 is the file contract: every listed file exists and holds its one type. Types absent from the tree are support types (error enums, builders, request structs, iterators, parser results, dialog state) and fold into their owner's file. Unit tests live inline in `#[cfg(test)] mod tests`. `mod.rs` declares and re-exports only. Source files target under 200 lines excluding tests.
- Commit after every milestone item. Capital first letter, title under 72 chars, imperative body.
- No `cargo`, `rustc`, or `tmux` on the host PATH. Build and test inside `nix develop`.
- Do not create a GitHub repository, do not run `gh repo create`, do not add a git remote, and do not push. Work and commit locally only. The project owner creates the GitHub repo manually after reviewing the final codebase.
- Keep the repo consumer-agnostic. Never reference or assume any specific downstream setup, local directory layout, or personal configuration. Examples and fixtures use generic names such as `example-project`.
- Human-facing prose uses "Ringleader". Identifiers stay lowercase: binary, crate, repo, flake input, config paths, tmux and branch prefixes, JSON fields.
- This file is the source of truth. If something here conflicts with an instruction received later, stop and ask.

## 1. Naming

| Item | Value |
|---|---|
| Repo / directory | the repository root |
| Remote | `github:KarlHarnois/ringleader` (identity only, no remote configured until the owner creates it) |
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
- Object-oriented design throughout, in Rust terms: state lives in structs with private fields, behavior in `impl` blocks, traits define the ports (interfaces), composition over inheritance. Constructors (`new`) and dependency injection through function arguments. No free functions except `main`. No `pub` data fields, access is through methods. This applies to domain, application, adapters, and UI equally.
- One primary type per file, named after it in snake case. The tree in section 3 is the file contract, and every listed file holds its type. Types not in the tree are support types that fold into their owner's file. Unit tests live inline in `#[cfg(test)] mod tests`. `mod.rs` files declare and re-export only. Source files target under 200 lines excluding tests; when one outgrows that, extract its largest behavior-bearing type into its own file and update section 3.
- Repo discovery is contextual. The picker scans the directory the app was launched from: if cwd is inside a git repo, that repo; otherwise a bounded walk of cwd. Explicit `[[repos]]` entries remain as overrides for custom names, custom `base_branch`, and repos that must always be visible. Explicit entries win on path collision.
- The session list is never filtered by scan scope. Existing sessions always show regardless of cwd.
- The scan walk is bounded by `scan_depth` (default 3), pruning `node_modules`, `target`, `vendor`, `.cache`, and `dist`. A `.git` directory marks a repo and stops descent; a `.git` file means a worktree or submodule and is skipped. Symlinks are not followed. Permission errors are ignored. Worktrees and submodules never appear as repos.
- Delete removes the worktree and deletes the branch. It warns and double-confirms when the worktree is dirty or the branch has commits that exist on no other ref, and it requires explicit confirmation while the agent is `Working`.
- Worktree removal uses `git worktree remove --force` only. There is no recursive-delete fallback. When git refuses, Ringleader reports git's stderr and removes nothing.
- Rename renames the tmux session, the registry entry, and the git branch by default. `--keep-branch` and a dialog toggle opt out. The worktree directory name does not change.
- Colors use ANSI named colors and modifiers only. Base style `Color::Reset`. Selection `Modifier::REVERSED`. Diff green/red. No RGB anywhere.
- Worktrees live under `$XDG_DATA_HOME/ringleader/worktrees/<repo>/<slug>`, outside the repos.
- The app owns its config file. No config file is managed by Nix.
- MSRV 1.95, edition 2024, matching the rustc shipped in nixpkgs 26.05 (rustc 1.95.0, verified). Milestone 1 confirms this against the dev shell before any code is written.
- Session state is observed from tmux, not inferred from agent internals. A pure classifier turns observations into states. Activity comes from `window_activity` of the agent window, an epoch-seconds format variable that updates within 1 second of pane output. `pane_last_output_time` is not available in tmux 3.6a.
- The agent window is tagged with the window user option `@ringleader_agent 1` at creation. Observation, preview capture, and respawn use only the tagged window's pane, addressed by its `pane_id`. Other windows in a managed session, including the `t` shell window and windows the user opens, are ignored.
- Every tmux target that Ringleader builds is exact: `=<session>:` for sessions and windows, a `pane_id` such as `%3` for panes. A bare `-t <name>` prefix-matches other sessions on tmux 3.6a (`kill-session -t ringleader-fix` kills `ringleader-fix-auth`), so it is never used. Milestone 6 confirms the exact form per command on the pinned tmux.
- Quitting with `q` exits the TUI only. Managed tmux sessions keep running in the background and are rediscovered on the next launch.
- Session metadata is mirrored into tmux session user options so a live managed session can be adopted back after a restart or registry loss.
- Managed tmux sessions are created and configured in a single tmux invocation (`new-session ... ; set-option ...`). Setting options in a second call can destroy the session when the user's global `destroy-unattached` is on. The command separator is a standalone `;` argv element, never a shell-escaped `\;` from Rust. tmux treats any argv element ending in `;` as a separator, so every other dynamic argument in a tmux invocation (paths, names, option values) that ends in `;` is escaped by inserting a backslash before the final `;`.
- Managed tmux sessions set `destroy-unattached off`, so detaching, including via Ctrl-Q, never destroys them, even if the user's global config enables it.
- Managed tmux sessions set `remain-on-exit on` on the agent window (`set-option -w -t =<session>: remain-on-exit on`), so an agent exit leaves a dead pane that `restart` can revive. `remain-on-exit` is a window and pane option, not a session option.
- Detached sessions get an explicit size (`-x 200 -y 50` by default, configurable) so previews are not limited to 80x24.
- Agents launch through a hidden `ringleader __exec-agent --handoff <path>` subcommand. Environment values and the agent argv never appear on a tmux command line. The creating process writes a handoff file holding the agent argv and the environment values, and tmux runs only the launcher with the file's path. The launcher reads the file, deletes it, runs `env_clear`, sets exactly the environment from the file, and execs the agent. That environment is a fixed baseline (section 8) plus the agent's configured `env` names, with values taken from the creating process only. The launcher also keeps `TERM`, `COLORTERM`, `TMUX`, and `TMUX_PANE` from its own pane environment. A name the creating process lacks is absent for the agent even if the tmux server's environment has it, so server environment drift never reaches the agent. The launcher never reads `config.toml`. Only variable names are configured and logged, never values.
- Base branch resolution never fetches. It reads local refs only. On a repo with an unborn HEAD and no resolvable base, creation runs `git worktree add --orphan -b <branch> <path>`, which needs git 2.42 or newer. Branches are always created with `--no-track`, so an agent branch never gets an upstream.
- Session names use one canonical slug function for the registry name, tmux session name, worktree directory, and branch, with a 64-character cap and uniqueness checked on the slug. The slug alphabet (section 4) makes every slug a legal tmux name and a legal git ref component by construction, so the domain validates without calling git.
- Attach behavior depends on `$TMUX`, which `main` reads once and injects into the tmux adapters and the `App`. Outside tmux, `tmux attach-session` runs as a child under the terminal guard. Inside tmux, `tmux switch-client` switches the current client and the TUI keeps running. A child process cannot use `attach-session` from inside tmux; it fails with "sessions should be nested with care".
- Only the TUI installs the Ctrl-Q return binding. The CLI never touches tmux key bindings.
- Locks are taken once per process by the entry point, never inside `application`. `main` takes the TUI's lifetime lock before building the `App`, and `Cli` takes the write lock for mutating commands.
- Mouse support is out of scope for v1.
- Terminal bell on `Working` to `Waiting` is config-gated, off by default. It is emitted by writing BEL to the attached client's tty, obtained from tmux, so it reaches the user even when the TUI's pane is hidden. If no client is attached, no bell.
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
    │   ├── agent_launcher.rs     # AgentLauncher, the hidden __exec-agent mode, owns AgentHandoff
    │   ├── directory_repo_scanner.rs # DirectoryRepoScanner
    │   ├── toml_config_source.rs # TomlConfigSource
    │   ├── json_session_registry.rs # JsonSessionRegistry
    │   ├── file_lock.rs          # FileLock
    │   ├── git_workspace.rs      # GitWorkspace
    │   ├── system_host_probe.rs  # SystemHostProbe
    │   ├── tmux_command.rs       # TmuxCommand, socket and config args, exact targets, `;` escaping
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
- OOP in Rust: all state in structs with private fields, all behavior in `impl` blocks, traits as interfaces, composition over inheritance, one primary type per responsibility, dependencies injected through constructors. No public data fields, no free functions except `main`.
- One primary type per file, named after it in snake case. Supporting types owned by the primary type live in the same file, unit tests live inline. `mod.rs` declares and re-exports only. The tree in section 3 is the file contract; do not merge primary types into one file.
- `cli.rs` owns the clap argument structs (`ListArgs`, `NewArgs`, and so on) as support types of `Cli`. They get no files of their own.
- Why the tree splits what it splits: `Session`, `SessionName`, and `SessionState` each get a file because each is used independently across modules, carries behavior or invariants, and changes for its own reasons. Merging files later is mechanical; splitting a file that grew is disruptive, so prefer the separate file. The reverse is also true: do not invent files for types the tree does not list.
- `src/test_support/` is `#[cfg(test)]` only and must not appear in the binary. It exists so unit tests across modules share one set of hand-written doubles.
- Other support types, such as a parser result, a doctor finding, a session spec, or a dialog's in-progress state, live in their primary type's file and get no tree entry.
- No code below `main` calls `std::env::current_exe`, reads `$TMUX`, `$TMUX_PANE`, or the process cwd, or resolves XDG paths. `main` reads them once and injects them.
- Comments: the repository is comment-hostile. Every comment is presumed unnecessary until proven otherwise. The fix for "this needs explaining" is better code: rename, extract, type, restructure. When a comment seems to survive, stop and justify it to the project owner before committing; never announce comments in the commit body. Doc-comments are comments and follow the same rule; do not document what a well-named item already says.
- Human-facing prose uses "Ringleader". Identifiers stay lowercase.
- Do not create a remote or push.

## 4. Domain

- `SessionName`: one canonical slug function used for registry name, tmux session name, worktree directory, and branch. The slug lowercases the input, maps every character outside `[a-z0-9-]` to `-`, collapses runs of `-`, trims `-` at both ends, truncates to 64 characters, and trims again. An empty result is a validation error (exit 1). The alphabet makes every slug a legal tmux session name (no `:` or `.`) and a legal git ref component, so `SessionName` validates in pure Rust and never calls git. Uniqueness is global across repos and checked on the slug, so two names that slug to the same string are rejected. Availability beyond the registry (worktree directory and branch) is checked by `SessionLifecycle` through the ports.
- `RepoName`: the same alphabet and slug function as `SessionName`. Names derived from a directory basename are slugged. A repo name is a path segment under `worktree_root`, so the alphabet also rules out `/` and `..`.
- `Agent`: name, launch command, environment variable names the launcher restores, and `prompt_args`, the argument template used to pass an initial prompt. The name is the lookup key used by `--agent`, the registry, and `@ringleader_meta`, compared exactly, and is also what the UI displays. Values are never stored in the domain.
- `SessionState`: `Working`, `Waiting`, `Exited`, `Stopped`, `Unmanaged`. `SessionState` owns the bell edge predicate: it is true only for a previous `Working` followed by a current `Waiting`.
- `Session`: name, repo name, repo path, worktree path, branch, base commit, agent, tmux session name, created timestamp (epoch seconds). The base commit is the start-point SHA resolved at creation, empty for an orphan worktree.
- `SessionMetadata { repo_name, repo_path, branch, base_commit, worktree_path, agent }`, the copy mirrored into tmux.
- `SessionObservation { exists, pane_dead, activity_at, created_at, metadata: Option<SessionMetadata> }`, built from the tagged agent window only. A live session with metadata but no tagged agent window is observed with `pane_dead` true.
- `Config`: parsed config.toml, with explicit repo overrides, agents, prefixes, refresh intervals, and the bell flag.
- `RepoScope`: the resolved directory to scan, produced by `RepoScanner`. Support type on `Repo`. The launch directory enters through `main` and is threaded through the service, so neither domain nor application reads the process cwd.
- `SessionClassifier`, pure, takes `now` as an argument, never reads the clock itself. No stored history:
  1. session missing, `Stopped`
  2. no valid metadata (malformed, absent, or rejected by the adoption checks in section 8), `Unmanaged`
  3. `pane_dead`, `Exited`
  4. `now - activity_at < activity_idle_seconds`, `Working`
  5. otherwise, `Waiting`
- `DiffStat { files, insertions, deletions, untracked }`, computed per request, not stored. It measures the worktree against the session's base commit: everything the agent committed plus uncommitted tracked changes. For an orphan worktree the base is the empty tree. `untracked` counts files git would add but does not track yet. A binary file counts as one file with zero insertions and deletions.
- `SessionSummary { session, state, diff: Option<DiffStat> }` is what the service returns to the UI. `diff` is `None` until computed and always `None` for `Unmanaged` sessions.

`Waiting` is the green bullet: the pane is running and quiet, so the agent is at its prompt. `Exited` requires `remain-on-exit on` on the agent window, set at creation, so the pane survives agent exit and `respawn-pane` can restart it.

## 5. Ports and use cases

Ports:

- `SessionRegistry`: `load`, `save`.
- `Workspace`: `create_worktree(repo, path, branch, start_point)`, `remove_worktree`, `prune_worktrees`, `list_worktrees(repo)`, `list_worktree_dirs(root)`, `delete_branch`, `rename_branch`, `branch_exists`, `list_branches(repo, prefix)`, `resolve_start_point(repo, base_branch)`, `dirty_state`, `unmerged_commits(repo, branch)`, `diff_stat(worktree, base_commit)`, `validate_repo`, `version`. `resolve_start_point` returns either a commit SHA or the orphan case.
- `Multiplexer`: `create_session(spec)`, `kill_session`, `observe_all`, `attach(target)`, `capture_pane`, `rename_session`, `respawn_agent`, `ensure_shell_window`, `set_metadata`, `ring_bell`, `version`. `create_session` takes a `SessionSpec` support type (tmux session name, worktree path, size, metadata, agent argv, environment names) and is used both for new sessions and for recreating a `Stopped` session in its existing worktree. `attach` runs `attach-session` or `switch-client` based on the injected inside-tmux flag. `ring_bell` resolves the attached client's tty and writes BEL to it, and does nothing when no client is attached.
- `ReturnBinding`: `install`, `restore`, `repair_stale`. See section 6, Attach and Ctrl-Q.
- `ConfigSource`: `load`, `add_repo`, `remove_repo`.
- `RepoScanner`: `resolve_scope(launch_directory) -> RepoScope` and `scan(scope) -> Vec<Repo>`, returning canonical paths. The nearest-`.git` walk and the bounded directory walk both live in the adapter, keeping the service free of filesystem logic.
- `HostProbe`: `resolve_command(name)`, `is_writable(path)`. Used by `Doctor` and config validation.
- `Lock`: `acquire_lifetime`, `acquire_for_write`. Used by `main` and `Cli` only.

Application structure: `SessionService` is a thin facade that composes `SessionLifecycle`, `SessionMonitor`, `SessionAccess`, `RepoCatalog`, and `Doctor`, each with only the ports it needs. The use cases below are the facade's methods.

Use cases:

- `observe(persist)`: one `observe_all` call, reconcile, classify, no git. Returns summaries with no diffs. The TUI calls it on the `state_poll_ms` cadence with `persist` true, and it writes the registry only when reconciliation changed something. `list` passes false.
- `reconcile`: the step inside `observe` that merges the registry with live tmux sessions. Adopt live sessions matching `tmux_prefix` that carry valid metadata but have no registry entry. Keep registry entries with no live session as `Stopped`.
- `diff_stats(names)`: diffs for the given sessions only. The TUI passes the visible rows on the `diff_refresh_ms` cadence; `list` passes every session.
- `preview(name)`: capture the agent pane of one session, on the `preview_refresh_ms` cadence.
- `reload()`: reload config and registry from disk, used by `r`.
- `discover_repos(launch_directory)`: `RepoScanner.resolve_scope`, then `scan`, then merge explicit config overrides keyed by canonical path with explicit winning, dedupe, and return. Never filters sessions. Cached in memory for the life of the picker; `r` re-scans. The launch directory is passed in from `main`, so the service never reads cwd itself. Changing the scope in the picker calls it again with the new path.
- `list_repos(scope, include_all)`, `add_repo(path, name, base_branch)`, `remove_repo(name)`: back the `n` dialog's "Add repo" row and the `repo` subcommands.
- `create_session(request)`:
  1. Validate the request. Without an agent, use the first `[[agents]]` entry.
  2. Check availability. The slug is unused by registry entries and live tmux sessions, `<worktree_root>/<repo>/<slug>` does not exist and is no registry entry's worktree path, and `<branch_prefix><slug>` does not exist (`branch_exists`). All three are checked before any git call.
  3. Resolve the start point lazily, for this repo only (section 8).
  4. Create the worktree and branch with `--no-track`, or `--orphan` for an unborn HEAD. If `git worktree add` fails after creating the branch, delete that branch.
  5. Create the detached tmux session in the worktree in one invocation. It runs the launcher with a handoff file, tags the agent window `@ringleader_agent`, sets `remain-on-exit on` on that window, sets `destroy-unattached off` on the session, writes `@ringleader_meta`, and applies the `-x/-y` size. If this fails, remove the worktree and branch from step 4.
  6. Persist.
  The agent argv is built by expanding `prompt_args` with the request's prompt. The prompt is not stored in the session or registry. `create_session` never touches key bindings.
- `delete_session(name, confirmation)`: `confirmation` is a `DeleteConfirmation { dirty, unmerged, working }` support type.
  1. Refuse `Unmanaged` sessions with a conflict.
  2. Re-check the guards at execution time: dirty file count, `unmerged_commits` count, and whether the state is `Working`. Any condition that is present but not confirmed refuses with a conflict naming every unconfirmed condition, so a check that went stale while a dialog was open never deletes unseen work.
  3. Refuse filesystem and branch operations when the worktree path is not strictly inside the canonical `worktree_root`, or the branch does not start with `branch_prefix`.
  4. Kill the tmux session if it is live.
  5. Remove the worktree with `git worktree remove --force`. An already-missing worktree path is tolerated: run `git worktree prune` and continue. If git refuses, stop with its stderr, and keep the branch and the registry entry.
  6. Log the branch tip SHA at info level, then delete the branch with `-D`.
  7. Remove the registry entry and persist.
  Return the deleted tip SHA so the CLI can print it and the user can recover with `git branch <name> <sha>`.
- `rename_session(old, new, keep_branch)`: refuse `Unmanaged` sessions. Validate the new name for availability on the slug and the tmux session name, and, unless `keep_branch`, fail early if the target branch exists. Rename the branch in the worktree first, then the tmux session, then persist the registry and refresh the tmux user options. On tmux failure, roll the branch rename back so the pair stays consistent. Never touch the worktree directory.
- `attach(name, return_mode)`: `return_mode` says whether to ensure the Ctrl-Q return binding first; the TUI passes `InstallReturnBinding`, the CLI passes `None`. Then `Multiplexer::attach(=<session>:)`. The facade reports the injected inside-tmux flag, and the UI wraps the call in `TerminalGuard` only when it is false. The CLI calls `attach` directly. Unmanaged sessions listed in the UI are attachable, not restartable or deletable.
- `open_shell(name, return_mode)`: if the tmux session is live, `ensure_shell_window` creates or selects a window identified by the window user option `@ringleader_shell` (not by name, since `allow-rename` can rename it), running `$SHELL` in the worktree, then attaches the client to it the same way as `attach`. The window persists like any other. If the tmux session is stopped, return an ephemeral shell request (`$SHELL`, cwd = worktree) for the UI to run under the terminal guard. Unmanaged sessions have no trusted worktree path, so `t` is disabled for them with a status-bar message.
- `restart(name)`:
  - `Exited`: `respawn_agent` writes a fresh handoff file and runs `respawn-pane` on the tagged agent pane by `pane_id`, passing the launcher command explicitly and never relying on the pane's original invocation. If the session has no tagged agent window, `respawn_agent` creates a new tagged window running the launcher.
  - `Stopped` with the tmux session gone: `create_session(spec)` recreates it in the existing worktree with the same single-invocation configuration. A missing worktree fails with a message pointing to `doctor`.
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
- Right panel: live preview via `preview(name)`, which runs `tmux capture-pane -e -p -J` on the tagged agent pane on a slower timer than the state poll, `-J` to join wrapped lines. Render ANSI through `ansi-to-tui`, fall back to plain text if parsing fails. On capture failure show session metadata.
- Diffs are computed only for visible rows on the `diff_refresh_ms` cadence, through `diff_stats(names)`, never for the whole registry on every tick. Twenty sessions must not spawn forty git processes per tick.
- Status bar lists keys and transient errors with the fixed order and width-based dropping described in the responsive tiers: `n` new, `d` delete, `R` rename, `Enter` attach, `t` shell, `s` restart, `r` refresh, `?` help, `q` quit.
- Sort order: state priority `Working`, `Waiting`, `Exited`, `Stopped`, `Unmanaged`, then name. Stable across refreshes because selection is name-based.
- Terminal-too-small guard: the TooSmall tier below 36x8, see the responsive tiers above. There is no fixed 80x24 cutoff; tiers scale all the way down.
- `n` dialog: name field, repo picker, agent picker, an optional initial prompt field, and an "Add repo" row. The agent picker preselects the first configured agent. The prompt field is single-line and submitted with the session; leaving it empty creates the session with no initial message. The picker lists `discover_repos` for the launch scope, with explicit config overrides merged in and marked. The resolved scope path is shown at the top of the picker and can be changed with a path input.
- Empty picker state: when the scope contains no repos, show "no repos found in <scope>" with two actions: "Add repo" and "Change scope". No silent fallback to `$HOME`.
- "Add repo" row: opens a path input, validates with `git -C <path> rev-parse --show-toplevel`, derives the name from the slugged basename, appends an explicit override to the config with `toml_edit`, reloads the picker, selects the new repo. This is the fix for the claude-squad pain point. Adding is optional: scanned repos already work without config entries.
- `d` dialog: shows name, branch, worktree, diff. A dirty worktree shows "N uncommitted files will be lost". A branch with commits on no other ref shows "N commits on <branch> exist on no other branch and will be lost". Either one requires a second confirmation. A `Working` session additionally warns "agent is actively working". Confirming passes a `DeleteConfirmation` with a flag set for each warning that was shown. If the service refuses because the state changed after the dialog opened, the dialog reopens with fresh numbers.
- `R` dialog: old and new name fields plus a "rename branch" toggle, on by default. The dialog shows the branch rename that will happen.
- `r`: `reload()` config and registry from disk, refresh diffs and observations.
- `t`: open a terminal in the selected session's worktree. If the tmux session is live, create or select the shell window (identified by `@ringleader_shell`, so a user rename cannot break it) in that session, running `$SHELL` in the worktree directory, and attach the client to it, so it persists like any other tmux window. If the tmux session is stopped, suspend the TUI and run an ephemeral `$SHELL` in the worktree, returning to the TUI on exit. Uses the same terminal guard as attach. `$SHELL` is resolved from the app's environment with `sh` as fallback. Disabled for unmanaged sessions. With no session selected, `t` shows a status-bar message and does nothing.
- `s`: restart the selected session. `Exited` sessions get `respawn-pane`; `Stopped` sessions get their tmux session recreated in the existing worktree. Disabled for `Working`, `Waiting`, and `Unmanaged` with a status-bar message.
- `q`: quit the app. tmux sessions keep running in the background; the next launch reconciles and lists them again.
- `?` help dialog: key bindings, config and log paths, version, and the Ctrl-Q behavior including the inside-tmux caveat. Centered overlay, closed with `?`, `Esc`, or `q`.
- Bell: when `bell_on_waiting` is enabled, `App` keeps the previous state per session and asks `SessionState`'s edge predicate whether the new state is a `Working` to `Waiting` transition. On that edge it calls `ring_bell()`, which writes BEL to the attached client's tty, so it reaches the user even when the TUI's pane is hidden. If no client is attached, no bell. No bell for `Exited`. While an outside-tmux attach child runs, the TUI is suspended and does not poll, so the bell fires only inside tmux or while the home screen is shown. The help dialog states this.
- Event loop drains `Event::Resize` and forces a full redraw, so the too-small guard clears and recalculates when the terminal grows.

Attach and Ctrl-Q:

- `main` detects `$TMUX` once. Outside tmux: terminal guard leaves raw mode and alternate screen, runs `tmux attach-session -t =<session>:` as a child, restores after it returns.
- Inside tmux: run `tmux switch-client -t =<session>:`. The TUI keeps running and polling. `attach-session` cannot be used from inside tmux by a child process; it fails with "sessions should be nested with care".
- `t` uses the same paths: live sessions get the shell window and the outside/inside attach split, stopped sessions get an ephemeral `$SHELL` under the terminal guard. Ctrl-Q returns to the TUI from the shell window too, since the binding matches on managed session names.
- Ctrl-Q is chosen as the return key over Ctrl-D because Ctrl-D is meaningful to the agents themselves, such as EOF to exit Claude Code, so intercepting it would break the session.
- Only the TUI installs the binding. CLI `new` and `attach` never touch bindings, so a session attached from the CLI receives Ctrl-Q as a normal key unless a TUI is running.
- The binding embeds its return target, so it never depends on `client_last_session`, which changes as soon as the user switches sessions inside tmux:
  - Outside tmux: `bind-key -T root C-q if-shell -F '#{m:<prefix>*,#{session_name}}' 'detach-client' 'send-keys C-q'`. Detaching ends the attach child and the TUI resumes.
  - Inside tmux: `bind-key -T root C-q if-shell -F '#{m:<prefix>*,#{session_name}}' 'switch-client -t <tui_session_id>' 'send-keys C-q'`, where `<tui_session_id>` is the TUI's own `#{session_id}` (such as `$3`), read at startup with `display-message -p -t <TMUX_PANE> '#{session_id}'`. A session id survives a rename of the TUI's session.
  - Each branch is passed as one quoted command string. The brace form (`{ ... }` passed as separate arguments) fails on 3.6a with `unknown command` or `too many arguments`; do not use it. The prefix pattern is built from `tmux_prefix` at runtime, which config validation restricts to `[a-z0-9-]` so it cannot break the format string. `send-keys C-q` does not recurse.
- Install timing: `start-server` cannot keep a server alive with no sessions (`exit-empty`), so there is no server bootstrap step. The TUI calls `install` immediately before every attach, switch, or shell open, when the target session guarantees a running server. `install` is idempotent: when the current root C-q is already recognizably Ringleader's (its condition is the managed prefix match), it changes nothing and saves nothing. A fresh server started after the TUI launched gets the binding on the next attach.
- Saving the previous binding: before the first install on a server, `TmuxReturnBinding` runs `list-keys -T root -1 C-q`. Exit 0 means bound, and stdout is saved verbatim. Exit 1 with stderr starting `unknown key:` means unbound. Any other failure aborts the install with a status-bar error. The record in `$XDG_STATE_HOME/ringleader/bindings.json` holds the server's `#{socket_path}` and `#{pid}` with the saved binding.
- Restoring: an unbound record runs `unbind-key -T root C-q`. A bound record writes the saved line to a temp file in the state directory and runs `tmux source-file` on it, since the saved line uses tmux quoting and cannot be replayed as argv. The TUI's binding guard restores on exit and on panic, before the terminal guard runs.
- Crash safety: at startup, `repair_stale` reads `bindings.json`. When the recorded server is still running (same socket path and pid) and its root C-q is still recognizably Ringleader's, it restores the saved binding. Otherwise it discards the record, so a binding from an older server never overwrites the current server's config. This covers SIGKILL and power loss, which the panic hook and signal handling cannot.
- The per-session `key-table` alternative is rejected, not deferred. Testing on 3.6a showed it bypasses root bindings entirely and swallows unbound keys rather than forwarding them to the pane, and it replaces the user's whole root table for managed sessions. Do not revisit it.
- Verification criteria: C-q returns to the home page from managed sessions in both outside-tmux and inside-tmux contexts, including after the user switches between two managed sessions first. C-q still reaches non-managed sessions. All pre-existing root bindings are restored on exit and on crash, including after two installs in one TUI run.

## 7. CLI

- `list [--json]` reconciles in memory only and never writes. It computes diffs for every session. It is safe to run while the TUI is running. Only the TUI and mutating commands persist the registry. JSON output is an array of objects with `name`, `repo`, `branch`, `worktree`, `agent`, `state`, and `diff` (`files`, `insertions`, `deletions`, `untracked`, or `null` for unmanaged sessions).
- `new --name <n> --repo <r> [--agent <a>] [--base-branch <b>] [--prompt <text>]`. Without `--agent`, the first `[[agents]]` entry is used. `--prompt` passes an initial message to the agent via the agent's `prompt_args`. `--repo` accepts a discovered repo name or a path, resolved against `discover_repos` and then validated with `git rev-parse`.
- `delete <name> [--force]`. The CLI never prompts. Without `--force`, a dirty worktree, unmerged commits, or a `Working` agent exits 3 and prints what would be lost. With `--force`, all three are confirmed and the warnings go to stderr. On success it prints the deleted branch tip SHA.
- `attach <name>`. Runs `attach-session` or `switch-client` only. It takes no lock and never touches key bindings.
- `rename <old> <new> [--keep-branch]`.
- `restart <name>`, restarts an `Exited` or `Stopped` session.
- `restart`, `delete`, and `rename` exit 2 for an unknown name and 3 for an `Unmanaged` session. `restart` exits 3 for a `Working` or `Waiting` session and 1 when the session's agent is missing from config.
- `repo add <path> [--name <n>] [--base-branch <b>]` writes an explicit override, `repo remove <name>` removes an override and reports when the repo is only discovered by scanning, `repo list [--all]` lists the repos discovered for the current scope and marks explicit overrides, with `--all` also listing overrides outside the scope. `repo remove` refuses while any registry session references the repo, naming the sessions and suggesting `delete` first.
- `__exec-agent --handoff <path>`, hidden. Invoked by tmux through the launcher path that `main` injects (`current_exe` in production). Reads the handoff file, deletes it, `env_clear`s, sets the environment from the file plus the kept pane variables, and execs the agent argv from the file. Never reads `config.toml`. Not documented in README.
- `doctor [--json]`, checks and reports:
  - tmux present, version >= 3.2 required for user options, `-e`, and `if-shell -F`
  - git present, version >= 2.42 required for `worktree add --orphan`, `worktree`, and `branch -m` in worktrees
  - each configured repo path resolves via `git rev-parse` and canonicalizes
  - each agent command resolves on the app's own `PATH`, which is the `PATH` the launcher passes to the agent
  - each agent with `prompt_args` has a template containing `{prompt}`
  - config path writable, or reported read-only because it resolves into `/nix/store`; state directory writable
  - registry readable, with a corrupt file reported on stderr
  - registry entries whose worktree, branch, or tmux session is missing
  - registry entries whose agent no longer exists in config
  - orphaned worktrees under the worktree root that no registry entry claims
  - orphaned branches matching `branch_prefix` with no worktree
- Exit codes, documented in README and AGENTS.md: 0 success, 1 user or validation error, 2 not found, 3 conflict, including a refused guard, an `Unmanaged` target, a session that is live and healthy, or the lock being held.
- JSON output uses stable lowercase state strings: `working`, `waiting`, `exited`, `stopped`, `unmanaged`.
- `--config <path>` and `$RINGLEADER_CONFIG` override for tests.
- Locking: `new`, `delete`, `rename`, `restart`, `repo add`, and `repo remove` take the write lock. `list`, `doctor`, `attach`, and `repo list` take no lock.

## 8. Config, state, locking

Config is app-owned at `$XDG_CONFIG_HOME/ringleader/config.toml`. First run writes a commented template with `create_new`, so it never overwrites an existing file. Writes go through `toml_edit` so user comments and formatting survive. The config path is canonicalized before any write. If the canonical path is inside `/nix/store`, report "config is Nix-managed, cannot write" and continue read-only, never fail. Otherwise writes are atomic and go to the canonical target (temp file next to it, then rename), so a symlinked config stays a symlink. An existing config that fails to parse is never rewritten: the TUI starts with defaults plus a status-bar error, and mutating commands exit 1.

```toml
worktree_root = "~/.local/share/ringleader/worktrees"
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
[[repos]]
name = "example-project"
path = "~/projects/example-project"
base_branch = "main"

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
- `state_poll_ms` drives observation, classification, and the spinner. Preview and diff refresh are slower, and diffs are computed for visible rows only.
- Repo scanning: scope resolves from the launch directory by walking up to the nearest `.git` directory, or the launch directory itself when there is none. If the walk-up meets a `.git` file at a linked worktree, it resolves the worktree to its main repository (`git rev-parse --git-common-dir`) and uses that repo as the scope, so launching from a managed worktree or a `t` shell still finds the repo. The walk is depth-limited by `scan_depth` (default 3), prunes `node_modules`, `target`, `vendor`, `.cache`, and `dist`, never enters `/proc`, `/sys`, `/dev`, or `/nix/store`, treats a `.git` directory as a repo and stops descending, skips `.git` files (worktrees and submodules), does not follow symlinks, and ignores permission errors. It stops after visiting 5,000 directories and shows a status-bar notice that the scan was cut short. Results are canonical paths, deduped, and cached in memory per picker session; `r` re-scans.
- `env` lists variable names. They add to a fixed baseline and never replace it. The baseline is `PATH`, `HOME`, `USER`, `LOGNAME`, `SHELL`, `LANG`, `LC_*`, `TZ`, `TMPDIR`, `XDG_*`, `SSH_AUTH_SOCK`, `TERMINFO`, `TERMINFO_DIRS`, and `LOCALE_ARCHIVE`. At session creation and on restart, the tmux adapter reads the values of the baseline and the configured names from the app's own environment and writes them, with the agent argv, into the handoff file through `AgentHandoff`. Values never pass through domain or application. Missing variables produce a warning, not a failure. Values are never logged.
- Handoff files live in `$XDG_RUNTIME_DIR/ringleader/handoff/`, or `$XDG_STATE_HOME/ringleader/handoff/` when there is no runtime directory. The directory is mode 0700, and each file is created with `create_new` and mode 0600. The launcher deletes its file before exec. Startup deletes handoff files older than 60 seconds left behind by a failed launch.
- `prompt_args` is the argument template for an initial prompt, with `{prompt}` substituted as one argv element. Claude Code uses `["{prompt}"]` (positional), opencode uses `["--prompt", "{prompt}"]`. Both were verified against the installed versions (Claude Code positional prompt; opencode 1.18.30 `--prompt`). A prompt containing multiple words, quotes, or a trailing `;` stays one argv element, never split or shell-evaluated, because the argv travels in the handoff file and never on a tmux command line. When the new-session prompt field is empty, no prompt arguments are appended, regardless of the template. An agent without `prompt_args` cannot receive an initial prompt and the UI says so.
- `~` in `worktree_root` and `Repo.path` expands to the home directory at load. `TomlConfigSource` then canonicalizes repo paths; when canonicalization fails it keeps the expanded path and `doctor` reports it. Resolution order for the config file: `--config` flag, then `$RINGLEADER_CONFIG`, then `$XDG_CONFIG_HOME/ringleader/config.toml` (falling back to `~/.config`). The first match wins, no merging.
- Config validation on load: `tmux_prefix` must match `^[a-z0-9-]+-$`, `branch_prefix` must match `^[a-z0-9-]+/$`, repo names use the `RepoName` alphabet, repo and agent names are unique, worktree root writable. No repo is required, since scanning supplies them. An empty or first-run config is valid and must not block startup; strict validation applies to mutating commands. First run seeds the template with the two default agents, `claude` and `opencode`, so `n` works immediately.
- Registry at `$XDG_STATE_HOME/ringleader/sessions.json` with a schema version field. Writes are atomic: write a temp file in the same directory, `sync_all` it, rename it over `sessions.json`, then `sync_all` the directory. Schema policy: refuse a newer version than the binary with a clear message. A corrupt file is moved aside as `sessions.json.corrupt-<timestamp>` only by a lock holder, which then continues with an empty registry plus a status-bar warning. `list` and `doctor` report a corrupt registry on stderr, exit 1, and change nothing. No migration machinery in v1, since there is no older schema yet.
- The registry is an index, not the source of truth. tmux user options are the source of truth for a live session:
  - `@ringleader_meta` holds one compact JSON object with repo name, repo path, branch, base commit, worktree path, and agent.
  - `create_session` writes it, `rename_session` updates it.
- `observe_all` makes one call: `list-panes -a -F` with `#{session_name}`, `#{session_created}`, `#{pane_id}`, `#{@ringleader_agent}`, `#{pane_dead}`, `#{window_activity}`, and `#{@ringleader_meta}`, tab-separated. Session user options expand on every pane row; this was verified on tmux 3.6a. It keeps the row of the tagged agent window for each session. A session with no tagged window keeps one row for its metadata and is observed as described in section 4. `window_activity` is epoch seconds with 1-second granularity. `pane_last_output_time` is unavailable on tmux 3.6a; `pane_activity` is not a format variable.
- Startup reconciliation merges registry entries and live tmux sessions matching `tmux_prefix`:
  - live session, valid metadata, no registry entry: adopt into the registry. The adopted name is the tmux session name with `tmux_prefix` stripped, and `created_at` is `#{session_created}`.
  - metadata is valid only if it parses, `branch` starts with `branch_prefix`, `worktree_path` lies strictly inside the canonical `worktree_root`, and the adopted name is a valid slug that is unique. Anything else is missing or malformed.
  - live session, missing or malformed metadata: state `Unmanaged`, attachable only, never edited, restarted, or deleted; warned about in the status bar
  - registry entry with no live session: keep, shown as `Stopped`, restartable in place
  - adopted sessions obey the same uniqueness rules as created ones
- `observe_all` treats a missing tmux server (nonzero `list-panes` with "no server running") as zero sessions, not an adapter error. On that path every registry session is simply `Stopped`.
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
- SIGHUP, SIGTERM, and SIGQUIT are registered as flags with `signal-hook`. The event loop checks them every tick and returns normally, so the guards run. SIGHUP is the usual way the TUI ends when its tmux pane or terminal window closes.

## 10. Testing and fixtures

- Domain: classifier boundaries (missing, dead, malformed metadata as `Unmanaged`, activity exactly at threshold, stale, fresh), slugging including the 64-char cap, the alphabet mapping, an input that slugs to empty, and slug collisions, `RepoName` slugging, diff accumulation including a binary file, and the `SessionState` bell edge predicate (`Working` to `Waiting` only).
- Application: hand-written `FakeSessionRegistry`, `FakeWorkspace`, `FakeMultiplexer`, `FakeReturnBinding`, `FakeConfigSource`, `FakeRepoScanner`, `FakeHostProbe`, `FakeLock` implementing the ports, shared from `src/test_support/`. Create, duplicate rejection on the slug, on an existing worktree directory, and on an existing branch, branch cleanup when worktree creation fails, default agent selection, dirty delete guard, unmerged-commit delete guard on a clean worktree, `Working` delete guard, a guard that appears after confirmation refusing the delete, delete refusing a worktree path outside the root, a failed worktree removal keeping the branch and registry entry, rename with branch, rename rollback, restart for `Exited` and `Stopped`, restart of a session with no tagged agent window, restart refusal for live and unmanaged, doctor findings, CLI service paths. Prompt expansion: template with and without prompt, multi-word prompt kept as one argv element, missing `prompt_args` refused, restart never replays the prompt. The `SessionSpec` carries environment names and never values. Reconciliation: adoption of an unknown live session with name and `created_at` derived from tmux, malformed metadata becomes `Unmanaged` and is never persisted as managed, metadata pointing outside `worktree_root` or at a branch without `branch_prefix` becomes `Unmanaged`, registry-only entries stay `Stopped`, adoption respects uniqueness. `open_shell` returns an attach for live sessions and an ephemeral request for stopped ones, and is refused for unmanaged sessions. `attach` from the CLI never calls `FakeReturnBinding`. Discovery: scope from a repo subdirectory, scope from a plain directory, explicit overrides winning on canonical path, scanned and explicit entries deduped, sessions never filtered by scope.
- Adapters with fixtures under `tests/fixtures/`, fake data only:
  - `config.toml`, `sessions.json`
  - `tmux-list-panes.txt` for the `observe_all` parser and reconciliation, including a dead agent pane next to a live shell pane in the same session, a `window_activity` value, a valid metadata record, a malformed one, and a session with no tagged agent window
  - `tmux-list-windows-shell.txt` for `ensure_shell_window`, including an existing `@ringleader_shell` window and a renamed shell window
  - `tmux-bindings.txt` for saving and restoring the previous root C-q binding, including a user binding and the `unknown key:` unbound case
  - `tmux-capture-pane-ansi.txt` for the ANSI preview fallback, plus `tmux-capture-pane-ansi-wrapped.txt` for a wrapped colored line, where `-J` can misalign ANSI
  - `git-numstat.txt`, including a binary `-\t-` row, and `git-untracked.txt` for diff parsing
- Git adapter tests run real git in `tempfile` repos, hermetically. Every git command in tests runs with `GIT_CONFIG_GLOBAL=/dev/null`, `GIT_CONFIG_NOSYSTEM=1`, `HOME=<tempdir>`, and `GIT_AUTHOR_NAME`, `GIT_AUTHOR_EMAIL`, `GIT_COMMITTER_NAME`, `GIT_COMMITTER_EMAIL` set on the `Command`, never through `std::env::set_var`. `GitWorkspace` takes this environment override through its constructor; production passes none. Fixture repos are created with `git init -b <branch>`, never relying on `init.defaultBranch`. Config, state, and data paths are injected, never resolved with `dirs` inside tests. Git tests cover: a cloned repo whose new branch has no upstream, the orphan path on an unborn HEAD, `unmerged_commits` returning 0 once another branch points at the tip, `remove_worktree` refusing a main worktree, a path outside the root, and a plain directory with nothing deleted, and a cross-check of sample slugs against `git check-ref-format --branch <branch_prefix><slug>`.
- tmux tests detect the binary and skip when absent. Each tmux test starts its own server with `-S <tempdir>/tmux.sock -f /dev/null`, so tests never share a server, never load the user's tmux config, and never touch the user's default server. `TmuxMultiplexer` and `TmuxReturnBinding` take the socket and config arguments through their constructors (via `TmuxCommand`); production passes none. The inside-tmux flag and `TMUX_PANE` are injected, never read from the developer's environment. A Drop guard runs `kill-server` on the test socket. CI and the Nix `checkPhase` use the same setup. tmux tests cover: exact targets with `ringleader-fix` absent and `ringleader-fix-auth` present, arguments ending in `;` (`fix bug;`, `a\;`, `;`, `x ; y`) arriving byte for byte with an argument after them, `@ringleader_meta` expansion in `list-panes`, respawn of the tagged agent pane while a shell window is current, Ctrl-Q binding install and restore including two installs followed by a restore, a restore after a server restart leaving the new server untouched, and a switch between two managed sessions before C-q, and that the server's `/proc/<pid>/cmdline` and `show-environment` never contain a fixture secret.
- The launcher command prefix is a constructor dependency of `TmuxMultiplexer`. `main` passes `[current_exe, "__exec-agent"]`. tmux tests pass a harmless stand-in such as `["sh", "-c", "sleep 30", "--"]`, so panes stay alive and the test harness is never run inside a pane.
- Launcher tests: the exec'd environment contains the baseline, the configured names, and the kept pane variables and nothing else, a value present only in the inherited environment for an allowlisted name does not reach the agent, and the handoff file is deleted before exec.
- Repo scanner tests build a `tempfile` directory tree with fixture repos: nested repos, a `.git` file worktree that must be skipped, a pruned directory (`node_modules`), a depth-4 repo that must be excluded at depth 3, a regular directory that is not a repo, a launch from inside a linked worktree that resolves to its main repo, and a tree larger than the visit cap (with the cap injected small). Scanner tests do not touch the real filesystem outside tempdirs.
- Config tests: a symlinked config stays a symlink after a write, an unparseable config is never rewritten, and the first-run template never overwrites an existing file.
- Registry and lock tests: a corrupt registry is moved aside only by a lock holder, and two `FileLock` instances on the same path in one process conflict.
- All tests are unit tests inside `src/` (`#[cfg(test)] mod tests`, or `src/test_support/`). `tests/` holds fixtures only. Do not add `tests/*.rs` integration tests; a single binary crate has no library target for them to link. If that changes, a `lib.rs` must be introduced deliberately.
- UI: `TestBackend` render assertions for the spinner glyph, the `●` bullet, the `Unmanaged` marker, each responsive tier at representative sizes, status bar dropping at 60 and 36 columns, the resize redraw, the help overlay, the delete confirmation, and the empty state. One `EventLoop` test: a raised signal flag exits through the normal return path.
- One concept per test, public interfaces only.

## 11. Tooling and CI

- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, all through `nix develop` so local and CI use the same rustc 1.95.
- In a binary crate, anything `main` cannot reach yet is dead code, and most of milestones 2 to 28 build ahead of their wiring. Until then, `main.rs` carries `#![cfg_attr(not(test), allow(dead_code, unused_imports))]`. Milestone 29 removes it, and `cargo clippy --all-targets -- -D warnings` must pass without it.
- `.github/workflows/ci.yml` on Ubuntu: DeterminateSystems nix installer, then `nix develop -c cargo fmt --check`, `nix develop -c cargo clippy ...`, `nix develop -c cargo test`, and `nix build .#` plus `nix flake check`. No separate toolchain action, the dev shell is the single source of truth.
- The workflow file is authored and committed, but it stays dormant until the owner creates the GitHub repository and pushes. Do not add CI badges to the README until then.

## 12. Flake for this repo

- Input `nixpkgs` pinned at `nixos-26.05` for stability.
- `packages.default = rustPlatform.buildRustPackage` with `cargoLock.lockFile`, wrapped with `makeWrapper --prefix PATH : ${lib.makeBinPath [ tmux git ]}` so `nix run` works standalone.
- `doCheck = true` with `nativeCheckInputs = [ git tmux ]`, so `nix build` runs the test suite, including the tmux tests on per-test sockets. The sandbox has no usable `HOME` and no git identity, which the hermetic git environment in section 10 provides.
- `devShells.default`: rustc, cargo, clippy, rustfmt, tmux, git, nixfmt.
- `formatter = nixfmt-tree`, `checks` build the package. Commit `flake.lock`.

## 13. Consumer integration

Out of scope for this repo. The flake's `packages.default` and `devShells.default` are the interface. Any Nix consumer can add the flake as an input and install the binary. Integration work happens outside this repository, so nothing here may reference a specific downstream setup or configuration.

## 14. Documentation split

- `README.md`: for humans. Terse and minimal. What it is, install (`nix run`, `nix profile install`, add as a flake input), first run, key bindings table, CLI synopsis. Generic examples only. No architecture, no agent instructions, no CI badge until the repo exists.
- `AGENTS.md`: the agent repo documentation and the living reference. It must embed a copy of section 3's tree, because it has to be self-contained; `docs/plans/v1-bootstrap.md` is the historical plan and is not a substitute. Layout, the one-primary-type-per-file rule and the fact that the tree is the file contract, port rules, OOP conventions (private fields, `impl` blocks, constructor injection, no free functions), the strict comment policy (comments are failures, fix the code, justify any survivor to the owner before committing, never in the commit body), naming rules, CLI and TUI wiring boundaries, prose capitalization, exact commands (`nix develop`, cargo test/clippy/fmt), test strategy and fixtures, commit conventions, the local-only no-remote rule, the plan storage convention below, and the rule that README stays terse and human-facing.
- Plan storage convention for future work: one file per effort at `docs/plans/YYYY-MM-DD-slug.md`, with a `> Status:` header (`active`, `completed`, or `abandoned`). `docs/plans/v1-bootstrap.md` is the v1 history.

## 15. Milestones, one commit each

1. Scaffold cargo project, rustfmt, MSRV confirmed against the dev shell, flake dev shell with `doCheck`, CI skeleton, and the temporary dead-code allowance from section 11.
2. Add domain session model, slug rules, and state classifier with tests.
3. Add application ports and fakes.
4. Add the `SessionService` facade and the create, delete, rename, observe, and attach use cases with tests.
5. Add git worktree adapter with hermetic tempfile tests, base branch ladder, `--no-track` and orphan creation, unmerged-commit check, and guarded removal.
6. Add tmux adapter on per-test private sockets, with exact targets, agent window tagging, `;` escaping, and validate the Ctrl-Q binding forms from section 6 against the pinned tmux.
7. Add JSON registry with atomic synced writes, corruption recovery, schema policy, and single-instance locking.
8. Add the repo scanner adapter with tempfile tree tests, cwd scope resolution, linked-worktree resolution, the visit cap, and explicit override merging.
9. Add a thin vertical slice: minimal session table, attach, terminal guard, signal handling, and `q` quit.
10. Add tmux metadata mirroring, reconciliation, and adoption with metadata validation.
11. Add the `__exec-agent` launcher, the handoff file with the baseline environment, and `prompt_args` expansion.
12. Add `Exited` and `Stopped` restart use cases.
13. Add config loader with validation, first-run template, and atomic `toml_edit` writes.
14. Add CLI subcommands list, new, delete, attach, rename, and restart, plus exit codes, write locking, and JSON state strings.
15. Add CLI `repo` and `doctor` subcommands with the host probe adapter.
16. Render working spinner and waiting bullet.
17. Add responsive tiers, resize redraw, status bar priority dropping, column dropping, sort order, name-based selection with arrow and `h`/`j`/`k`/`l` navigation, and the home empty state.
18. Add TUI `s` restart and `r` reload keys.
19. Add the `t` shell action, live via `shell` window and stopped via ephemeral `$SHELL`.
20. Add new session dialog with repo picker, scope header, agent picker, initial prompt field, and empty picker state.
21. Add "Add repo" row with `git rev-parse` validation.
22. Add delete confirmation with dirty, unmerged-commit, and `Working` guards.
23. Wire the Ctrl-Q return binding into TUI attach, with idempotent install, saved-binding restore, and startup repair.
24. Add `R` rename dialog and rename flow with branch rename and rollback.
25. Refine live preview with `ansi-to-tui`, `-J`, and visible-row diff computation.
26. Add logging and status bar error surfacing.
27. Add bell on Working to Waiting transition.
28. Add in-TUI help dialog.
29. Remove the dead-code allowance, then add README and AGENTS.md, with the tree copied into AGENTS.md.
30. Finish GitHub Actions.

## 16. Risks to verify early

- Worktree removal never deletes recursively. `git worktree remove --force` after the guards handles submodule worktrees; anything git refuses (a main worktree, a path that is not a worktree, a locked worktree) is reported and left alone.
- Ctrl-Q root binding conflicts with the user's tmux config. Restore correctness on crash is handled by the saved-binding record keyed by server plus startup repair; `key-table` is rejected and documented as such.
- The inside-tmux attach path uses `switch-client`, never `attach-session` from a child process.
- Bare tmux targets prefix-match other sessions. Every target is `=<session>:` or a `pane_id`.
- The return binding is installed before each attach rather than at startup, because a server with no sessions exits.
- `window_activity` has 1-second granularity, so `activity_idle_seconds` below 1 is meaningless.
- Detached session sizing (`-x/-y`) must match what preview expects, so `capture-pane -J` output is not clipped at 80 columns.
- Terminal raw mode and alternate screen restore around `tmux attach`, moved early as the milestone 9 vertical slice.
- Resize events must force a redraw so tiers and the too-small guard recalculate instantly.
- Unborn-HEAD repos use `git worktree add --orphan` instead of erroring on the base-branch ladder; this needs git 2.42.
- Initial prompt argv must stay one element per template entry; a prompt with quotes, shell metacharacters, or a trailing `;` must reach the agent intact, which the handoff file guarantees. Both Claude Code and opencode were verified to accept it, but agents change their flags; `doctor` should warn when a configured `prompt_args` template lacks `{prompt}`.
- The launcher's cleared environment must keep the baseline, or agents fail to resolve, lose their auth, and render without `TERM`.
- Branch rename rollback ordering in `rename_session`.
- Dirty worktree removal, unmerged commits, and branch deletion safety, including the explicit confirmation for a `Working` session and re-checking the guards at execution time.
- Activity heuristic false positives if an agent paints an idle animation, tunable via `activity_idle_seconds`.
- `ansi-to-tui` parsing of capture output, especially wrapped colored lines; always keep the plain text fallback.
- Preview load: only the selected session is captured, diffs are computed for visible rows only, both slower than the state poll.
- Lock contention between a running TUI and CLI mutations, make the message actionable, exit 3.
- User option expansion in `list-panes -F` was verified working; keep the per-session `show-options -qv` fallback as a defensive path.
- Adoption when the worktree was deleted manually: adopt the session, surface the missing worktree in `doctor` and the status bar.
- Global `destroy-unattached` in the user's tmux config is why creation and configuration happen in a single tmux invocation.
- tmux server environment drift: `-e` does not strip the inherited server environment, so the `__exec-agent` launcher owns the real allowlist, and values travel only in the handoff file.
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
