# Ringleader agent guide

This file is the living reference for agents working in this repository.

## Ground rules

- Keep functions small, each doing one thing at one level of abstraction, with callers above the functions they call. Avoid flag arguments and side effects a name does not announce, keep commands separate from queries, and state each piece of knowledge once. Talk to direct collaborators instead of reaching through chains of objects. Errors use `Result` with the operation that failed in their message, and absence uses `Option`.
- Comments are failures to express intent in code. Every comment is a signal, not documentation: revisit the code it explains and improve it until the comment is unnecessary, then delete it. Rename the variable, extract the function, introduce the type, or restructure the control flow. Never add a comment to justify messy code or to stand in for a better name. The only surviving comments are these exceptions: legal headers, non-obvious intent that cannot be encoded in structure, warnings of real consequences, and TODOs that reference a tracked issue. Doc comments are comments and follow the same rule. When a comment seems to survive, stop and justify it to the project owner before committing, and never announce it in the commit body. If the justification against an exception is not convincing, delete the comment and fix the code. A commit that adds an unjustified comment is not done.
- Write object-oriented Rust: state in structs with private fields, behavior in `impl` blocks, traits as interfaces, composition over inheritance, dependencies injected through constructors. No public data fields and no free functions except `main` and `#[test]` functions, which Rust requires to be free. A test helper that is not a `#[test]` function is a method or associated function of a fixture struct. A fixture struct used by one file's tests lives in that file's `mod tests`, and one shared across files lives in `src/test_support/` with a tree entry.
- Names reveal intent. Types are nouns, methods that act are verbs, and accessors are named after what they return. No abbreviations, no number series, no noise words, and no magic numbers or strings: a value whose meaning is not obvious becomes a named constant. Files are named after their primary type in snake case.
- When a type's role matches an established pattern, name it after that pattern instead of inventing a name. The sign to look for one is a first name that is a gerund, or that says how the type works rather than what role it plays. User-configured actions that run when an event happens are hooks, so the type that runs them is `AgentHooks` with `after_start` and `after_rename`, not `AgentTyping`. This codebase already names adapters, guards, plans, requests, fakes, and fixtures this way. When no pattern fits, name the type after its domain role. Never reshape a type to fit a pattern, and never give a type a pattern name for a role it does not play.
- Build, test, and run every tool inside `nix develop`. The host `PATH` may carry other versions of `cargo`, `rustc`, and `tmux`, so never rely on what the host provides. `.envrc` loads the same dev shell through direnv with `use flake` once `direnv allow` has been run in the repository. The commands below keep `nix develop -c`, so they also work without direnv.
- Stage each new file by path with `git add` before running any `nix` command, because `nix develop`, `nix build`, and `nix flake check` ignore files outside the git index.
- Every ad-hoc tmux command, such as a binding experiment, runs as `env -u TMUX -u TMUX_PANE tmux -u -S <tempdir>/x.sock -f /dev/null ...` and ends with `kill-server` on that socket. `nix develop` passes `$TMUX` through, so a bare `tmux` command reaches the user's live server. Never touch the default server or the server named in `$TMUX`. Keep the socket path under 100 bytes, because tmux rejects a longer one with `File name too long`. Use a directory from `mktemp -d` under `/tmp`.
- Every manual run of the `ringleader` binary, TUI or CLI, uses throwaway state, because production `TmuxCommand` passes no socket or config arguments and `main` resolves the real XDG paths.
  - Create `<tempdir>` with `mktemp -d /tmp/rl.XXXXXX`, and inside it `tmux-<uid>` with mode 0700 (`<uid>` from `id -u`), `config/ringleader`, `state`, `data`, `run`, `repos`, and `home`.
  - The prefix is `env -u TMUX -u TMUX_PANE -u RINGLEADER_CONFIG TMUX_TMPDIR=<tempdir> XDG_CONFIG_HOME=<tempdir>/config XDG_STATE_HOME=<tempdir>/state XDG_DATA_HOME=<tempdir>/data XDG_RUNTIME_DIR=<tempdir>/run HOME=<tempdir>/home SHELL=sh` followed by the hermetic git environment below. It sends Ringleader's tmux calls to the private server at `<tempdir>/tmux-<uid>/default`, and it keeps the owner's shell and git startup files out of the run.
  - Every tmux command typed for that server uses the prefix and also passes that path with `-S`, because tmux silently uses the user's default server when `TMUX_TMPDIR` names a missing directory.
  - Before the binary runs, start the server with the prefix and `tmux -u -S <tempdir>/tmux-<uid>/default -f /dev/null new-session -d -s holder sh`, so it never loads the user's tmux config or login shell.
  - Before the first run, write `<tempdir>/config/ringleader/config.toml` with one stand-in agent, `name = "stand-in"` and `command = "sh"`, never `claude` or `opencode`.
  - Build fixture repos under `<tempdir>/repos` with the hermetic git environment, and run the binary by absolute path with `<tempdir>/repos` as the working directory, so no real repository is in scope.
  - An outside-tmux check runs the binary with the prefix in a pane of a second server started under the ad-hoc tmux rule, and drives it with `send-keys` on that server. An inside-tmux check runs it in a pane of the `holder` server with the prefix minus `-u TMUX -u TMUX_PANE`.
  - End with `kill-server` on each socket by explicit `-S` path, then remove `<tempdir>`. Never run the binary against the user's tmux server, config, state, data, or repositories.
