# Layout

**This file holds the Layout tree. The Layout rules in `AGENTS.md` govern the tree.**

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
├── docs/agents/            # reference files that AGENTS.md names, read on demand
├── tests/fixtures/         # fake config, registry, tmux and git output
└── src/
    ├── main.rs             # fn main and CompositionRoot
    ├── cli.rs              # Cli struct, clap definitions, dispatch
    ├── cli/
    │   ├── cli_output.rs         # CliOutput, writeln! output that stops at a broken pipe
    │   ├── cli_exit.rs           # CliExit, the exit of one command and the exit code of each service and lock error
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
    │   ├── merge_bases.rs        # MergeBases, the best common ancestors a session diff may measure from, and the one it prefers
    │   ├── session_summary.rs    # SessionSummary
    │   ├── unmanaged_session.rs  # UnmanagedSession, a live tmux session Ringleader does not manage, its label, and the note or kill-session advice that explains why
    │   ├── agent.rs              # Agent
    │   ├── agent_mode.rs         # AgentMode, build or plan, the mode a new session's agent starts in
    │   ├── repo.rs               # Repo
    │   ├── repo_name.rs          # RepoName
    │   ├── repo_draft.rs         # RepoDraft, a [[repos]] entry before the load checks
    │   ├── base_branch.rs        # BaseBranch, a requested, configured, or origin default base branch and the branches it is looked up as
    │   ├── base_branch_ladder.rs # BaseBranchLadder, the origin default, main, and master branches tried when no base branch is set, never a recorded session's branch
    │   ├── branch.rs             # Branch, a local or remote-tracking branch named as users type it, and origin's copy of a branch
    │   ├── branch_tips.rs        # BranchTips, the commits the existing ones of some branches point at
    │   ├── config.rs             # Config, the parsed config.toml
    │   ├── config_draft.rs       # ConfigDraft, the raw config values the Config constructor checks
    │   ├── config_checks.rs      # ConfigChecks, the checks every config load runs
    │   ├── naming_scheme.rs      # NamingScheme, the worktree root and tmux prefix a session records
    │   ├── pull_request.rs       # PullRequest, its number, state, and opening time
    │   ├── hot_key.rs            # HotKey, a key a terminal can send, which every key binding is made of
    │   ├── terminal_text.rs      # TerminalText, text written for a terminal, and the same text without its escape sequences
    │   └── key_bindings.rs       # KeyBindings, the return key that detaches back to Ringleader, the keys that switch to the next and previous session, and the keys that toggle between the agent and the shell
    ├── application/
    │   ├── mod.rs
    │   ├── session_service.rs    # SessionService, the facade the UI and CLI call
    │   ├── service_context.rs    # ServiceContext and LoadedConfig, what main injects into the facade
    │   ├── service_ports.rs      # ServicePorts with the StatePorts, RepoPorts, TmuxPorts, and DesktopPorts bundles
    │   ├── service_error.rs      # ServiceError, the kinds of failure the use cases report
    │   ├── session_lifecycle.rs  # SessionLifecycle: create, delete, rename, restart, merge
    │   ├── busy_worktree.rs      # BusyWorktree, the tmux server processes that keep a stopped entry from restarting, with the advice for each case
    │   ├── unseen_session.rs     # UnseenSession, a tmux server Ringleader cannot see that may still hold a stopped entry's session, which rename refuses and delete refuses or warns about
    │   ├── session_creation.rs   # SessionCreation: create_session with its rollbacks
    │   ├── start_point_resolver.rs # StartPointResolver, the commit a new worktree starts from, or why there is none
    │   ├── base_branch_resolver.rs # BaseBranchResolver, the base branch a session's diff follows: its recorded branch while that still exists, else its repo's configured one
    │   ├── base_branch_ladders.rs # BaseBranchLadders, the base branch ladder of a checkout, led by origin's default and skipping the repo's session branches, which the start point, diff base, and upstream resolvers share
    │   ├── worktree_rollback.rs  # WorktreeRollback, removes the worktree and branch of a failed create
    │   ├── creation_plan.rs      # CreationPlan and CreateRequest
    │   ├── use_checks.rs         # UseChecks, the checks a create or a restart runs before acting
    │   ├── session_renaming.rs   # SessionRenaming: rename_session with its branch rollback
    │   ├── branch_renaming.rs    # BranchRenaming, moves a session's branch to its new name, back when the rename fails, and drops the upstream left on the old name once the rename is kept
    │   ├── rename_request.rs     # RenameRequest, the old and new names
    │   ├── branch_availability.rs # BranchAvailability, refuses a branch name that exists or holds other branches, for create and rename
    │   ├── session_restart.rs    # SessionRestart: restart of an Exited or Stopped session
    │   ├── session_scratch.rs    # SessionScratch, makes a session's scratch directory before its agent starts, removes it with the session, and logs either failure
    │   ├── session_envrc.rs      # SessionEnvrc and EnvrcChoice, whether a session's agent starts through its worktree's .envrc, with the warning when it cannot
    │   ├── start_outcome.rs      # StartOutcome, the session a create or restart started, its .envrc warning, and the socket recovery note
    │   ├── session_merge.rs      # SessionMerge: merge of origin's copy of the base branch into a Waiting session
    │   ├── merge_report.rs       # MergeReport, what a pull did to a session and whether its agent was asked to resolve the conflicts
    │   ├── upstream_resolver.rs  # UpstreamResolver, the branch a pull fetches from origin, or why there is none
    │   ├── agent_hooks.rs        # AgentHooks, schedules an agent's startup input in the background and types its rename input
    │   ├── worktree_inspection.rs # WorktreeInspection, the worktree checks restart, delete, and doctor share
    │   ├── session_deletion.rs   # SessionDeletion: delete_session steps 5 to 8 and forget
    │   ├── deletion_guards.rs    # DeletionGuards: delete_session steps 2 to 4
    │   ├── branch_ownership.rs   # BranchOwnership, refuses to delete or rename a branch without the Ringleader marker
    │   ├── delete_impact.rs      # DeleteImpact and DeleteConfirmation
    │   ├── delete_outcome.rs     # DeleteOutcome
    │   ├── worktree_containment.rs # WorktreeContainment, shared by delete step 3 and reconciliation
    │   ├── session_monitor.rs    # SessionMonitor: observation, reconciliation, diff measurement, and the background jobs for observation, diff measurement, and preview capture
    │   ├── diff_base_resolver.rs # DiffBaseResolver, the commit a session diff measures from
    │   ├── session_reconciler.rs # SessionReconciler, pairs registry entries with live tmux sessions
    │   ├── reconciliation.rs     # Reconciliation, the reconciled entries and rows
    │   ├── session_access.rs     # SessionAccess: attach, open_shell, and open_terminal, and the refusal from a pane of the private server
    │   ├── repo_catalog.rs       # RepoCatalog: discover, find, add, and remove repos
    │   ├── pull_request_catalog.rs # PullRequestCatalog, looks up the pull requests of sessions in the background
    │   ├── repo_merge.rs         # RepoMerge, joins configured and scanned repos, configured entries winning, and finds the config entry and the sessions of a repo by its canonical path
    │   ├── home_repo_search.rs   # HomeRepoSearch, the repos under home the repo search offers
    │   ├── default_repo.rs       # DefaultRepo, the launch repo or the newest session's repo
    │   ├── typed_paths.rs        # TypedPaths, a typed path read as a directory or as a repo
    │   ├── repo_overrides.rs     # RepoOverrides: the add_repo and remove_repo workers
    │   ├── pending.rs            # Pending, a worker result the TUI polls and the CLI waits on
    │   ├── doctor.rs             # Doctor
    │   ├── diagnostic_paths.rs   # DiagnosticPaths, the program, state, and private tmux server paths doctor and help report, and the log file help shows
    │   ├── tool_version.rs       # ToolVersion, the major and minor numbers of a tmux or git version
    │   ├── doctor_report.rs      # DoctorReport, the doctor's info, warning, and problem lines
    │   ├── registry_audit.rs     # RegistryAudit, the doctor's checks of each registry entry
    │   ├── orphan_audit.rs       # OrphanAudit, the doctor's search for worktrees and marked branches no session claims
    │   ├── server_audit.rs       # ServerAudit, the doctor's checks of Ringleader's tmux server and the key bindings
    │   └── ports/
    │       ├── mod.rs
    │       ├── session_registry.rs   # SessionRegistry trait
    │       ├── workspace.rs          # Workspace trait
    │       ├── dirty_state.rs        # DirtyState, the uncommitted files and nested repos of a worktree
    │       ├── head.rs               # Head, whether a checkout's HEAD is unborn, detached, or on a branch
    │       ├── unmerged_commits.rs   # UnmergedCommits, commits on no other ref with the tip and HEAD
    │       ├── merge_outcome.rs      # MergeOutcome, whether a worktree's merge of a branch from origin was up to date, merged, or conflicted
    │       ├── multiplexer.rs        # Multiplexer trait
    │       ├── agent_launch.rs       # AgentLaunch, the argv, environment, and worktree an agent starts with
    │       ├── config_source.rs      # ConfigSource trait
    │       ├── repo_scanner.rs       # RepoScanner trait
    │       ├── scratch_space.rs      # ScratchSpace trait
    │       ├── directory_environment.rs # DirectoryEnvironment trait and Envrc, a directory's .envrc with its bytes, whether it is allowed or denied, and the command that allows it
    │       ├── pull_requests.rs      # PullRequests trait
    │       ├── terminal_emulator.rs  # TerminalEmulator trait and TerminalEmulatorError, opens a terminal window in a directory
    │       ├── host_probe.rs         # HostProbe trait
    │       ├── clock.rs              # Clock trait
    │       ├── lock.rs               # Lock trait
    │       └── interrupts.rs         # Interrupts trait
    ├── adapters/
    │   ├── mod.rs
    │   ├── agent_launcher.rs     # AgentLauncher, the hidden __exec-agent mode, owns AgentHandoff and the exec seam, and execs direnv in front of a second launcher for a direnv launch
    │   ├── launcher_command.rs   # LauncherCommand and PreparedLaunch, the launcher words a pane runs and the handoff file they name
    │   ├── agent_handoff.rs      # AgentHandoff, the handoff file a launch writes and the launcher takes
    │   ├── handoff_contents.rs   # HandoffContents and EnvrcStep, the argv, inherited and explicit variables, worktree, and .envrc step a handoff file carries
    │   ├── app_log.rs            # AppLog, the append-mode app.log behind env_logger
    │   ├── process_environment.rs # ProcessEnvironment, the environment snapshot main captures once, and the shell it names
    │   ├── xdg_paths.rs          # XdgPaths, the config, state, data, cache, and runtime directories, the scratch root, and the private tmux server's socket and config
    │   ├── directory_repo_scanner.rs # DirectoryRepoScanner
    │   ├── directory_scratch_space.rs # DirectoryScratchSpace, one private scratch directory per worktree inside Ringleader's cache directory
    │   ├── mount_table.rs        # MountTable, the network and FUSE mount points the scan skips, and whether anything is mounted inside a scratch directory
    │   ├── toml_config_source.rs # TomlConfigSource
    │   ├── raw_config.rs         # RawConfig, the config.toml shape before the load checks
    │   ├── config_file_writer.rs # ConfigFileWriter, the read-only checks and mode-keeping config write
    │   ├── json_session_registry.rs # JsonSessionRegistry
    │   ├── file_lock.rs          # FileLock
    │   ├── fnv_hash.rs           # FnvHash, the FNV-1a hash of the server config and of a worktree path
    │   ├── gh_pull_requests.rs   # GhPullRequests, looks up the pull requests of a repo's branches in one gh GraphQL call
    │   ├── gh_query.rs           # GhQuery, the aliased GraphQL query for a repo's branches
    │   ├── gh_listing.rs         # GhListing, the viewer's or repo owner's newest pull request per branch in a gh response
    │   ├── null_pull_requests.rs # NullPullRequests, finds no pull request on a host without gh
    │   ├── direnv_directory_environment.rs # DirenvDirectoryEnvironment, reads a directory's .envrc, asks direnv whether it is allowed, and allows it
    │   ├── null_directory_environment.rs # NullDirectoryEnvironment, finds no .envrc on a host without direnv
    │   ├── system_terminal_emulator.rs # SystemTerminalEmulator, asks xdg-terminal-exec which terminal it opens, starts the terminal program in a directory, in a session of its own, and logs a failed exit
    │   ├── terminal_program.rs   # TerminalProgram, xdg-terminal-exec with --dir, the program $TERMINAL names, or ghostty +new-window with --working-directory, since a running Ghostty ignores the directory of a plain launch
    │   ├── null_terminal_emulator.rs # NullTerminalEmulator, opens no window on a host without a graphical display or a terminal, and says which it lacks
    │   ├── bounded_command.rs    # BoundedCommand, runs git, tmux, gh, direnv, and xdg-terminal-exec children in a new session with a deadline
    │   ├── deadlines.rs          # Deadlines and CallKind, the deadline of each kind of child call
    │   ├── command_output.rs     # CommandOutput, a finished child's status and output
    │   ├── host_path.rs          # HostPath, lenient canonicalization and directory listing
    │   ├── git_workspace.rs      # GitWorkspace
    │   ├── git_runner.rs         # GitRunner, runs a GitInvocation through BoundedCommand
    │   ├── git_invocation.rs     # GitInvocation, one git call with its child environment
    │   ├── git_porcelain.rs      # GitPorcelain, parses git's machine-readable output
    │   ├── git_refs.rs           # GitRefs, HEAD, the default branch a remote's HEAD names, and the tips of branches
    │   ├── git_diffs.rs          # GitDiffs, the merge bases of a worktree's HEAD and its diff stat
    │   ├── branch_ref.rs         # BranchRef, the git ref name of a Branch and the Branch a ref name spells
    │   ├── git_branches.rs       # GitBranches: compare-and-swap delete, rename, the managed-branch marker, and whether a branch has been pushed
    │   ├── branch_marker.rs      # BranchMarker, the git config entry that marks a branch Ringleader created, and the command that sets it by hand
    │   ├── path_option.rs        # PathOption, a command line option that takes a path in the same argument, such as --dir=<path>
    │   ├── shell_word.rs         # ShellWord, one word of a command Ringleader advises, quoted when a shell would split or expand it
    │   ├── branch_section.rs     # BranchSection, the git config section and upstream keys of one branch
    │   ├── upstream_merger.rs    # UpstreamMerger, fetches a branch from origin and merges origin's copy into a worktree, listing any conflicted paths
    │   ├── submodule_commits.rs  # SubmoduleCommits, the submodule walk of unmerged_commits
    │   ├── system_clock.rs       # SystemClock, reads `SystemTime`
    │   ├── system_host_probe.rs  # SystemHostProbe
    │   ├── live_process.rs       # LiveProcess, one /proc entry, whether it is this user's tmux server of a socket, and signals to it
    │   ├── tmux_command_line.rs  # TmuxCommandLine, the socket a tmux argv names in its global flags and the process name tmux gives a server it starts
    │   ├── metadata_json.rs      # MetadataJson, the @ringleader_meta JSON each session carries
    │   ├── signal_hook_interrupts.rs # SignalHookInterrupts, the SIGHUP, SIGINT, SIGTERM, and SIGQUIT flags
    │   ├── durable_file.rs       # DurableFile, synced temp file renamed over the target
    │   ├── tmux_command.rs       # TmuxCommand, the private server's `-S` and `-f`, `-u` and `-N`, a child environment without `TMUX` and `TMUX_PANE`, the caller's identity
    │   ├── server_files.rs       # ServerFiles, the private server's socket and generated config, the 0700 socket directory, and whether a server still starts under the socket's lock
    │   ├── tmux_error.rs         # TmuxError
    │   ├── tmux_sequence.rs      # TmuxSequence, `;`-separated commands with `#` and `;` escaping
    │   ├── tmux_output.rs        # TmuxOutput, labeled lines and the zero-session errors
    │   ├── tmux_scripts.rs       # TmuxScripts, every tmux command sequence the multiplexer sends, exact targets, start directories
    │   ├── tmux_launch.rs        # TmuxLaunch, ringleader-created and ringleader-done handling, rollback, tag move
    │   ├── tmux_agent_input.rs   # TmuxAgentInput, waits for the agent's prompt and types lines into its pane
    │   ├── tmux_listing.rs       # TmuxListing, parses list-panes, list-windows, list-keys, and server rows
    │   ├── tmux_multiplexer.rs   # TmuxMultiplexer
    │   ├── key_switch.rs         # KeySwitch, the copy-mode bindings each newly bound key displaces, saved in server options, and the ones each dropped key displaced, replayed
    │   ├── private_server.rs     # PrivateServer, the private server's config files, return binding check, state, config reapply that unbinds the keys the config dropped, and lost socket recovery
    │   ├── running_config.rs     # RunningConfig, the hash and key bindings a running server reports
    │   ├── tmux_server_config.rs # TmuxServerConfig, the generated config of the private tmux server with its return, session switch, and shell toggle bindings, its hash, and the script that reapplies it
    │   └── tmux_shell_toggle.rs  # TmuxShellToggle, the tmux command a shell toggle key runs: the agent window from the shell window, else the shell window, opened in the worktree when missing
    ├── ui/
    │   ├── mod.rs
    │   ├── app.rs                  # App
    │   ├── action_gate.rs          # ActionGate, which actions the selected row allows
    │   ├── back_key.rs             # BackKey, Ctrl-Q, which closes any dialog and never quits
    │   ├── key_sequence.rs         # KeySequence, the g that waits for a second g to make gg
    │   ├── home_state.rs           # HomeState, the polled rows, selection, and preview
    │   ├── session_list.rs         # SessionList, rows sorted by repo, state, and name, the selected session or heading, the folded repos, and the viewport
    │   ├── list_selection.rs       # ListSelection, the selected session or repo heading
    │   ├── observation_feed.rs     # ObservationFeed, the observation background job and its timer
    │   ├── preview_feed.rs         # PreviewFeed, the preview capture background job for the selected row and its timer
    │   ├── diff_queue.rs           # DiffQueue, round-robin diff measurement background jobs for visible rows, at most two at a time
    │   ├── pull_request_feed.rs    # PullRequestFeed, the pull requests of visible rows and their timer
    │   ├── bell.rs                 # Bell, rings on a Working to Waiting edge
    │   ├── shell_runner.rs         # ShellRunner, runs the ephemeral t shell
    │   ├── blocking_operation.rs   # BlockingOperation, a create, delete, rename, restart, or merge run behind a busy message
    │   ├── status_report.rs        # StatusReport, the status bar message and how outcomes become one
    │   ├── text_fit.rs             # TextFit, padding and ellipsis truncation
    │   ├── event_loop.rs           # EventLoop
    │   ├── terminal_guard.rs       # TerminalGuard
    │   ├── home_view.rs            # HomeView
    │   ├── empty_state.rs          # EmptyState, the no-sessions message with the logo above it when it fits
    │   ├── session_table.rs        # SessionTable, the home screen's session list, drawn as a tree of repos
    │   ├── session_tree.rs         # SessionTree, the sorted rows as repo headings and branch lines, and the lines a viewport shows
    │   ├── viewport.rs             # Viewport, the lines the session table shows, which move only to keep the selection in view, and the lines H M L select
    │   ├── repo_heading.rs         # RepoHeading, a repo's heading line with its number and fold arrow, and its session and state counts while folded
    │   ├── repo_group.rs           # RepoGroup, the repo a row is listed under, or other
    │   ├── colored_row.rs          # ColoredRow, a row's text split around its coloured cells
    │   ├── state_indicator.rs      # StateIndicator, the glyph and color of a session state
    │   ├── table_columns.rs        # TableColumns, column widths and dropping, and RowCells, the cell texts of one row
    │   ├── preview_panel.rs        # PreviewPanel, the Full tier's preview
    │   ├── preview_text.rs         # PreviewText, the rows of a preview capture, parsed once, and the rows that fit the preview
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
    │       ├── new_session_view.rs      # NewSessionView, draws the n dialog's fields, mode, error, and hints into its area
    │       ├── new_session_layout.rs    # NewSessionLayout, the rows the n dialog gives its repo search, mode, prompt, and error
    │       ├── prompt_field.rs          # PromptField, the n dialog's multiline prompt on ratatui-textarea
    │       ├── new_session_field.rs     # NewSessionField, the n dialog's fields in Tab order with their labels and hints
    │       ├── repo_picker.rs           # RepoPicker, the n dialog's Repo field and its search
    │       ├── chosen_repo.rs           # ChosenRepo, the default repo until the search picks another
    │       ├── repo_finder.rs           # RepoFinder, the fuzzy search over the repos under home
    │       ├── repo_filter.rs           # RepoFilter, ranks repo paths against a fuzzy query
    │       ├── fuzzy_ranker.rs          # FuzzyRanker, ranks names or paths against a fuzzy query
    │       ├── match_list.rs            # MatchList, the ranked matches of a fuzzy search and the cursor over them
    │       ├── repo_scan.rs             # RepoScan, one home search and its result
    │       ├── search_query.rs          # SearchQuery, a finder's typed query shown after its prompt
    │       ├── typed_repo_check.rs      # TypedRepoCheck, the git check of a typed repo path
    │       ├── home_path.rs             # HomePath, a path shown from ~ when it is under home
    │       ├── agent_picker.rs          # AgentPicker, the n dialog's agent list
    │       ├── delete_session_dialog.rs # DeleteSessionDialog
    │       ├── rename_session_dialog.rs # RenameSessionDialog
    │       └── session_finder.rs        # SessionFinder, the / and Ctrl-K search over session names that attaches to the pick
    └── test_support/               # #[cfg(test)] doubles and fixtures shared by unit tests
        ├── mod.rs
        ├── tmux_test_server.rs      # TmuxTestServer: private socket, generated config, managed sessions, control-mode and nested clients, kill-server on drop
        ├── git_test_repo.rs         # GitTestRepo: hermetic git environment and fixture repos
        ├── service_fixture.rs       # ServiceFixture: every fake wired into a SessionService
        ├── fake_session_registry.rs # FakeSessionRegistry
        ├── fake_workspace.rs        # FakeWorkspace
        ├── fake_multiplexer.rs      # FakeMultiplexer
        ├── fake_config_source.rs    # FakeConfigSource
        ├── fake_repo_scanner.rs     # FakeRepoScanner
        ├── fake_scratch_space.rs    # FakeScratchSpace
        ├── fake_directory_environment.rs # FakeDirectoryEnvironment
        ├── direnv_test_home.rs      # DirenvTestHome: a temporary HOME, XDG_DATA_HOME, and XDG_CONFIG_HOME for tests that run a real direnv
        ├── fake_pull_requests.rs    # FakePullRequests
        ├── fake_terminal_emulator.rs # FakeTerminalEmulator
        ├── log_capture.rs           # LogCapture, the test logger that keeps warnings for assertions
        ├── fake_host_probe.rs       # FakeHostProbe
        ├── fake_clock.rs            # FakeClock
        ├── call_hooks.rs            # CallHooks, the hooks FakeMultiplexer and FakeWorkspace run before a call
        ├── gate.rs                  # Gate: a test gate that blocks a call until it is opened or a deadline passes
        ├── held_calls.rs            # HeldCalls: holds the calls that reach a call hook on a Gate and counts them
        ├── deadline.rs              # Deadline: polls a condition until it holds or a time limit passes
        ├── recording_screen_mode.rs # RecordingScreenMode, the screen mode that records each switch
        ├── ui_harness.rs            # UiHarness: scripted input and a TestBackend wrapper for EventLoop tests
        ├── cli_harness.rs           # CliHarness: a Cli run against the fakes with captured output
        ├── fake_lock.rs             # FakeLock
        └── fake_interrupts.rs       # FakeInterrupts
```
