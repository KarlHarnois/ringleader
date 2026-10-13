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
- Every ad-hoc tmux command, such as a binding experiment, runs as `env -u TMUX -u TMUX_PANE tmux -u -S <tempdir>/x.sock -f /dev/null ...` and ends with `kill-server` on that socket. `nix develop` passes `$TMUX` through, so a bare `tmux` command reaches the user's live server. Never touch the default server, the server named in `$TMUX`, or the owner's real Ringleader server at `$XDG_RUNTIME_DIR/ringleader/tmux/tmux.sock`. Keep the socket path under 100 bytes, because tmux rejects a longer one with `File name too long`. Use a directory from `mktemp -d` under `/tmp`.
- Every manual run of the `ringleader` binary, TUI or CLI, uses throwaway state, because `main` resolves the real XDG paths, and the private tmux server's socket lives under `XDG_RUNTIME_DIR`.
  - Create `<tempdir>` with `mktemp -d /tmp/rl.XXXXXX`, and inside it `config/ringleader`, `state`, `data`, `run`, `repos`, and `home`.
  - The prefix is `env -u TMUX -u TMUX_PANE -u RINGLEADER_CONFIG XDG_CONFIG_HOME=<tempdir>/config XDG_STATE_HOME=<tempdir>/state XDG_DATA_HOME=<tempdir>/data XDG_RUNTIME_DIR=<tempdir>/run HOME=<tempdir>/home SHELL=sh` followed by the hermetic git environment below. The throwaway `XDG_RUNTIME_DIR` and `XDG_STATE_HOME` keep a run off the owner's real private server and state, and the rest keeps the owner's shell and git startup files out of the run.
  - Every tmux command typed for Ringleader's server uses the prefix and passes `-S <tempdir>/run/ringleader/tmux/tmux.sock`.
  - Before the first run, write `<tempdir>/config/ringleader/config.toml` with one stand-in agent, `name = "stand-in"` and `command = "sh"`, never `claude` or `opencode`.
  - Build fixture repos under `<tempdir>/repos` with the hermetic git environment, and run the binary by absolute path with `<tempdir>/repos` as the working directory, so no real repository is in scope.
  - Runs happen in a pane of an outer server started under the ad-hoc tmux rule, which stands in for the user's tmux and is driven with `send-keys` on that server. Three checks cover the contexts: the binary with the prefix in a plain pane, the binary in the same pane with the prefix minus `-u TMUX -u TMUX_PANE` so the client opens nested, and the binary in a `t` shell of a session, which must refuse with exit 3. Each ends by asserting that the outer server's `list-keys` output and session list are unchanged.
  - End with `kill-server` on the outer server and on `<tempdir>/run/ringleader/tmux/tmux.sock` by explicit `-S` path, then remove `<tempdir>`. Never run the binary against the user's tmux server, config, state, data, or repositories.
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

## Architecture

Ringleader follows hexagonal architecture, also called ports and adapters. The core holds the domain and the use cases. It never imports an adapter, `crossterm`, `ratatui`, or `std::process`. It never runs tmux or git or touches the filesystem or the terminal itself, and reaches them through traits it owns, which adapters outside the core implement.

Dependencies point inward only. An inner layer never imports an outer one.

- `domain` holds the entities, value types, and pure rules. It imports nothing else from the crate.
- `application` holds the use cases and the ports, and imports only `domain`. It never shells out, never touches the filesystem directly, and never reads the system clock. Ports are traits in `application/ports/`, named after the role they play, such as `Workspace` and `Multiplexer`, with any error type in the same file.
- `adapters` holds the driven adapters, which implement the ports with git, tmux, files, and `/proc`, and the process-level types `CompositionRoot` uses directly, such as `ProcessEnvironment`, `XdgPaths`, and `AgentLauncher`. They import `application` and `domain`, never `ui` or `cli`. Each port adapter is named after its technology and the port it implements, such as `GitWorkspace` and `TmuxMultiplexer`.
- `ui` and `cli` are the driving adapters. They import `application` and `domain`, never `adapters` or each other. They reach the use cases only through the `SessionService` facade, which composes `SessionLifecycle`, `SessionMonitor`, `SessionAccess`, `RepoCatalog`, and `Doctor`, so `ui` and `cli` never call those directly. A port handed to a `ui` or `cli` type, like `Lock`, is called by that type.
- `main.rs` is the composition root and the only production code that imports every layer. The Wiring section says what it builds.
- `src/test_support/` may import any layer, because it fakes the ports and drives the outer layers under test. Inline `#[cfg(test)]` modules follow their layer's rule, except that any of them may also import `src/test_support/`.

