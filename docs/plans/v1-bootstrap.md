# Plan: Ringleader, a tmux session manager for Claude and opencode

> Status: active. Flip to `completed` when v1 ships. This file is the historical plan, not living documentation; `AGENTS.md` is the living documentation.

## 0. Ground rules for the implementing agent

- Load the clean-code skill before writing any code.
- Comments are treated as failures to express intent in code. Every comment is a signal, not documentation: revisit the code it explains and improve it until the comment is unnecessary, then delete the comment. Rename the variable, extract the function, introduce the type, or restructure the control flow that made the explanation feel needed. Never add a comment to justify messy code, and never leave a comment as a substitute for a better name. The only surviving comments are the clean-code exceptions: legal headers, non-obvious intent that cannot be encoded in structure, warnings of real consequences, and TODOs that reference a tracked issue. Before each commit, list every surviving comment in the commit body and name the exception it falls under. If a comment cannot be named against an exception, delete it and fix the code instead. This rule outranks convenience; a commit that adds an unexplained comment is not done.
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
- Delete removes worktree, deletes branch, warns and double-confirms if dirty.
- Rename renames the tmux session, the registry entry, and the git branch by default. `--keep-branch` and a dialog toggle opt out. The worktree directory name does not change.
- Colors use ANSI named colors and modifiers only. Base style `Color::Reset`. Selection `Modifier::REVERSED`. Diff green/red. No RGB anywhere.
- Worktrees live under `$XDG_DATA_HOME/ringleader/worktrees/<repo>/<slug>`, outside the repos.
- The app owns its config file. No config file is managed by Nix.
- MSRV 1.95, edition 2024, matching the rustc shipped in nixpkgs 26.05 (rustc 1.95.0, verified). Milestone 1 confirms this against the dev shell before any code is written.
- Session state is observed from tmux, not inferred from agent internals. A pure classifier turns observations into states. Activity comes from `window_activity`, an epoch-seconds format variable that updates within 1 second of pane output. `pane_last_output_time` is not available in tmux 3.6a.
- Quitting with `q` exits the TUI only. Managed tmux sessions keep running in the background and are rediscovered on the next launch.
- Session metadata is mirrored into tmux session user options so a live managed session can be adopted back after a restart or registry loss.
- Managed tmux sessions are created and configured in a single tmux invocation (`new-session ... \; set-option ...`). Setting options in a second call can destroy the session when the user's global `destroy-unattached` is on.
- Managed tmux sessions set `destroy-unattached off`, so detaching, including via Ctrl-Q, never destroys them, even if the user's global config enables it.
- Managed tmux sessions set `remain-on-exit on` on the session's window (`set-option -w -t <session>: remain-on-exit on`), so an agent exit leaves a dead pane that `restart` can revive. `remain-on-exit` is a window and pane option, not a session option.
- Detached sessions get an explicit size (`-x 200 -y 50` by default, configurable) so previews are not limited to 80x24.
- Agents launch through a hidden `ringleader __exec-agent` subcommand, invoked via `current_exe`. The launcher runs `env_clear`, restores only the configured allowlist names (values supplied by tmux `-e` at session creation), and execs the agent. `tmux -e` alone does not strip the server's inherited environment, so it is not an allowlist. Only variable names are configured and logged, never values.
- Base branch resolution never fetches. It reads local refs only. On a repo with an unborn HEAD and no resolvable base, creation falls back to `git worktree add -b <branch>` with no start point, which git treats as an orphan.
- Session names use one canonical slug function for the registry name, tmux session name, worktree directory, and branch, with a 64-character cap and uniqueness checked on the slug. Branch names are validated with `git check-ref-format --branch`.
- Attach behavior depends on `$TMUX`: outside tmux, `tmux attach-session` runs as a child under the terminal guard. Inside tmux, `tmux switch-client -t <name>` switches the current client and the TUI keeps running. A child process cannot use `attach-session` from inside tmux; it fails with "sessions should be nested with care".
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
    │   ├── session_service.rs
    │   ├── doctor.rs
    │   └── ports/
    │       ├── mod.rs
    │       ├── session_registry.rs   # SessionRegistry trait
    │       ├── workspace.rs          # Workspace trait
    │       ├── multiplexer.rs        # Multiplexer trait
    │       ├── config_source.rs      # ConfigSource trait
    │       ├── repo_scanner.rs       # RepoScanner trait
    │       └── lock.rs               # Lock trait
    ├── adapters/
    │   ├── mod.rs
    │   ├── agent_launcher.rs     # AgentLauncher, the hidden __exec-agent mode
    │   ├── directory_repo_scanner.rs # DirectoryRepoScanner
    │   ├── toml_config_source.rs # TomlConfigSource
    │   ├── json_registry.rs      # JsonSessionRegistry
    │   ├── lock_file.rs          # FileLock
    │   ├── git_workspace.rs      # GitWorkspace
    │   └── tmux_multiplexer.rs   # TmuxMultiplexer
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
    │       ├── new_session.rs      # NewSessionDialog
    │       ├── delete_session.rs   # DeleteSessionDialog
    │       ├── rename_session.rs   # RenameSessionDialog
    │       └── add_repo.rs         # AddRepoDialog
    └── test_support/               # #[cfg(test)] doubles shared by unit tests
        ├── mod.rs
        ├── fake_session_registry.rs # FakeSessionRegistry
        ├── fake_workspace.rs        # FakeWorkspace
        ├── fake_multiplexer.rs      # FakeMultiplexer
        ├── fake_config_source.rs    # FakeConfigSource
        ├── fake_repo_scanner.rs     # FakeRepoScanner
        └── fake_lock.rs             # FakeLock