- The hermetic git environment is `GIT_CONFIG_GLOBAL=/dev/null`, `GIT_CONFIG_NOSYSTEM=1`, `HOME=<tempdir>`, and fake `GIT_AUTHOR_NAME`, `GIT_AUTHOR_EMAIL`, `GIT_COMMITTER_NAME`, and `GIT_COMMITTER_EMAIL` values. Fixture repos are created with `git init -b <branch>`, never relying on `init.defaultBranch`.
- The private GitHub repository exists and is configured as `origin`. Do not create another repository or add another remote. Push only when the project owner asks.
- Keep the repository consumer-agnostic. Never reference or assume any specific downstream setup, local directory layout, or personal configuration. Examples and fixtures use generic names such as `example-project`.
- Human-facing prose uses "Ringleader". Identifiers stay lowercase: binary, crate, repo, flake input, config paths, the tmux prefix, JSON fields. The git config marker `ringleaderManaged` keeps git's camelCase.

## Commands

```sh
nix develop -c cargo fmt --check
nix develop -c cargo clippy --all-targets -- -D warnings
nix develop -c cargo test
nix build .#
nix flake check
```

These are the commands `.github/workflows/ci.yml` runs. A change is done when all of them pass.

## Metrics

Local, on-demand code-health tools. They are never part of CI. Do not add them to
`checks`, to the default dev shell, or to `.github/workflows/ci.yml`. The flake
builds them as `apps` so `nix flake check` only evaluates them, and puts them in
the `metrics` dev shell, which extends the default one. `dryer`, `crapper`, and
`mutator` write snapshots under `.metrics/`, which is gitignored.
Run them from the repository root; a scoped run reads `git status` for `--changed`.
Pass `crapper` the `src` path. A pathless run also walks nested checkouts such as
`.claude/worktrees`, reruns coverage for each, and scores every copy under the
same namespaces.

```sh
nix develop .#metrics -c dryer --changed       # duplicate candidates; writes .metrics/dry.edn
nix develop .#metrics -c crapper src           # CC plus coverage plus CRAP; writes .metrics/crap.edn
nix develop .#metrics -c mutator --max-workers N src/<module>.rs
nix run .#dryer -- --changed                   # the apps also work outside any dev shell
nix run .#crapper -- --help
nix run .#mutator -- --help
```