The code breaks the `ui` and `cli` rule in four places today: `SignalFlags` in `ui/event_loop.rs`, `cli/cli_session.rs`, and a test in `cli/session_commands.rs`, and `AppLog` in `ui/dialogs/dialog.rs`. They are violations to fix, not precedent to copy.

A new capability that `application`, `ui`, or `cli` needs from outside the process goes through a port, because none of them may import `adapters`. `CompositionRoot` and the types in `adapters` reach the outside directly. Logging needs no port: every layer writes through the `log` macros, and `CompositionRoot` installs `AppLog` behind them. The steps for a port:

1. Define the trait in a new file under `application/ports/`, named after the role, with an error type beside it when any of its methods can fail, and re-export them from `ports/mod.rs`.
2. Implement it in `adapters/`, named after the technology and the role.
3. Add a hand-written fake in `src/test_support/`. Wire the fake of a port the use cases need into `ServiceFixture`, and the fake of any other port into `CliHarness` or `UiHarness`.
4. Inject the port as a trait object. A port the use cases need joins a bundle in `ServicePorts` as an `Arc<dyn Trait>`. Any other port, like `Lock`, is handed by `CompositionRoot` to the type that uses it.
5. Build the adapter in `CompositionRoot` and pass it in.

The one exception is a driving adapter's own channel to the user: the terminal for the TUI, including the shell it runs there, and the standard streams for the CLI. That I/O stays in `ui` or `cli`, behind a trait beside its real implementation or a std `Read` or `Write`, with its fake in `src/test_support/`. The TUI owns `InputSource`, `ScreenMode`, and `ShellRunner` this way, and the CLI writes through the `Write` streams `CliOutput` holds. A test seam inside `adapters`, like `AgentLauncher`'s `ExecSeam`, has the same shape with its fake in the adapter's own tests.

## Metrics

Local, on-demand code-health tools. CI never builds or runs them. Do not add them
to `checks`, to the default dev shell, or to `.github/workflows/ci.yml`. The flake
builds them as `apps` so `nix flake check` only evaluates them, and puts them in
the `metrics` dev shell, which extends the default one. That evaluation still
downloads the four upstream inputs, so if one can no longer be fetched, CI's
flake check fails until its input is repinned or removed. `dryer`, `crapper`, and
`mutator` write snapshots under `.metrics/`, which is gitignored.
Run them from the repository root, and pass `dryer` and `crapper` the `src` path.
A pathless run also walks nested checkouts such as `.claude/worktrees` and scores
every copy under the same namespaces, and `crapper` reruns coverage for each.
`dryer --changed` compares the changed files only with each other, so it misses
code copied from an unchanged file.

```sh
nix develop .#metrics -c dryer src             # duplicate candidates; writes .metrics/dry.edn
nix develop .#metrics -c crapper src           # CC plus coverage plus CRAP; writes .metrics/crap.edn
nix develop .#metrics -c mutator --max-workers 2 --timeout-factor 60 src/<module>.rs
nix run .#dryer -- src                         # the apps also work outside any dev shell
nix run .#crapper -- --help
nix run .#mutator -- --help
```