```

Rules for `AGENTS.md`:

- `domain` never imports `crossterm`, `ratatui`, `std::process`, or any adapter.
- `application` defines traits and use cases, never shells out.
- `ui` calls `SessionService` only.
- OOP in Rust: all state in structs with private fields, all behavior in `impl` blocks, traits as interfaces, composition over inheritance, one primary type per responsibility, dependencies injected through constructors. No public data fields, no free functions except `main`.
- One primary type per file, named after it in snake case. Supporting types owned by the primary type live in the same file, unit tests live inline. `mod.rs` declares and re-exports only. The tree in section 3 is the file contract; do not merge primary types into one file.
- `cli.rs` owns the clap argument structs (`ListArgs`, `NewArgs`, and so on) as support types of `Cli`. They get no files of their own.
- Why the tree splits what it splits: `Session`, `SessionName`, and `SessionState` each get a file because each is used independently across modules, carries behavior or invariants, and changes for its own reasons. Merging files later is mechanical; splitting a file that grew is disruptive, so prefer the separate file. The reverse is also true: do not invent files for types the tree does not list.
- `src/test_support/` is `#[cfg(test)]` only and must not appear in the binary. It exists so unit tests across modules share one set of hand-written doubles.
- Other support types, such as a parser result, a doctor finding, or a dialog's in-progress state, live in their primary type's file and get no tree entry.
- Comments: the repository is comment-hostile. Every comment is presumed unnecessary until proven otherwise. The fix for "this needs explaining" is better code: rename, extract, type, restructure. Commit messages must list surviving comments and the clean-code exception each falls under, or state "no comments added". Doc-comments are comments and follow the same rule; do not document what a well-named item already says.
- Human-facing prose uses "Ringleader". Identifiers stay lowercase.
- Do not create a remote or push.

## 4. Domain

- `SessionName`: validated non-empty, one canonical slug function used for registry name, tmux session name, worktree directory, and branch, capped at 64 characters. Uniqueness is global across repos and checked on the slug, so two names that slug to the same string are rejected. Branch names are validated with `git check-ref-format --branch`. tmux sanitizes illegal characters (`:` and `.` become `_`), so the slug must already be legal for tmux and for a git ref.
- `Agent`: configured display name, launch command, environment variable names the launcher restores, and `prompt_args`, the argument template used to pass an initial prompt. Values are never stored in the domain.
- `SessionState`: `Working`, `Waiting`, `Exited`, `Stopped`, `Unmanaged`.
- `Session`: name, repo name, repo path, worktree path, branch, agent, tmux session name, created timestamp (epoch seconds from `SystemTime`).
- `SessionMetadata { repo_name, repo_path, branch, worktree_path, agent }`, the copy mirrored into tmux.
- `SessionObservation { exists, pane_dead, activity_at, metadata: Option<SessionMetadata> }`.
- `Config`: parsed config.toml, with explicit repo overrides, agents, prefixes, refresh intervals, and the bell flag.
- `RepoScope`: the resolved directory to scan, produced by `RepoScanner`. Support type on `Repo`. The launch directory enters through `main` and is threaded through the service, so neither domain nor application reads the process cwd.
- `SessionClassifier`, pure, takes `now` as an argument, never reads the clock itself. No stored history:
  1. session missing, `Stopped`
  2. no valid metadata (malformed or absent), `Unmanaged`
  3. `pane_dead`, `Exited`
  4. `now - activity_at < activity_idle_seconds`, `Working`
  5. otherwise, `Waiting`
- `DiffStat { files, insertions, deletions, untracked }`, computed per request, not stored.
- `SessionSummary { session, state, diff }` is what the service returns to the UI.

`Waiting` is the green bullet: the pane is running and quiet, so the agent is at its prompt. `Exited` requires `remain-on-exit on` on managed sessions, set at creation, so the pane survives agent exit and `respawn-pane` can restart it.

## 5. Ports and use cases

Ports:

- `SessionRegistry`: `load`, `save`.
- `Workspace`: `create_worktree`, `remove_worktree`, `prune_worktrees`, `delete_branch`, `rename_branch`, `branch_exists`, `dirty_state`, `diff_stat`, `default_branch`, `validate_repo`.
- `Multiplexer`: `create_session`, `kill_session`, `observe_all`, `attach`, `capture_pane`, `rename_session`, `respawn_pane`, `create_in_worktree`, `ensure_shell_window`, `set_metadata`, `ensure_server`, `install_ctrl_q_return`, `restore_ctrl_q_return`, `save_binding_state`, `restore_binding_state`, `attached_client_tty`.
- `ConfigSource`: `load`, `add_repo`, `remove_repo`.
- `RepoScanner`: `resolve_scope(launch_directory) -> RepoScope` and `scan(scope) -> Vec<Repo>`. The nearest-`.git` walk and the bounded directory walk both live in the adapter, keeping the service free of filesystem logic.
- `Lock`: `acquire_lifetime`, `acquire_for_write`.

Use cases:

- `list_summaries()`: reconcile without persisting, observe all sessions in one tmux call, refresh diffs for visible rows, return summaries. `list` is read-only and lock-free. Only the TUI and mutating commands persist the registry.
- `reconcile(persist)`: merge the registry with live tmux sessions. Adopt live sessions matching `tmux_prefix` that carry valid metadata but have no registry entry. Keep registry entries with no live session as `Stopped`. Persist the merged registry only when `persist` is true: the TUI and mutating commands pass true, `list` passes false.
- `discover_repos(launch_directory)`: `RepoScanner.resolve_scope`, then `scan`, then merge explicit config overrides keyed by canonical path with explicit winning, dedupe, and return. Never filters sessions. Cached in memory for the life of the picker; `r` re-scans. The launch directory is passed in from `main`, so the service never reads cwd itself.
- `create_session(request)`: validate, ensure unique slug, resolve base branch (unborn HEAD falls back to an orphan start point), create worktree and branch, create the detached tmux session in the worktree running the `__exec-agent` launcher with configured env names via `-e`, and configure it in the same invocation: `remain-on-exit on` on the window, `destroy-unattached off` on the session, explicit `-x/-y` sizing. Build the agent argv by expanding `prompt_args` with the request's prompt. Write the tmux session user options, install the Ctrl-Q return binding, then persist. The prompt is not stored in the session or registry.
- `delete_session(name, confirmed_dirty)`: dirty guard, kill tmux session, remove worktree, delete branch, remove registry entry. Tolerate an already-missing worktree. `remove_worktree` falls back to `rm -rf` plus `git worktree prune` when submodules make plain `git worktree remove` fail. Branch deletion uses `-D` only after the dirty guard has passed. Delete refuses while the session's agent is `Working` unless the caller passes an explicit force.
- `rename_session(old, new, keep_branch)`: validate both names for uniqueness on the slug, fail early if the target branch exists. Rename the branch in the worktree first, then the tmux session, then persist the registry and refresh the tmux user options. On tmux failure, roll the branch rename back so the pair stays consistent. Never touch the worktree directory.
- `attach(name)`: outside tmux, delegate to `Multiplexer.attach` under the terminal guard. Inside tmux (`$TMUX` set), use `tmux switch-client -t <name>` and keep the TUI running. Unmanaged sessions listed in the UI are attachable, not restartable or deletable.
- `open_shell(session)`: if the tmux session is live, `ensure_shell_window` creates or selects a window identified by the window user option `@ringleader_shell` (not by name, since `allow-rename` can rename it), running `$SHELL` in the worktree, then attaches the client to it using the same outside/inside tmux split. The window persists like any other. If the tmux session is stopped, return an ephemeral shell request (`$SHELL`, cwd = worktree) for the UI to run under the terminal guard. Unmanaged sessions have no trusted worktree path, so `t` is disabled for them with a status-bar message.
- `restart(session)`: for an `Exited` session, `respawn-pane` with the command and environment passed explicitly, never relying on the pane's original invocation. For a `Stopped` session whose tmux session is gone, `create_in_worktree` recreates the tmux session in the existing worktree and reapplies the metadata and options. Restart never replays the initial prompt, even if one was used at creation. Unmanaged sessions cannot be restarted. If the session's agent no longer exists in config, fail with a clear message and a doctor finding.
- `doctor.run()`: see section 7.

## 6. UI

Layout target at 100x28, and 80x24 is the smallest Full tier. The implementing agent must match this composition: fixed left table, preview panel on the right, one status bar, borders drawn with the terminal's box-drawing style. Widths are proportional: the table takes about 45%, the preview the rest, and only the selected row is reverse video.

```
┌ Sessions ──────────────────────────────────────┐┌ Preview ──────────────────────────────────────┐
│  Name          Branch               Diff       ││ ringleader-fix-auth · working · Claude        │
│⠹ fix-auth      ringleader/fix-auth  +124 -31 ?2││ ~/.local/share/ringleader/worktrees/fix-auth  │
│● dark-mode     ringleader/dark-mode +12 -4 ?0  ││ ────────────────────────────────────────────── │
│⠹ spike-tui     ringleader/spike-tui +0 -0 ?0   ││ ✻ Thinking…                                   │
│✗ docs          ringleader/docs      +57 -2 ?0  ││                                               │
│○ flaky-test    ringleader/flaky-test +3 -1 ?0  ││   Read src/ui/home_view.rs                    │
│? mystery       ringleader/mystery   —          ││   ⎿  Read 214 lines                           │
│                                                ││                                               │
│                                                ││   Edited src/domain/session_name.rs            │
│                                                ││   ⎿  Added 12, removed 3                       │
│                                                ││                                               │
│                                                ││ ✻ Cogitating… (12s · ↑ 1.2k tokens)            │
│                                                ││                                               │
└────────────────────────────────────────────────┘└───────────────────────────────────────────────┘
 n new   d delete   R rename   Enter attach   t shell   s restart   r refresh   ? help   q quit
```

- The selected row uses `Modifier::REVERSED`, not a marker glyph, so selection is visible on any theme.
- Preview header line one: tmux session name, state, agent. Line two: worktree path, truncated from the left. Then captured pane content.
- The unmanaged row shows `—` for diff, because it has no trusted worktree.

Empty state on first launch:

```
┌ Sessions ──────────────────────────────────────┐┌ Preview ──────────────────────────────────────┐
│  Name          Branch               Diff       ││                                               │
│                                                ││            No session selected                │
│                                                ││        Press n to create one, or t for        │
│                                                ││        a shell. Scanned scope: ~/projects     │
│                                                ││                                               │
└────────────────────────────────────────────────┘└───────────────────────────────────────────────┘
```

Responsive layout tiers. The home screen adapts to the terminal size on every render. The tier is derived by `HomeView` from the current terminal dimensions, as a support type in `home_view.rs`. No config key, no persistence.