`mutator` runs every covered mutant and many tests start real tmux servers. Never
run it whole-repo. Name one module or file and cap `--max-workers`. Once a
snapshot exists, a rerun only runs survivors and sites in changed functions.
Never pass `--changed` with a path: it drops the path and mutates every changed
file in `git status`. `crapper` and `mutator` need `cargo-llvm-cov`
and the LLVM tools named by `LLVM_COV` and `LLVM_PROFDATA`, which the `metrics`
shell and the apps set. When `cargo llvm-cov` fails, whether from a missing tool
or one failing test, every function scores 0% coverage and the run still exits 0.
Check the output for `Rust coverage will score 0%` before trusting the snapshot.
This crate is bin-only, so a scoped `mutator` test command is
`--test-command "cargo test --bin ringleader <module::path>"`.

uml-viewer reads `.metrics/crap.edn` and `.metrics/mutate/`. It ignores
`.metrics/dry.edn`, so `dryer` output stays a text report and never reaches the
diagram. It needs a display and downloads Maven dependencies on first run. Write
the policy to the gitignored `.uml-viewer/ringleader.policy.edn`:

```edn
{:title "Ringleader"
 :sources [{:lang :rust :root "src" :prefix "ringleader"}]
 :hierarchical true
 :out ".uml-viewer/ringleader.edn"}
```

Leave the policy-level `:prefix` unset. The overlay joins a `crate::module::Type`
snapshot to the module class through the class id's crate path, and stripping the
crate name from the id breaks that join. `$UML_VIEWER_DEPS`, set by the `metrics`
shell, points the Clojure CLI at the `uml-viewer` flake input. Inside
`nix develop .#metrics`, generate the IR, then open the viewer:

```sh
clojure -Sdeps "$UML_VIEWER_DEPS" -M:ir .uml-viewer/ringleader.policy.edn
viewer_tmux_dir=$(mktemp -d /tmp/uml.XXXXXX)
env -u TMUX -u TMUX_PANE TMUX_TMPDIR="$viewer_tmux_dir" \
  clojure -Sdeps "$UML_VIEWER_DEPS" -M:run --restart .uml-viewer/ringleader.edn
rm -rf "$viewer_tmux_dir"
```

Always pass `--restart`. Without it the viewer starts `grok --yolo --trust` as its
companion in a new session on the user's tmux server, and opens on a blank canvas
until that companion answers. `--restart` skips the companion and loads the EDN
at once. It still looks for a companion with `tmux has-session`, so the `env`
prefix sends those lookups to an empty private directory, removed after the
viewer exits.

## Layout