`mutator` runs every covered mutant and many tests start real tmux servers. Never
run it whole-repo. Name one module or file, and keep `--max-workers 2` and
`--timeout-factor 60`. Each run builds the crate from scratch in new worker
directories under `target/mutation-workers/`, and a mutant that times out counts
as killed. The default factor of 10 times the warm baseline ends those builds
early, so every mutant reads as killed before a test runs. Parallel cold builds
slow each other, so raise the factor before the worker count. Once a
snapshot exists, a rerun only runs survivors and sites in changed functions, so a
kill recorded by a timeout stays. After a run with a smaller factor, rerun that
file once with `--mutate-all`.
Never pass `--changed` with a path: it drops the path and mutates every changed
file in `git status`. `crapper` and `mutator` need `cargo-llvm-cov`
and the LLVM tools named by `LLVM_COV` and `LLVM_PROFDATA`, which the `metrics`
shell and the apps set. When `cargo llvm-cov` fails, whether from a missing tool
or one failing test, every function scores 0% coverage and the run still exits 0.
Check the output for `Rust coverage will score 0%` before trusting the snapshot.
Do not pass `mutator` a filtered `--test-command`. Many modules are tested only
through their callers, so a module-path filter runs zero tests, exits 0, and lets
every mutant survive.

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
├── .gitignore              # /target, /result, /result-*, /.direnv, /.claude/worktrees, /.metrics, /.uml-viewer, /uml-viewer-log.txt
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
    │   ├── cli_session.rs        # CliSession, signal checks, worker waits, the startup input wait the CLI and TUI end with, and the exit of one command
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
    │   ├── naming_scheme.rs      # NamingScheme, the worktree root and tmux prefix a session records
    │   └── return_key.rs         # ReturnKey, the key that detaches from a session back to Ringleader
    ├── application/
    │   ├── mod.rs
    │   ├── session_service.rs    # SessionService, the facade the UI and CLI call
    │   ├── service_context.rs    # ServiceContext and LoadedConfig, what main injects into the facade
    │   ├── service_ports.rs      # ServicePorts with the StatePorts, RepoPorts, and TmuxPorts bundles
    │   ├── service_error.rs      # ServiceError and its exit codes
    │   ├── session_lifecycle.rs  # SessionLifecycle: create, delete, rename, restart
    │   ├── busy_worktree.rs      # BusyWorktree, the tmux server processes that keep a stopped entry from restarting, with the advice for each case
    │   ├── session_creation.rs   # SessionCreation: create_session with its rollbacks
    │   ├── worktree_rollback.rs  # WorktreeRollback, removes the worktree and branch of a failed create
    │   ├── creation_plan.rs      # CreationPlan and CreateRequest
    │   ├── use_checks.rs         # UseChecks, the checks a create or a restart runs before acting
    │   ├── session_renaming.rs   # SessionRenaming: rename_session with its branch rollback
    │   ├── branch_renaming.rs    # BranchRenaming, moves a session's branch to its new name, back when the rename fails, and drops the upstream left on the old name once the rename is kept
    │   ├── rename_request.rs     # RenameRequest, the old and new names
    │   ├── branch_availability.rs # BranchAvailability, refuses a branch name that exists or holds other branches, for create and rename
    │   ├── session_restart.rs    # SessionRestart: restart of an Exited or Stopped session
    │   ├── agent_hooks.rs        # AgentHooks, schedules an agent's startup input in the background and types its rename input
    │   ├── worktree_inspection.rs # WorktreeInspection, the worktree checks restart, delete, and doctor share
    │   ├── session_deletion.rs   # SessionDeletion: delete_session steps 5 to 8 and forget
    │   ├── deletion_guards.rs    # DeletionGuards: delete_session steps 2 to 4
    │   ├── branch_ownership.rs   # BranchOwnership, refuses to delete or rename a branch without the Ringleader marker
    │   ├── delete_impact.rs      # DeleteImpact and DeleteConfirmation
    │   ├── delete_outcome.rs     # DeleteOutcome
    │   ├── worktree_containment.rs # WorktreeContainment, shared by delete step 3 and reconciliation
    │   ├── session_monitor.rs    # SessionMonitor: reconcile, observe, diffs, preview
    │   ├── session_reconciler.rs # SessionReconciler, pairs registry entries with live tmux sessions
    │   ├── reconciliation.rs     # Reconciliation, the reconciled entries and rows
    │   ├── session_access.rs     # SessionAccess: attach and open_shell, and the refusal from a pane of the private server
    │   ├── repo_catalog.rs       # RepoCatalog: discover, find, add, and remove repos
    │   ├── repo_merge.rs         # RepoMerge, joins configured and scanned repos, configured entries winning
    │   ├── home_repo_search.rs   # HomeRepoSearch, the repos under home the repo search offers
    │   ├── default_repo.rs       # DefaultRepo, the launch repo or the newest session's repo
    │   ├── typed_paths.rs        # TypedPaths, a typed path read as a directory or as a repo
    │   ├── repo_overrides.rs     # RepoOverrides: the add_repo and remove_repo workers
    │   ├── pending.rs            # Pending, a worker result the TUI polls and the CLI waits on
    │   ├── doctor.rs             # Doctor
    │   ├── diagnostic_paths.rs   # DiagnosticPaths, the program, state, and private tmux server paths doctor and help report
    │   ├── tool_version.rs       # ToolVersion, the major and minor numbers of a tmux or git version
    │   ├── doctor_report.rs      # DoctorReport, the doctor's info, warning, and problem lines
    │   ├── registry_audit.rs     # RegistryAudit, the doctor's checks of each registry entry
    │   ├── orphan_audit.rs       # OrphanAudit, the doctor's search for worktrees and marked branches no session claims
    │   ├── server_audit.rs       # ServerAudit, the doctor's checks of Ringleader's tmux server and the return key
    │   └── ports/
    │       ├── mod.rs
    │       ├── session_registry.rs   # SessionRegistry trait
    │       ├── workspace.rs          # Workspace trait
    │       ├── branch_marker.rs      # BranchMarker, the git config entry that marks a branch Ringleader created
    │       ├── dirty_state.rs        # DirtyState, the uncommitted files and nested repos of a worktree
    │       ├── unmerged_commits.rs   # UnmergedCommits, commits on no other ref with the tip and HEAD
    │       ├── multiplexer.rs        # Multiplexer trait
    │       ├── agent_launch.rs       # AgentLaunch, the argv, environment, and worktree an agent starts with
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
    │   ├── xdg_paths.rs          # XdgPaths, the config, state, data, and runtime directories and the private tmux server's socket and config
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
    │   ├── git_branches.rs       # GitBranches: compare-and-swap delete, rename, the managed-branch marker, and whether a branch has been pushed
    │   ├── branch_section.rs     # BranchSection, the git config section and upstream keys of one branch
    │   ├── start_point_resolver.rs # StartPointResolver, the base branch ladder
    │   ├── submodule_commits.rs  # SubmoduleCommits, the submodule walk of unmerged_commits
    │   ├── system_clock.rs       # SystemClock, reads `SystemTime`
    │   ├── system_host_probe.rs  # SystemHostProbe
    │   ├── live_process.rs       # LiveProcess, one /proc entry, whether it is the tmux server of a socket, and signals to it
    │   ├── metadata_json.rs      # MetadataJson, the @ringleader_meta JSON each session carries
    │   ├── signal_flags.rs       # SignalFlags, the SIGHUP, SIGINT, SIGTERM, and SIGQUIT flags
    │   ├── durable_file.rs       # DurableFile, synced temp file renamed over the target
    │   ├── tmux_command.rs       # TmuxCommand, the private server's `-S` and `-f`, `-u` and `-N`, a child environment without `TMUX` and `TMUX_PANE`, the caller's identity
    │   ├── server_files.rs       # ServerFiles, the private server's socket and generated config, and the 0700 socket directory
    │   ├── tmux_error.rs         # TmuxError
    │   ├── tmux_sequence.rs      # TmuxSequence, `;`-separated commands with `#` and `;` escaping
    │   ├── tmux_output.rs        # TmuxOutput, labeled lines and the zero-session errors
    │   ├── tmux_scripts.rs       # TmuxScripts, every tmux command sequence the multiplexer sends, exact targets, start directories
    │   ├── tmux_launch.rs        # TmuxLaunch, ringleader-created and ringleader-done handling, rollback, tag move
    │   ├── tmux_agent_input.rs   # TmuxAgentInput, waits for the agent's prompt and types lines into its pane
    │   ├── tmux_listing.rs       # TmuxListing, parses list-panes, list-windows, list-keys, and server rows
    │   ├── tmux_multiplexer.rs   # TmuxMultiplexer
    │   ├── private_server.rs     # PrivateServer, the private server's config files, return binding check, state, config reapply, and lost socket recovery
    │   ├── running_config.rs     # RunningConfig, the hash and return key a running server reports
    │   └── tmux_server_config.rs # TmuxServerConfig, the generated config of the private tmux server, its hash, and the script that reapplies it
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
    │   ├── state_indicator.rs      # StateIndicator, the glyph and color of a session state
    │   ├── table_columns.rs        # TableColumns, column widths and dropping
    │   ├── preview_panel.rs        # PreviewPanel, the Full tier's preview
    │   ├── preview_text.rs         # PreviewText, the captured rows that fit the preview
    │   ├── help_view.rs            # HelpView
    │   ├── label_column.rs         # LabelColumn, the help's padded label and the indent of its wrapped rows
    │   ├── legend.rs               # Legend, the help's symbols and their meanings, wrapped under a label
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
        ├── tmux_test_server.rs      # TmuxTestServer: private socket, generated config, control-mode clients, kill-server on drop
        ├── git_test_repo.rs         # GitTestRepo: hermetic git environment and fixture repos
        ├── service_fixture.rs       # ServiceFixture: every fake wired into a SessionService
        ├── fake_session_registry.rs # FakeSessionRegistry
        ├── fake_workspace.rs        # FakeWorkspace
        ├── fake_multiplexer.rs      # FakeMultiplexer
        ├── fake_config_source.rs    # FakeConfigSource
        ├── fake_repo_scanner.rs     # FakeRepoScanner
        ├── fake_host_probe.rs       # FakeHostProbe
        ├── fake_clock.rs            # FakeClock
        ├── gate.rs                  # Gate: a test gate that blocks a call until it is opened or a deadline passes
        ├── deadline.rs              # Deadline: polls a condition until it holds or a time limit passes
        ├── recording_screen_mode.rs # RecordingScreenMode, the screen mode that records each switch
        ├── ui_harness.rs            # UiHarness: scripted input and a TestBackend wrapper for EventLoop tests
        ├── cli_harness.rs           # CliHarness: a Cli run against the fakes with captured output
        └── fake_lock.rs             # FakeLock