| Tier | Condition | Layout |
|---|---|---|
| Full | width >= 80 and height >= 24 | Table and preview side by side |
| Compact | width >= 60 and height >= 12 | Table full width, preview hidden |
| Minimal | width >= 36 and height >= 8 | Table full width, columns reduced to state and name |
| TooSmall | below 36 wide or 8 tall | Centered `Terminal too small` |

Compact tier, preview hidden, branch and diff kept:

```
┌ Sessions ──────────────────────────────────────────┐
│  Name            Branch               Diff         │
│⠹ fix-auth        ringleader/fix-auth  +124 -31 ?2  │
│● dark-mode       ringleader/dark-mode +12 -4 ?0    │
│✗ docs            ringleader/docs      +57 -2 ?0    │
│○ flaky-test      ringleader/flaky-test +3 -1 ?0    │
└────────────────────────────────────────────────────┘
 n new   d delete   Enter attach   ? help   q quit
```

Minimal tier at 36 wide, state and name only:

```
┌ Sessions ────────────────────────┐
│⠹ fix-auth                        │
│● dark-mode                        │
│✗ docs                            │
│○ flaky-test                      │
└──────────────────────────────────┘
 n new   Enter attach   ? help   q quit
```

- Dialogs render centered when they fit inside the current tier. On Compact and Minimal they use the full frame with one row of margin, and their field labels abbreviate rather than wrapping.
- Status bar display order is fixed: `n`, `d`, `R`, `Enter`, `t`, `s`, `r`, `?`, `q`. As width shrinks, drop the lowest priority first: `r`, `s`, `t`, `R`, `d`. `n`, `Enter`, `?`, and `q` are always shown.
- The table drops columns in priority order when space is tight: branch first, then diff. Name truncates with an ellipsis at a minimum of 12 columns. State and name never drop.
- `Event::Resize` forces a full redraw so tiers recalculate instantly.

Home screen:

- Left table columns: state indicator, name, branch, diff `+x -y ?u`.
- State indicator: `Working` animated Braille spinner, `Waiting` green `●`, `Exited` red `✗`, `Stopped` dim `○`, `Unmanaged` yellow `?`.
- Spinner frame derives from the app tick counter, not per-session state.
- Navigation with arrows and `h`/`j`/`k`/`l`. Selection is tracked by `SessionName`, never by row index, so rows changing state do not move the cursor.
- Right panel: live preview via `tmux capture-pane -e -p -J` on a slower timer than the state poll, `-J` to join wrapped lines. Render ANSI through `ansi-to-tui`, fall back to plain text if parsing fails. On capture failure show session metadata.
- Diffs are computed only for visible rows on the `diff_refresh_ms` cadence, never for the whole registry on every tick. Twenty sessions must not spawn forty git processes per tick.
- Status bar lists keys and transient errors with the fixed order and width-based dropping described in the responsive tiers: `n` new, `d` delete, `R` rename, `Enter` attach, `t` shell, `s` restart, `r` refresh, `?` help, `q` quit.
- Sort order: state priority `Working`, `Waiting`, `Exited`, `Stopped`, `Unmanaged`, then name. Stable across refreshes because selection is name-based.
- Terminal-too-small guard: the TooSmall tier below 36x8, see the responsive tiers above. There is no fixed 80x24 cutoff; tiers scale all the way down.
- `n` dialog: name field, repo picker, agent picker, an optional initial prompt field, and an "Add repo" row. The prompt field is single-line and submitted with the session; leaving it empty creates the session with no initial message. The picker lists `discover_repos` for the launch scope, with explicit config overrides merged in and marked. The resolved scope path is shown at the top of the picker and can be changed with a path input.
- Empty picker state: when the scope contains no repos, show "no repos found in <scope>" with two actions: "Add repo" and "Change scope". No silent fallback to `$HOME`.
- "Add repo" row: opens a path input, validates with `git -C <path> rev-parse --show-toplevel`, derives the name from the basename, appends an explicit override to the config with `toml_edit`, reloads the picker, selects the new repo. This is the fix for the claude-squad pain point. Adding is optional: scanned repos already work without config entries.
- `d` dialog: shows name, branch, worktree, diff. Dirty sessions show "N uncommitted files will be lost" and require a second confirmation. A `Working` session additionally warns "agent is actively working".
- `R` dialog: old and new name fields plus a "rename branch" toggle, on by default. The dialog shows the branch rename that will happen.
- `r`: reload config and registry from disk, refresh diffs and observations.
- `t`: open a terminal in the selected session's worktree. If the tmux session is live, create or select the shell window (identified by `@ringleader_shell`, so a user rename cannot break it) in that session, running `$SHELL` in the worktree directory, and attach the client to it, so it persists like any other tmux window. If the tmux session is stopped, suspend the TUI and run an ephemeral `$SHELL` in the worktree, returning to the TUI on exit. Uses the same terminal guard as attach. `$SHELL` is resolved from the app's environment with `sh` as fallback. Disabled for unmanaged sessions.
- `s`: restart the selected session. `Exited` sessions get `respawn-pane`; `Stopped` sessions get their tmux session recreated in the existing worktree. Disabled for `Working`, `Waiting`, and `Unmanaged` with a status-bar message.
- `q`: quit the app. tmux sessions keep running in the background; the next launch reconciles and lists them again.
- `?` help dialog: key bindings, config and log paths, version, and the Ctrl-Q behavior including the inside-tmux caveat. Centered overlay, closed with `?`, `Esc`, or `q`.
- Bell: when `bell_on_waiting` is enabled and a session transitions `Working` to `Waiting`, emit BEL through the attached client's tty, resolved from `attached_client_tty`, so it reaches the user even when the TUI's pane is hidden. If no client is attached, no bell. Track the previous state per session to detect the edge. No bell for `Exited`.
- Event loop drains `Event::Resize` and forces a full redraw, so the too-small guard clears and recalculates when the terminal grows.