```
.
├── Cargo.toml              # rust-version = "1.95", edition = "2024", license = "MIT"
├── Cargo.lock
├── rustfmt.toml            # max_width = 100
├── LICENSE                 # MIT
├── .gitignore              # /target, /result, /result-*, /.direnv, /.claude/worktrees, /.metrics, /.uml-viewer
├── flake.nix
├── flake.lock
├── .envrc                  # direnv: use flake, loads the dev shell
├── .github/workflows/ci.yml
├── README.md               # human readers, terse
├── AGENTS.md               # agent repo documentation, living
├── tests/fixtures/         # fake config, registry, tmux and git output
└── src/
    ├── main.rs             # fn main and CompositionRoot
    ├── cli.rs              # Cli struct, clap definitions, dispatch
    ├── cli/
    │   ├── cli_output.rs         # CliOutput, writeln! output that stops at a broken pipe
    │   ├── cli_session.rs        # CliSession, signal checks, worker waits, and the exit of one command
    │   ├── session_commands.rs   # SessionCommands: list, new, delete, attach, rename, restart
    │   ├── list_report.rs        # ListReport, the list text and JSON
    │   ├── repo_argument.rs      # RepoArgument, the --repo name or path resolution
    │   ├── repo_commands.rs      # RepoCommands: repo add, remove, list
    │   └── delete_report.rs      # DeleteReport, the delete warnings and results
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
    │   ├── repo_draft.rs         # RepoDraft, a [[repos]] entry before the load checks
    │   ├── base_branch.rs        # BaseBranch
    │   ├── config.rs             # Config, the parsed config.toml
    │   ├── config_draft.rs       # ConfigDraft, the raw config values the Config constructor checks
    │   ├── config_checks.rs      # ConfigChecks, the checks every config load runs
    │   └── naming_scheme.rs      # NamingScheme, the worktree root and tmux prefix a session records
    ├── application/
    │   ├── mod.rs
    │   ├── session_service.rs    # SessionService, the facade the UI and CLI call
    │   ├── service_context.rs    # ServiceContext and LoadedConfig, what main injects into the facade
    │   ├── service_ports.rs      # ServicePorts with the StatePorts, RepoPorts, and TmuxPorts bundles
    │   ├── service_error.rs      # ServiceError and its exit codes
    │   ├── session_lifecycle.rs  # SessionLifecycle: create, delete, rename, restart
    │   ├── session_creation.rs   # SessionCreation: create_session with its rollbacks
    │   ├── worktree_rollback.rs  # WorktreeRollback, removes the worktree and branch of a failed create
    │   ├── creation_plan.rs      # CreationPlan and CreateRequest
    │   ├── use_checks.rs         # UseChecks, the checks a create or a restart runs before acting
    │   ├── session_renaming.rs   # SessionRenaming: rename_session with its branch rollback
    │   ├── rename_request.rs     # RenameRequest, the old and new names and whether the branch stays
    │   ├── branch_availability.rs # BranchAvailability, refuses a branch name that exists or holds other branches, for create and rename
    │   ├── session_restart.rs    # SessionRestart: restart of an Exited or Stopped session
    │   ├── agent_hooks.rs        # AgentHooks, types an agent's startup and rename input after a start or a rename
    │   ├── worktree_inspection.rs # WorktreeInspection, the worktree checks restart, delete, and doctor share
    │   ├── session_deletion.rs   # SessionDeletion: delete_session steps 5 to 8 and forget
    │   ├── deletion_guards.rs    # DeletionGuards: delete_session steps 2 to 4
    │   ├── branch_ownership.rs   # BranchOwnership, refuses to delete or rename a branch without the Ringleader marker
    │   ├── delete_impact.rs      # DeleteImpact and DeleteConfirmation
    │   ├── delete_outcome.rs     # DeleteOutcome
    │   ├── worktree_containment.rs # WorktreeContainment, shared by delete step 3 and reconciliation
    │   ├── session_monitor.rs    # SessionMonitor: reconcile, observe, diffs, preview, bell
    │   ├── session_reconciler.rs # SessionReconciler, pairs registry entries with live tmux sessions
    │   ├── reconciliation.rs     # Reconciliation, the reconciled entries and rows
    │   ├── session_access.rs     # SessionAccess: attach, open_shell, return binding
    │   ├── repo_catalog.rs       # RepoCatalog: discover, find, add, and remove repos
    │   ├── repo_merge.rs         # RepoMerge, joins configured and scanned repos, configured entries winning
    │   ├── home_repo_search.rs   # HomeRepoSearch, the repos under home the repo search offers
    │   ├── default_repo.rs       # DefaultRepo, the launch repo or the newest session's repo
    │   ├── typed_paths.rs        # TypedPaths, a typed path read as a directory or as a repo
    │   ├── repo_overrides.rs     # RepoOverrides: the add_repo and remove_repo workers
    │   ├── pending.rs            # Pending, a worker result the TUI polls and the CLI waits on
    │   ├── doctor.rs             # Doctor
    │   ├── diagnostic_paths.rs   # DiagnosticPaths, the program and state paths doctor and help report
    │   ├── tool_version.rs       # ToolVersion, the major and minor numbers of a tmux or git version
    │   ├── doctor_report.rs      # DoctorReport, the doctor's info, warning, and problem lines
    │   ├── registry_audit.rs     # RegistryAudit, the doctor's checks of each registry entry
    │   ├── orphan_audit.rs       # OrphanAudit, the doctor's search for worktrees and marked branches no session claims
    │   └── ports/
    │       ├── mod.rs
    │       ├── session_registry.rs   # SessionRegistry trait
    │       ├── workspace.rs          # Workspace trait
    │       ├── branch_marker.rs      # BranchMarker, the git config entry that marks a branch Ringleader created
    │       ├── dirty_state.rs        # DirtyState, the uncommitted files and nested repos of a worktree
    │       ├── unmerged_commits.rs   # UnmergedCommits, commits on no other ref with the tip and HEAD
    │       ├── multiplexer.rs        # Multiplexer trait
    │       ├── return_binding.rs     # ReturnBinding trait
    │       ├── config_source.rs      # ConfigSource trait
    │       ├── repo_scanner.rs       # RepoScanner trait
    │       ├── host_probe.rs         # HostProbe trait
    │       ├── clock.rs              # Clock trait
    │       └── lock.rs               # Lock trait
    ├── adapters/
    │   ├── mod.rs
    │   ├── agent_launcher.rs     # AgentLauncher, the hidden __exec-agent mode, owns AgentHandoff and the exec seam
    │   ├── agent_handoff.rs      # AgentHandoff, the handoff file a launch writes and the launcher takes
    │   ├── app_log.rs            # AppLog, the append-mode app.log behind env_logger
    │   ├── process_environment.rs # ProcessEnvironment, the environment snapshot main captures once
    │   ├── xdg_paths.rs          # XdgPaths, the config, state, data, and runtime directories
    │   ├── directory_repo_scanner.rs # DirectoryRepoScanner
    │   ├── mount_table.rs        # MountTable, the network and FUSE mount points the scan skips
    │   ├── toml_config_source.rs # TomlConfigSource
    │   ├── raw_config.rs         # RawConfig, the config.toml shape before the load checks
    │   ├── config_file_writer.rs # ConfigFileWriter, the read-only checks and mode-keeping config write
    │   ├── json_session_registry.rs # JsonSessionRegistry
    │   ├── file_lock.rs          # FileLock
    │   ├── bounded_command.rs    # BoundedCommand, runs git and tmux children in a new session with a deadline
    │   ├── command_output.rs     # CommandOutput, a finished child's status and output
    │   ├── host_path.rs          # HostPath, lenient canonicalization and directory listing
    │   ├── git_workspace.rs      # GitWorkspace
    │   ├── git_runner.rs         # GitRunner, runs a GitInvocation through BoundedCommand
    │   ├── git_invocation.rs     # GitInvocation, one git call with its child environment
    │   ├── git_porcelain.rs      # GitPorcelain, parses git's machine-readable output
    │   ├── git_branches.rs       # GitBranches: compare-and-swap delete, rename, and the managed-branch marker
    │   ├── start_point_resolver.rs # StartPointResolver, the base branch ladder
    │   ├── submodule_commits.rs  # SubmoduleCommits, the submodule walk of unmerged_commits
    │   ├── system_clock.rs       # SystemClock, reads `SystemTime`
    │   ├── system_host_probe.rs  # SystemHostProbe
    │   ├── live_process.rs       # LiveProcess, one /proc entry
    │   ├── signal_flags.rs       # SignalFlags, the SIGHUP, SIGINT, SIGTERM, and SIGQUIT flags
    │   ├── durable_file.rs       # DurableFile, synced temp file renamed over the target
    │   ├── tmux_command.rs       # TmuxCommand, socket and config args, `-u` and `-N`, child environment, labeled output, `#` and `;` escaping, CallerProcess
    │   ├── caller_process.rs     # CallerProcess, the parent chain is_inside_client reads from /proc
    │   ├── tmux_error.rs         # TmuxError
    │   ├── tmux_sequence.rs      # TmuxSequence, `;`-separated commands with `#` and `;` escaping
    │   ├── tmux_output.rs        # TmuxOutput, labeled lines and the zero-session errors
    │   ├── tmux_scripts.rs       # TmuxScripts, every tmux command sequence the multiplexer sends, exact targets, start directories
    │   ├── tmux_launch.rs        # TmuxLaunch, ringleader-created and ringleader-done handling, rollback, tag move
    │   ├── tmux_agent_input.rs   # TmuxAgentInput, waits for the agent's prompt and types lines into its pane
    │   ├── tmux_listing.rs       # TmuxListing, parses list-panes, list-windows, and list-clients rows and @ringleader_meta
    │   ├── tmux_multiplexer.rs   # TmuxMultiplexer
    │   ├── tmux_return_binding.rs # TmuxReturnBinding, owns bindings.json
    │   ├── binding_records.rs    # BindingRecords, the bindings.json records
    │   └── key_binding.rs        # KeyBinding, parses and compares list-keys lines
    ├── ui/
    │   ├── mod.rs
    │   ├── app.rs                  # App
    │   ├── action_gate.rs          # ActionGate, which actions the selected row allows
    │   ├── home_state.rs           # HomeState, the polled rows, selection, and preview
    │   ├── session_list.rs         # SessionList, sorted rows and name-based selection
    │   ├── preview_feed.rs         # PreviewFeed, the preview capture and its timer
    │   ├── diff_queue.rs           # DiffQueue, round-robin diffs of visible rows
    │   ├── bell.rs                 # Bell, rings on a Working to Waiting edge
    │   ├── shell_runner.rs         # ShellRunner, runs the ephemeral t shell
    │   ├── blocking_operation.rs   # BlockingOperation, a create, delete, rename, or restart run behind a busy message
    │   ├── status_report.rs        # StatusReport, the status bar message and how outcomes become one
    │   ├── text_fit.rs             # TextFit, padding and ellipsis truncation
    │   ├── event_loop.rs           # EventLoop
    │   ├── terminal_guard.rs       # TerminalGuard
    │   ├── home_view.rs            # HomeView
    │   ├── empty_state.rs          # EmptyState, the no-sessions message with the logo above it when it fits
    │   ├── session_table.rs        # SessionTable, the home screen's session list
    │   ├── table_columns.rs        # TableColumns, column widths and dropping
    │   ├── preview_panel.rs        # PreviewPanel, the Full tier's preview
    │   ├── preview_text.rs         # PreviewText, the captured rows that fit the preview
    │   ├── help_view.rs            # HelpView
    │   ├── status_bar.rs           # StatusBar
    │   ├── theme.rs                # Theme
    │   └── dialogs/
    │       ├── mod.rs
    │       ├── dialog.rs                # Dialog, the open dialog, its key routing and placement
    │       ├── dialog_slot.rs           # DialogSlot, the dialog App shows, if any
    │       ├── dialog_labels.rs         # DialogLabels, full or abbreviated field labels
    │       ├── text_field.rs            # TextField, typing and paste rules for name and path fields
    │       ├── new_session_dialog.rs    # NewSessionDialog
    │       ├── new_session_view.rs      # NewSessionView, draws the n dialog's fields, error, and hints into its area
    │       ├── new_session_layout.rs    # NewSessionLayout, the rows the n dialog gives its repo search, prompt, and error
    │       ├── prompt_field.rs          # PromptField, the n dialog's multiline prompt on ratatui-textarea
    │       ├── new_session_field.rs     # NewSessionField, the n dialog's fields in Tab order with their labels and hints
    │       ├── repo_picker.rs           # RepoPicker, the n dialog's Repo field and its search
    │       ├── chosen_repo.rs           # ChosenRepo, the default repo until the search picks another
    │       ├── repo_finder.rs           # RepoFinder, the fuzzy search over the repos under home
    │       ├── repo_filter.rs           # RepoFilter, ranks repo paths against a fuzzy query
    │       ├── repo_scan.rs             # RepoScan, one home search and its result
    │       ├── typed_repo_check.rs      # TypedRepoCheck, the git check of a typed repo path
    │       ├── home_path.rs             # HomePath, a path shown from ~ when it is under home
    │       ├── agent_picker.rs          # AgentPicker, the n dialog's agent list
    │       ├── delete_session_dialog.rs # DeleteSessionDialog
    │       └── rename_session_dialog.rs # RenameSessionDialog
    └── test_support/               # #[cfg(test)] doubles and fixtures shared by unit tests
        ├── mod.rs
        ├── tmux_test_server.rs      # TmuxTestServer: private socket, control-mode clients, kill-server on drop
        ├── git_test_repo.rs         # GitTestRepo: hermetic git environment and fixture repos
        ├── service_fixture.rs       # ServiceFixture: every fake wired into a SessionService
        ├── fake_session_registry.rs # FakeSessionRegistry
        ├── fake_workspace.rs        # FakeWorkspace
        ├── fake_multiplexer.rs      # FakeMultiplexer
        ├── fake_return_binding.rs   # FakeReturnBinding
        ├── fake_config_source.rs    # FakeConfigSource
        ├── fake_repo_scanner.rs     # FakeRepoScanner
        ├── fake_host_probe.rs       # FakeHostProbe
        ├── fake_clock.rs            # FakeClock
        ├── recording_screen_mode.rs # RecordingScreenMode, the screen mode that records each switch
        ├── ui_harness.rs            # UiHarness: scripted input and a TestBackend wrapper for EventLoop tests
        ├── cli_harness.rs           # CliHarness: a Cli run against the fakes with captured output
        └── fake_lock.rs             # FakeLock