```

## Layout rules

- One primary type per file, named after it in snake case. Supporting types owned by the primary type live in the same file. Unit tests live inline. `mod.rs` declares and re-exports only. The tree in this file is the file contract: every listed file exists and holds its one type, and primary types are never merged into one file. Source files target under 200 lines excluding tests, and the files under `src/test_support/` count as tests. When one outgrows that, extract its largest behavior-bearing type into its own file. A file may stay over the target when its primary type is the only behavior-bearing type left in it. The types the next rule places in `cli.rs` stay there even when that keeps the file over the target. Never split one type's own behavior across new collaborators only to meet the target. A commit that adds a file under `src/` that the tree does not list, or moves, renames, or removes a listed file, updates the tree in this file in the same commit.
- `cli.rs` owns the clap argument structs (`ListArgs`, `NewArgs`, and so on) as support types of `Cli`. They get no files of their own.
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
- A session's branch is its name, and its tmux session is the tmux prefix followed by its name. Create and rename never record a branch that differs from the name, and rename always moves the branch with the name. Rename refuses a branch that has been pushed, because the remote branch and any pull request keep the old name, and it never moves the worktree, because the running agent and its transcripts are keyed by that path. A session recorded on another branch, from before the branch prefix was dropped, from an adopted tmux session, or from a tmux session renamed by hand, keeps working. When its branch is marked and not pushed, `doctor` warns about it, and renaming it to a new name moves its branch.
- Key bindings come only from the generated config of the private tmux server, which `create_session` writes, and which the TUI at start and after `r`, `new`, and `restart` re-source on the running server when its `@ringleader_config` hash differs from the current render. Only lock holders do either, so only one process ever writes that config, and the reapply is skipped while the config failed to load. Every tmux call passes `-S <private socket>` and `-f`, so Ringleader never reads or writes the user's tmux server. An entry recorded on another socket is checked only through `/proc`. The TUI and the CLI attach through the same foreground child.
- Blocking TUI operations (create, delete, rename, restart, and the delete measurement) run behind a busy message, and the event loop drops the key and paste events queued while they ran.
- A dialog's hint line lists only keys whose meaning is Ringleader's own, such as `Enter create`, `Ctrl-S create`, and `Esc cancel`, and the keys that choose from a list, such as `↑ ↓ j k choose`. It never lists text-editing keys like cursor movement, Ctrl-U, or Ctrl-W, because every terminal user already expects them. A new editing key goes into `TextField` with its usual terminal meaning, not into a hint.
- Logs go to `$XDG_STATE_HOME/ringleader/app.log` through `env_logger`, never to the terminal.
- `AgentHooks` schedules an agent's `startup_input` in the background after create and restart, so the use case returns once the tmux session exists. One typing job runs per session at a time, and a second start for the same session is skipped. The job finds the agent pane once when it starts, then checks only the prompt conditions below before each line. It never rechecks the session's name or registry entry, so a rename during its wait does not reach it. The job waits up to 60 seconds for the first prompt, and each later line waits up to 2 seconds for the prompt to come back, after a short pause that lets the agent take the line before it. The TUI polls the job while it runs. On exit, the CLI after its command and the TUI after restoring the screen release the lock, note the wait on stderr, and wait for the job unless a signal arrives. Its `rename_input` is typed after a rename only when the agent is at its prompt at that moment. An agent is at its prompt when its pane is out of copy mode, shows the cursor, and the text before the cursor on its row is a bare prompt marker and a blank, with no letters or digits. Copy mode would take the keys instead of the agent. Claude hides the cursor in its trust and permission dialogs, an agent still starting has drawn no marker, and an unsent draft puts its text before the cursor. A skipped or failed typing is logged and never fails the use case.

## CLI exit codes

- 0: success.
- 1: a user or validation error, including an argument error, and any other failure.
- 2: a session name that was not found.
- 3: a conflict, including a refused guard, an `Unmanaged` target, a session that is live and healthy, or the lock being held.

A command interrupted by a signal before its use case starts never runs it, and one interrupted during its use case finishes it and then ends by that signal. One interrupted while it waits for queued startup input stops waiting, logs the abandoned input, and ends by that signal, though its use case succeeded.

## Tests

- All tests are unit tests inside `src/`, in `#[cfg(test)] mod tests` or `src/test_support/`. `tests/fixtures/` holds fixtures only, with fake data, and there are no `tests/*.rs` integration tests.
- Application tests run against the hand-written fakes in `src/test_support/`, wired by `ServiceFixture`. There is no mocking crate. One concept per test, public interfaces only.
- Git adapter tests run real git in `tempfile` repos with the hermetic git environment set on each `Command`, never through `std::env::set_var`.
- tmux tests never skip. Each starts its own server through `TmuxTestServer` on a private socket under `/tmp` with the rendered generated config, or with a fixture config when the test is about the config itself, and builds its snapshot from the test process's own `PATH`. Tests that need a client hold a control-mode client, and they press keys through it with `send-keys -K -c <client>`, never with `send-keys -t <pane>`, which skips the key tables. The one exception types raw terminal bytes with `send-keys -t <pane> -H` into a pane of a second test server that holds a nested client, so the nested client's own parsing of those bytes is what gets tested.
- UI tests render with `TestBackend` and drive `EventLoop` with scripted input, a recording screen mode, and a recording shell runner. No test calls crossterm or needs a terminal.
- No test reads wall-clock time to classify a session, and tests read nothing from the process environment except their own `PATH`.
- A constructor, accessor, or helper that only tests call carries `#[cfg(test)]`, so `cargo clippy --all-targets -- -D warnings` stays clean without any dead-code allowance.

## Commits

- One commit per finished change, after every command in Commands passes.
- The title starts with a capital letter, uses the imperative mood, and stays under 72 characters. The body says what changed in plain language.
- Push only when the project owner asks.

## Documentation

- `README.md` stays terse and human-facing: what Ringleader is, install, first run, the CLI synopsis, and the exit codes. No architecture and no agent instructions.
- The repository holds no `CLAUDE.md` or `CLAUDE.local.md`, because either would hide this file from Claude Code. If one is ever needed, its first line is `@AGENTS.md`.