Attach and Ctrl-Q:

- Detect `$TMUX`. Outside tmux: terminal guard leaves raw mode and alternate screen, runs `tmux attach-session -t <name>` as a child, restores after it returns.
- Inside tmux: run `tmux switch-client -t <name>`. The TUI keeps running and polling. `attach-session` cannot be used from inside tmux by a child process; it fails with "sessions should be nested with care".
- `t` uses the same paths: live sessions get the shell window and the outside/inside attach split, stopped sessions get an ephemeral `$SHELL` under the terminal guard. Ctrl-Q returns to the TUI from the shell window too, since the binding matches on managed session names.
- Ctrl-Q is chosen as the return key over Ctrl-D because Ctrl-D is meaningful to the agents themselves, such as EOF to exit Claude Code, so intercepting it would break the session.
- Ctrl-Q return mechanism: bind `C-q` in the `root` table while the TUI runs, restore or unbind on exit through an RAII guard plus a panic hook. The nested `if-shell` branch is passed as a quoted command string and must keep its quoting: `bind-key -T root C-q if-shell -F '#{m:<prefix>*,#{session_name}}' 'if-shell -F "#{==:#{client_last_session},}" "detach-client" "switch-client -l"' 'send-keys C-q'`. Verified on tmux 3.6a that `list-keys` preserves the nested command. The brace form (`{ ... }` passed as separate arguments) fails on 3.6a with `unknown command` or `too many arguments`; do not use it. The prefix pattern is built from the tmux prefix config at runtime. `send-keys C-q` does not recurse.
- Server bootstrap: `bind-key` needs a running tmux server. The TUI calls `ensure_server` before installing the binding, and when a new server starts later the binding is reinstalled.
- Crash safety: before installing, the adapter saves the previous root C-q binding to `$XDG_STATE_HOME/ringleader/bindings.json` via `list-keys -T root -1 C-q` (exit 0 bound, 1 unbound). At startup it checks the saved marker and restores or repairs a stale binding before reinstalling. This covers SIGKILL and power loss, which the panic hook cannot.
- The per-session `key-table` alternative is rejected, not deferred. Testing on 3.6a showed it bypasses root bindings entirely and swallows unbound keys rather than forwarding them to the pane, and it replaces the user's whole root table for managed sessions. Do not revisit it.
- Verification criteria: C-q returns to the home page from managed sessions in both outside-tmux and inside-tmux contexts, C-q still reaches non-managed sessions, and all pre-existing root bindings are restored on exit and on crash.

## 7. CLI

- `list [--json]` reconciles in memory only and never writes. It is safe to run while the TUI is running. Only the TUI and mutating commands persist the registry.
- `new --name <n> --repo <r> [--agent <a>] [--base-branch <b>] [--prompt <text>]`. `--prompt` passes an initial message to the agent via the agent's `prompt_args`. `--repo` accepts a discovered repo name or a path, resolved against `discover_repos` and then validated with `git rev-parse`.
- `delete <name> [--force]`, `--force` skips the dirty confirmation but still warns.
- `attach <name>`.
- `rename <old> <new> [--keep-branch]`.
- `restart <name>`, restarts an `Exited` or `Stopped` session. Exit 2 if not found, exit 3 if the session is live and healthy.
- `repo add <path> [--name <n>] [--base-branch <b>]` writes an explicit override, `repo remove <name>` removes an override and reports when the repo is only discovered by scanning, `repo list [--all]` lists the repos discovered for the current scope and marks explicit overrides, with `--all` also listing overrides outside the scope. `repo remove` refuses while any registry session references the repo, naming the sessions and suggesting `delete` first.
- `__exec-agent --agent <name> <command...>`, hidden. Invoked by tmux via `current_exe`. The command is the expanded agent argv, including any `prompt_args` for the initial prompt. `env_clear`s, restores the configured variable names, and execs the agent. Not documented in README.
- `doctor [--json]`, checks and reports:
  - tmux present, version >= 3.2 required for user options, `-e`, and `if-shell -F`
  - git present, version >= 2.30 required for `worktree` and `branch -m` in worktrees
  - each configured repo path resolves via `git rev-parse`
  - each agent command resolves on PATH
  - each agent with `prompt_args` has a template containing `{prompt}`
  - config path writable, state directory writable
  - registry entries whose worktree, branch, or tmux session is missing
  - registry entries whose agent no longer exists in config
  - orphaned worktrees under the worktree root that no registry entry claims
  - orphaned branches matching `branch_prefix` with no worktree
- Exit codes, documented in README and AGENTS.md: 0 success, 1 user or validation error, 2 not found, 3 conflict, including a session that is live and healthy or the lock being held.
- JSON output uses stable lowercase state strings: `working`, `waiting`, `exited`, `stopped`, `unmanaged`.
- `--config <path>` and `$RINGLEADER_CONFIG` override for tests.

## 8. Config, state, locking

Config is app-owned at `$XDG_CONFIG_HOME/ringleader/config.toml`. First run writes a commented template. Writes go through `toml_edit` so user comments and formatting survive. If the resolved config path is inside `/nix/store`, report "config is Nix-managed, cannot write" and continue read-only, never fail.

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

[[agents]]
name = "Claude"
command = "claude"
env = ["ANTHROPIC_API_KEY"]
prompt_args = ["{prompt}"]