```

## Layout rules

- `domain` never imports `crossterm`, `ratatui`, `std::process`, or any adapter.
- `application` defines traits and use cases, never shells out, never touches the filesystem directly, and never reads the system clock. Ports are traits in `application/ports/`, and adapters implement them.
- `ui` calls the `SessionService` facade only. `SessionLifecycle`, `SessionMonitor`, `SessionAccess`, `RepoCatalog`, and `Doctor` are composed by the facade and are not called from `ui` or `cli.rs`.
- One primary type per file, named after it in snake case. Supporting types owned by the primary type live in the same file. Unit tests live inline. `mod.rs` declares and re-exports only. The tree in this file is the file contract: every listed file exists and holds its one type, and primary types are never merged into one file. Source files target under 200 lines excluding tests, and the files under `src/test_support/` count as tests. When one outgrows that, extract its largest behavior-bearing type into its own file. A file may stay over the target when its primary type is the only behavior-bearing type left in it. The types the next rule places in `cli.rs` and `app.rs` stay there even when that keeps the file over the target. Never split one type's own behavior across new collaborators only to meet the target. A commit that adds a file under `src/` that the tree does not list, or moves, renames, or removes a listed file, updates the tree in this file in the same commit.
- `cli.rs` owns the clap argument structs (`ListArgs`, `NewArgs`, and so on) as support types of `Cli`. They get no files of their own. The binding guard and the exit notice are support types of `App` in `app.rs`.
- Prefer a separate file for a type that is used across modules, carries behavior or invariants, and changes for its own reasons. Merging files later is mechanical, and splitting a file that grew is disruptive. The reverse also holds: do not invent files for types that need none.
- `src/test_support/` is `#[cfg(test)]` only and never appears in the binary. It holds the hand-written doubles and fixtures that tests across modules share.
- Other support types, such as a parser result, a doctor finding, a session spec, or a dialog's in-progress state, live in their primary type's file and get no tree entry.
- No code below `main` calls `std::env::current_exe`, reads the process environment (including `$TMUX`, `$TMUX_PANE`, `$HOME`, and `$SHELL`) or the process cwd, resolves XDG paths, or asks which terminal standard input is. `main` reads them once and injects them, with environment values in the `ProcessEnvironment` snapshot. `ProcessEnvironment::capture` reads the terminal name of standard input with `libc::ttyname_r` on descriptor 0, in the type's only `unsafe` block.
- Every `unsafe` block carries one `// SAFETY:` comment that states why the call is sound, and is confined to one adapter method.
- Every tmux target is exact: `=<session>:` from `TmuxScripts::exact`, a window id, or a pane id. Never pass a bare `-t <name>`, because tmux resolves a name with no exact match by prefix, so `fix` would reach `fix-auth`. Every `new-session`, `new-window`, and `respawn-window` passes the worktree through `TmuxScripts::start_in`, because without `-c` tmux starts the pane in the caller's directory and still exits 0.

## Wiring

- `main.rs` holds `fn main` and `CompositionRoot`, whose methods build the adapters, the `SessionService`, and the TUI or the CLI run. Argument parsing and dispatch stay in `Cli`.
- `ringleader` with no subcommand opens the TUI. Any subcommand runs the CLI and never starts the TUI. The hidden `__exec-agent` path loads no config, opens no log, and takes no lock.
- The TUI takes the lifetime lock, and the mutating commands (`new`, `delete`, `rename`, `restart`, `repo add`, `repo remove`) take the write lock. `list`, `doctor`, `attach`, `repo list`, and `__exec-agent` take no lock.
- Only the TUI installs the Ctrl-Q return binding. The CLI never touches tmux key bindings.
- Blocking TUI operations (create, delete, rename, restart, and the delete measurement) run behind a busy message, and the event loop drops the key and paste events queued while they ran.
- A dialog's hint line lists only keys whose meaning is Ringleader's own, such as `Enter create`, `Ctrl-S create`, and `Esc cancel`, and the keys that choose from a list, such as `↑ ↓ j k choose`. It never lists text-editing keys like cursor movement, Ctrl-U, or Ctrl-W, because every terminal user already expects them. A new editing key goes into `TextField` with its usual terminal meaning, not into a hint.
- Logs go to `$XDG_STATE_HOME/ringleader/app.log` through `env_logger`, never to the terminal.
- `AgentHooks` types an agent's `startup_input` after create and restart, waiting up to 10 seconds for its prompt, and its `rename_input` after a rename only when the agent is at its prompt at that moment. Each later line waits up to 2 seconds for the prompt to come back, after a short pause that lets the agent take the line before it. An agent is at its prompt when its pane is out of copy mode, shows the cursor, and the text before the cursor on its row is a bare prompt marker and a blank, with no letters or digits. Copy mode would take the keys instead of the agent. Claude hides the cursor in its trust and permission dialogs, an agent still starting has drawn no marker, and an unsent draft puts its text before the cursor. A skipped or failed typing is logged and never fails the use case.