[[agents]]
name = "opencode"
command = "opencode"
env = ["OPENAI_API_KEY"]
prompt_args = ["--prompt", "{prompt}"]
```

- `base_branch` optional. Resolution ladder: `refs/remotes/origin/HEAD`, then `main`, then `master`, then, for a repo with an unborn HEAD, `git worktree add -b <branch>` with no start point (git treats it as an orphan). Otherwise a clear error asking for an explicit `base_branch`. Local refs only, no fetch ever. Resolution is lazy, only for the repo the user actually picks.
- `state_poll_ms` drives observation, classification, and the spinner. Preview and diff refresh are slower, and diffs are computed for visible rows only.
- Repo scanning: scope resolves from the launch directory by walking up to the nearest `.git` directory, or the launch directory itself when there is none. The walk is depth-limited by `scan_depth` (default 3), prunes `node_modules`, `target`, `vendor`, `.cache`, and `dist`, treats a `.git` directory as a repo and stops descending, skips `.git` files (worktrees and submodules), does not follow symlinks, and ignores permission errors. Results are deduped by canonical path and cached in memory per picker session; `r` re-scans.
- `env` lists variable names. At session creation the app copies their values from its own environment into `tmux -e` arguments for the launcher, which `env_clear`s and restores only those names before execing the agent. Missing variables produce a warning, not a failure. Values are never logged.
- `prompt_args` is the argument template for an initial prompt, with `{prompt}` substituted as one argv element. Claude Code uses `["{prompt}"]` (positional), opencode uses `["--prompt", "{prompt}"]`. Both were verified against the installed versions (Claude Code positional prompt; opencode 1.18.30 `--prompt`). A prompt containing multiple words or quotes stays one argv element, never split or shell-evaluated; `tmux new-session` accepts argv directly, so the launcher receives it intact. When the new-session prompt field is empty, no prompt arguments are appended, regardless of the template. An agent without `prompt_args` cannot receive an initial prompt and the UI says so.
- `~` in `worktree_root` and `Repo.path` expands to the home directory at load. Resolution order for the config file: `--config` flag, then `$RINGLEADER_CONFIG`, then `$XDG_CONFIG_HOME/ringleader/config.toml` (falling back to `~/.config`). The first match wins, no merging.
- Config validation on load: `tmux_prefix` must be a legal tmux session name, `branch_prefix` a legal ref prefix, repo and agent names unique, worktree root writable. No repo is required, since scanning supplies them. An empty or first-run config is valid and must not block startup; strict validation applies to mutating commands. First run seeds the template with the two default agents, `claude` and `opencode`, so `n` works immediately.
- Registry at `$XDG_STATE_HOME/ringleader/sessions.json` with a schema version field. Writes are atomic: temp file in the same directory, then rename. Schema policy: refuse a newer version than the binary with a clear message; keep a corrupt file as `sessions.json.corrupt-<timestamp>` and continue with an empty registry plus a status-bar warning. No migration machinery in v1, since there is no older schema yet.
- The registry is an index, not the source of truth. tmux user options are the source of truth for a live session:
  - `@ringleader_meta` holds one compact JSON object with repo name, repo path, branch, worktree path, and agent.
  - `create_session` writes it, `rename_session` updates it.
- `observe_all` reads `window_activity` from `list-panes -a -F` (epoch seconds, 1-second granularity) alongside `pane_dead`. `pane_last_output_time` is unavailable on tmux 3.6a; `pane_activity` is not a format variable. `@ringleader_meta` reads in the same `list-sessions` call using `-F '#{session_name}\t#{@ringleader_meta}'`; user option expansion was verified working.
- Startup reconciliation merges registry entries and live tmux sessions matching `tmux_prefix`:
  - live session, valid metadata, no registry entry: adopt into the registry
  - live session, missing or malformed metadata: state `Unmanaged`, attachable only, never edited, restarted, or deleted; warned about in the status bar
  - registry entry with no live session: keep, shown as `Stopped`, restartable in place
  - adopted sessions obey the same uniqueness rules as created ones
- `observe_all` treats a missing tmux server (nonzero `list-sessions` with "no server running") as zero sessions, not an adapter error. On that path every registry session is simply `Stopped`.
- Locking with `fs4` on `$XDG_STATE_HOME/ringleader/lock`:
  - The TUI takes an exclusive lifetime lock at startup. A second TUI exits with a clear message naming the lock path.
  - CLI mutations take a short-lived exclusive lock. If the TUI holds it, the command exits 3 with "another instance is running".
  - Read-only commands such as `list` and `doctor` do not lock and do not write the registry.
- The registry write path is single-writer: the TUI lifetime lock is held while it persists, and CLI mutations persist under the short-lived write lock. `list` never persists, eliminating the race between a lock-free read and a writer.
- Dependencies: `ratatui`, `crossterm`, `serde`, `serde_json`, `toml`, `toml_edit`, `thiserror`, `anyhow` (main only), `clap`, `dirs`, `log`, `env_logger`, `ansi-to-tui`, `fs4`. Dev: `tempfile`. No mocking crate.

## 9. Logging and error handling

- `log` + `env_logger`, default level info, file at `$XDG_STATE_HOME/ringleader/app.log`, `--log-level` flag. If the file exceeds 5 MB at startup, truncate it. No rotation machinery in v1.
- Adapter errors are wrapped with `thiserror` and surfaced in the status bar. The UI never panics on adapter failure.
- Panic hook restores the terminal, releases the lock, and writes the panic to the log. Terminal state and the lock live in RAII guards so normal exit always restores them.

## 10. Testing and fixtures

- Domain: classifier boundaries (missing, dead, malformed metadata as `Unmanaged`, activity exactly at threshold, stale, fresh), name validation and slugging including the 64-char cap and slug collisions, `git check-ref-format --branch` validation, diff accumulation, the `Working` to `Waiting` transition edge used by the bell.
- Application: hand-written `FakeSessionRegistry`, `FakeWorkspace`, `FakeMultiplexer`, `FakeConfigSource`, `FakeRepoScanner`, `FakeLock` implementing the ports, shared from `src/test_support/`. Create, duplicate rejection, dirty delete guard, rename with branch, rename rollback, restart for `Exited` and `Stopped`, restart refusal for live and unmanaged, doctor findings, CLI service paths. Prompt expansion: template with and without prompt, multi-word prompt kept as one argv element, missing `prompt_args` refused, restart never replays the prompt. Reconciliation: adoption of an unknown live session, malformed metadata becomes `Unmanaged` and is never persisted as managed, registry-only entries stay `Stopped`, adoption respects uniqueness. `open_shell` returns an attach for live sessions and an ephemeral request for stopped ones, and is refused for unmanaged sessions. Discovery: scope from a repo subdirectory, scope from a plain directory, explicit overrides winning on canonical path, scanned and explicit entries deduped, sessions never filtered by scope.
- Adapters with fixtures under `tests/fixtures/`, fake data only:
  - `config.toml`, `sessions.json`
  - `tmux-list-panes.txt` for the `observe_all` parser, including a dead pane and a `window_activity` value
  - `tmux-list-sessions-meta.txt` for reconciliation, including a valid record and a malformed one
  - `tmux-list-windows-shell.txt` for `ensure_shell_window`, including an existing `@ringleader_shell` window and a renamed shell window
  - `tmux-bindings.txt` for saving and restoring the previous root C-q binding, including the unbound case
  - `tmux-capture-pane-ansi.txt` for the ANSI preview fallback, plus `tmux-capture-pane-ansi-wrapped.txt` for a wrapped colored line, where `-J` can misalign ANSI
  - `git-numstat.txt` and `git-untracked.txt` for diff parsing
- Git adapter tests run real git in `tempfile` repos. tmux tests detect the binary and skip when absent.
- Repo scanner tests build a `tempfile` directory tree with fixture repos: nested repos, a `.git` file worktree that must be skipped, a pruned directory (`node_modules`), a depth-4 repo that must be excluded at depth 3, and a regular directory that is not a repo. Scanner tests do not touch the real filesystem outside tempdirs.
- All tests are unit tests inside `src/` (`#[cfg(test)] mod tests`, or `src/test_support/`). `tests/` holds fixtures only. Do not add `tests/*.rs` integration tests; a single binary crate has no library target for them to link. If that changes, a `lib.rs` must be introduced deliberately.
- Tmux integration tests, including Ctrl-Q binding installation and restore, run against a private server on a temp socket (`tmux -L ringleader-test`), never the user's default server. CI runs them headless the same way.
- UI: `TestBackend` render assertions for the spinner glyph, the `●` bullet, the `Unmanaged` marker, each responsive tier at representative sizes, the resize redraw, the help overlay, and the delete confirmation.
- One concept per test, public interfaces only.

## 11. Tooling and CI

- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, all through `nix develop` so local and CI use the same rustc 1.95.
- `.github/workflows/ci.yml` on Ubuntu: DeterminateSystems nix installer, then `nix develop -c cargo fmt --check`, `nix develop -c cargo clippy ...`, `nix develop -c cargo test`, and `nix build .#` plus `nix flake check`. No separate toolchain action, the dev shell is the single source of truth.
- The workflow file is authored and committed, but it stays dormant until the owner creates the GitHub repository and pushes. Do not add CI badges to the README until then.

## 12. Flake for this repo

- Input `nixpkgs` pinned at `nixos-26.05` for stability.
- `packages.default = rustPlatform.buildRustPackage` with `cargoLock.lockFile`, wrapped with `makeWrapper --prefix PATH : ${lib.makeBinPath [ tmux git ]}` so `nix run` works standalone.
- `doCheck = true` with `nativeCheckInputs = [ git tmux ]`, so `nix build` runs the test suite, including the tmux tests on a private socket.
- `devShells.default`: rustc, cargo, clippy, rustfmt, tmux, git, nixfmt.
- `formatter = nixfmt-tree`, `checks` build the package. Commit `flake.lock`.

## 13. Consumer integration

Out of scope for this repo. The flake's `packages.default` and `devShells.default` are the interface. Any Nix consumer can add the flake as an input and install the binary. Integration work happens outside this repository, so nothing here may reference a specific downstream setup or configuration.

## 14. Documentation split

- `README.md`: for humans. Terse and minimal. What it is, install (`nix run`, `nix profile install`, add as a flake input), first run, key bindings table, CLI synopsis. Generic examples only. No architecture, no agent instructions, no CI badge until the repo exists.
- `AGENTS.md`: the agent repo documentation and the living reference. It must embed a copy of section 3's tree, because it has to be self-contained; `docs/plans/v1-bootstrap.md` is the historical plan and is not a substitute. Layout, the one-primary-type-per-file rule and the fact that the tree is the file contract, port rules, OOP conventions (private fields, `impl` blocks, constructor injection, no free functions), the strict comment policy (comments are failures, fix the code, list survivors with their clean-code exception in each commit body), naming rules, CLI and TUI wiring boundaries, prose capitalization, exact commands (`nix develop`, cargo test/clippy/fmt), test strategy and fixtures, commit conventions, the local-only no-remote rule, the plan storage convention below, and the rule that README stays terse and human-facing.
- Plan storage convention for future work: one file per effort at `docs/plans/YYYY-MM-DD-slug.md`, with a `> Status:` header (`active`, `completed`, or `abandoned`). `docs/plans/v1-bootstrap.md` is the v1 history.