## CLI exit codes

- 0: success.
- 1: a user or validation error, including an argument error, and any other failure.
- 2: a session name that was not found.
- 3: a conflict, including a refused guard, an `Unmanaged` target, a session that is live and healthy, or the lock being held.

A command interrupted by a signal before its use case starts never runs it, and one interrupted during its use case finishes it and then ends by that signal.

## Tests

- All tests are unit tests inside `src/`, in `#[cfg(test)] mod tests` or `src/test_support/`. `tests/fixtures/` holds fixtures only, with fake data, and there are no `tests/*.rs` integration tests.
- Application tests run against the hand-written fakes in `src/test_support/`, wired by `ServiceFixture`. There is no mocking crate. One concept per test, public interfaces only.
- Git adapter tests run real git in `tempfile` repos with the hermetic git environment set on each `Command`, never through `std::env::set_var`.
- tmux tests never skip. Each starts its own server through `TmuxTestServer` on a private socket under `/tmp` with `-f /dev/null`, and builds its snapshot from the test process's own `PATH`. Tests that need a client hold a control-mode client.
- UI tests render with `TestBackend` and drive `EventLoop` with scripted input, a recording screen mode, and a recording shell runner. No test calls crossterm or needs a terminal.
- No test reads wall-clock time to classify a session, and tests read nothing from the process environment except their own `PATH`.
- A constructor, accessor, or helper that only tests call carries `#[cfg(test)]`, so `cargo clippy --all-targets -- -D warnings` stays clean without any dead-code allowance.

## Commits

- One commit per finished change, after every command in Commands passes.
- The title starts with a capital letter, uses the imperative mood, and stays under 72 characters. The body says what changed in plain language.
- Push only when the project owner asks.

## Documentation

- `README.md` stays terse and human-facing: what Ringleader is, install, first run, keys, the CLI synopsis, and the exit codes. No architecture and no agent instructions.
- The repository holds no `CLAUDE.md` or `CLAUDE.local.md`, because either would hide this file from Claude Code. If one is ever needed, its first line is `@AGENTS.md`.