## 15. Milestones, one commit each

1. Scaffold cargo project, rustfmt, MSRV confirmed against the dev shell, flake dev shell with `doCheck`, CI skeleton.
2. Add domain session model and state classifier with tests.
3. Add application ports and fakes.
4. Add session service use cases with tests.
5. Add git worktree adapter with tempfile tests, base branch ladder, and removal fallback.
6. Add tmux adapter on a private test socket, and validate the Ctrl-Q binding forms from section 6 against the pinned tmux before wiring them.
7. Add JSON registry with atomic writes, corruption recovery, schema policy, and single-instance locking.
8. Add the repo scanner adapter with tempfile tree tests, cwd scope resolution, and explicit override merging.
9. Add a thin vertical slice: minimal session table, attach, terminal guard, and `q` quit.
10. Add tmux metadata mirroring, reconciliation, adoption, `Stopped` restart, the `__exec-agent` launcher, and `prompt_args` expansion.
11. Add config loader with validation, first-run template, and `toml_edit` writes.
12. Add CLI subcommands: list, new, delete, attach, rename, repo, doctor, plus exit codes and JSON state strings.
13. Render working spinner and waiting bullet.
14. Add the `t` shell action, live via `shell` window and stopped via ephemeral `$SHELL`.
15. Add new session dialog with repo picker, scope header, initial prompt field, and empty state.
16. Add "Add repo" row with `git rev-parse` validation.
17. Add delete confirmation with dirty guard.
18. Wire the Ctrl-Q return binding into attach, with saved-binding restore and server bootstrap.
19. Add rename flow with branch rename and rollback.
20. Refine live preview with `ansi-to-tui`, `-J`, and visible-row diff computation.
21. Add logging and status bar error surfacing.
22. Add bell on Working to Waiting transition.
23. Add in-TUI help dialog.
24. Add README and AGENTS.md, with the tree copied into AGENTS.md.
25. Finish GitHub Actions.

## 16. Risks to verify early

- `rm -rf` fallback is a last resort: try `git worktree remove --force` after the dirty guard first, since it handles submodule worktrees. Only if that fails, fall back to `rm -rf` plus `git worktree prune`.
- Ctrl-Q root binding conflicts with the user's tmux config. Restore correctness on crash is handled by the saved-binding state plus startup repair; `key-table` is rejected and documented as such.
- The inside-tmux attach path uses `switch-client`, never `attach-session` from a child process.
- Server bootstrap before `bind-key`, and reinstall when a new server starts.
- `window_activity` has 1-second granularity, so `activity_idle_seconds` below 1 is meaningless.
- Detached session sizing (`-x/-y`) must match what preview expects, so `capture-pane -J` output is not clipped at 80 columns.
- Terminal raw mode and alternate screen restore around `tmux attach`, moved early as the milestone 9 vertical slice.
- Resize events must force a redraw so tiers and the too-small guard recalculate instantly.
- Unborn-HEAD repos fall back to an orphan worktree start point instead of erroring on the base-branch ladder.
- Initial prompt argv must stay one element per template entry; a prompt with quotes or shell metacharacters must reach the agent intact. Both Claude Code and opencode were verified to accept it, but agents change their flags; `doctor` should warn when a configured `prompt_args` template lacks `{prompt}`.
- Branch rename rollback ordering in `rename_session`.
- Dirty worktree removal and branch deletion safety, including refusing delete on a `Working` session.
- Activity heuristic false positives if an agent paints an idle animation, tunable via `activity_idle_seconds`.
- `ansi-to-tui` parsing of capture output, especially wrapped colored lines; always keep the plain text fallback.
- Preview load: only the selected session is captured, diffs are computed for visible rows only, both slower than the state poll.
- Lock contention between a running TUI and CLI mutations, make the message actionable, exit 3.
- User option expansion in `list-sessions -F` was verified working; keep the per-session `show-options -qv` fallback as a defensive path.
- Adoption when the worktree was deleted manually: adopt the session, surface the missing worktree in `doctor` and the status bar.
- Global `destroy-unattached` in the user's tmux config is why creation and configuration happen in a single tmux invocation.
- tmux server environment drift: `-e` does not strip the inherited server environment, so the `__exec-agent` launcher owns the real allowlist.
- Registry `list` race: `list` reconciles in memory and never persists, single-writer elsewhere.
- Repo scan scope: launched from `$HOME` or `/` the walk could be large. Mitigated by the depth cap, pruning, and the empty state; consider a hard cap on visited directories with a status-bar notice if the scan exceeds it.
- Repo name collisions between scanned and explicit entries, resolved by canonical path with explicit winning; names must still be unique for the picker display.
- Submodule repos and `git worktree remove --force`; try `--force` first, `rm -rf` only as a last resort.
- Agent command resolution under Nix, wrapped PATH plus absolute commands in config.

## 17. Out of scope for v1

- Mouse support.
- Configurable keybindings.
- `doctor --fix` auto-repair.
- Registry schema migration machinery; only refuse-newer and corrupt-file backup.
- Log rotation; truncate on startup instead.
- The per-session `key-table` Ctrl-Q alternative; rejected after testing on 3.6a.
- Agent-specific activity detectors beyond tmux pane activity.
- Session output logging or transcript search, agent tooling owns that.
