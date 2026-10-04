# v1 implementation log

Decisions made while implementing `docs/plans/v1-bootstrap.md`, recorded under the project owner's standing answers: contradictions inside the plan, plan claims that turned out wrong, and comments that survived the comment policy.

## Milestone 4

### Tree updates for extracted files

The owner's answer 2 says not to edit `docs/plans/v1-bootstrap.md` except where a milestone says to. Section 3 says:

> A commit that adds a file under `src/` that the tree does not list, or moves, renames, or removes a listed file, updates the tree in section 3 in the same commit.

Section 2 says:

> when one outgrows that, extract its largest behavior-bearing type into its own file and update the tree in section 3

Choice: treat these as standing instructions that apply to every milestone commit. Milestone 4's use cases came to about 1,900 lines of non-test code, so `session_lifecycle.rs`, `session_service.rs`, and `session_monitor.rs` were split into the files the section 3 tree now lists (`session_creation.rs`, `session_renaming.rs`, `session_deletion.rs`, `deletion_guards.rs`, and so on). The shared test fixture `src/test_support/service_fixture.rs` got a tree entry as section 0 requires. No other part of the plan was edited.

### Size target for test doubles

Section 0 says source files target under 200 lines "excluding tests". The hand-written fakes under `src/test_support/` are `#[cfg(test)]` code, so they are treated as tests and not split for size. `fake_workspace.rs` is about 400 lines.

## Milestone 6

### Unsafe block of the outside attach

Section 6 says:

> `TmuxMultiplexer::attach` owns this wait, so CLI `attach` behaves the same, and its `waitpid` and `kill` calls form that method's one `unsafe` block.

Choice: the `waitpid` and `kill` calls form one `unsafe` block in `TmuxMultiplexer::attach_in_foreground`, a private method that `attach` calls only in outside mode. The block stays confined to one method of `TmuxMultiplexer`, which is the intent.

### Restore checks that the key is still Ringleader's

Section 6 describes the startup repair as restoring only "when the pid matches and the root C-q is still recognizably Ringleader's", and describes the exit restore without that condition. Choice: the exit restore applies the same two checks through the same code. A user who rebinds C-q while the TUI runs keeps that binding, and a record whose server restarted is discarded without touching the new server, which the section 10 test "a restore after a server restart leaving the new server untouched" asks for.

### tmux 3.7 release used

Section 6 names 3.7b as the 3.7 release the binding forms were checked on. `nixpkgs/nixos-unstable#tmux` now provides tmux 3.7c. The milestone 6 run accepted it, since the milestone asks for "a 3.7 release". All 88 `adapters::tmux` tests passed on it with none skipped.

## Milestone 9

### Shell of the sessions made for manual runs

Section 0 says:

> Before the binary runs, start the server with the prefix and `tmux -u -S <tempdir>/tmux-<uid>/default -f /dev/null new-session -d -s holder`

and milestone 9 says:

> A session made on the private server with `new-session -d -s ringleader-demo` lists as `Unmanaged`

Section 0 also says never to touch the owner's config. Without a shell command, tmux starts the owner's login shell, which ran the owner's shell startup files and raised a tool's trust prompt in the throwaway pane. Nothing answered the prompt and nothing changed. Choice: from then on, manual runs pass `sh` as the pane command of `holder` and of the demo session, which keeps the owner's startup files out of the throwaway server.

### Clearing the screen at startup

After `script` dies by SIGKILL, it cannot restore the outer terminal, which stays on the alternate screen. The next TUI then drew its first frame over leftover text, because ratatui assumes a blank screen. The TUI now clears the terminal once before its first frame. The milestone 9 checks passed after the change: attach, `C-b C-z`, `fg`, `C-b d`, and `q` on the outside-tmux path, `kill-pane` ending the TUI in 0.16 seconds, and SIGKILL to `script` ending it in 0.16 seconds, each followed by a run that took the lock.

## Milestone 19

### Environment of the TUI in manual runs

Section 0 gives the prefix for manual runs, which sets `TMUX_TMPDIR` and the XDG directories and leaves `HOME` and `SHELL` as the owner's. The `t` check runs `$SHELL` in a tmux window and as an ephemeral shell, so the owner's shell would have read the owner's startup files, and `ringleader new` would have run git with the owner's global git config. Section 0 also says never to touch the owner's config. Choice: manual runs from milestone 19 on add `HOME=<tempdir>/home`, `SHELL=sh`, `GIT_CONFIG_GLOBAL=/dev/null`, `GIT_CONFIG_NOSYSTEM=1`, and a fake git author and committer to the section 0 prefix. The milestone 19 check passed under it: `t` on a live session opened a window tagged `@ringleader_shell` in the worktree, a second `t` reused that window, `t` on a stopped session ran `sh` in the worktree and returned to the TUI on `exit`, `s` restarted the stopped session, `s` on a live session was refused, and `r` reported a reload and a broken config.

## Milestone 23

### Binding guard and exit notice stay in app.rs

Section 6 says:

> The binding guard and the exit notice are support types of `App` in `app.rs`.

Section 0 says:

> Source files target under 200 lines excluding tests.

With `BindingGuard`, `ExitNotice`, and its `NoticeHandle`, `app.rs` holds about 275 lines of non-test code. Choice: section 6 is the more detailed passage, since it names these two types and their file, so they stay in `app.rs`. `App` itself stays under 200 lines because the open dialog lives in `DialogSlot`, the key table in `ActionGate`, and the polled rows in `HomeState`.

### Ctrl-Q verification by hand

The section 6 verification criteria were checked under the section 0 rule for manual runs, with the milestone 19 additions to the prefix, once on a private server started by the pinned tmux 3.6a and once on one started by tmux 3.7c from `nixpkgs/nixos-unstable#tmux`. In the 3.7 run, `RINGLEADER_TMUX` pointed the TUI and the CLI at the same 3.7c binary that started the server, so client and server matched. Both runs started with a user `C-q` binding and a user `F12` binding in the root table and two managed sessions made with `ringleader new`. Both passed every check:

- Outside tmux, `C-q` returned to the home screen after switching from the attached managed session to the other one with `C-b )`.
- `C-q` in the unmanaged `holder` session reached its pane, where `cat -v` printed `^Q`, and the client stayed attached.
- A second attach in the same run reused the saved record, and `q` restored the user's `C-q` binding with `F12` untouched.
- After SIGKILL the Ringleader binding remained, and the next start restored the user's binding before any attach.
- Inside tmux, with the TUI in a window of `ringleader-alpha`, `C-q` from the agent window of the same session, from `ringleader-beta` after a session switch, and from `alpha` after a visit to `holder` each brought the client back to the TUI's pane id. `C-q` in `holder` reached its pane. Quitting and a SIGKILL followed by a restart both restored the user's binding.

## Milestone 29

### Files still over the size target

Section 3 says source files target under 200 lines excluding tests and that a file that outgrows that extracts its largest behavior-bearing type. Removing the dead-code allowance came with a review of every production file. These moved out to bring their files under the target: `DiagnosticPaths` and `ToolVersion` from `doctor.rs`, `TmuxError` from `tmux_command.rs`, `LiveProcess` from `system_host_probe.rs`, `WorktreeRollback` from `session_creation.rs`, `DirtyState` and `UnmergedCommits` from the workspace port, `RawConfig` with its TOML parsing from `toml_config_source.rs`, `RepoDraft` from `config_draft.rs`, `CliSession` from `cli.rs`, and `XdgPaths` from `main.rs`. `ProcessList` became a `Display` on `ProcessInfo`, and the session size parser became `SessionSize::parse`. These files still exceed 200 lines, each for the reason given:

- `app.rs`, about 280 lines: see milestone 23. `App` itself is about 186.
- `cli.rs`, about 270 lines: section 3 says the clap argument structs are support types of `Cli` that "get no files of their own".
- `main.rs`, about 335 lines: `CompositionRoot` is the one type left, and its methods are the wiring section 3 assigns to it.
- `tmux_multiplexer.rs`, about 350 lines, `git_workspace.rs`, about 240, `tmux_return_binding.rs`, about 280, and `session_reconciler.rs`, about 260: each holds one type whose support types already have their own files, such as `TmuxScripts`, `TmuxListing`, `TmuxLaunch`, `GitPorcelain`, `GitBranches`, and `BindingRecords`. Splitting further would split the type's own behavior across new collaborators, which no milestone asks for.
- `session_service.rs`, about 240 lines: the facade of section 5, one short delegating method per use case.

### Manual-run rules in AGENTS.md

Section 14 says `AGENTS.md` carries "the section 0 rules for ad-hoc tmux commands and for manual runs of the binary". Choice: the copy includes the refinements recorded for milestones 9 and 19, `sh` as the pane command of `holder` and the `HOME`, `SHELL`, and git environment additions to the prefix, and states the hermetic git environment inline, since `AGENTS.md` has to be self-contained and cannot point to section 10.

### Tree entries in AGENTS.md

Section 14 says that from milestone 29 on the tree in `AGENTS.md` is the file contract and section 3 is not edited again. The files this milestone added are listed only in the `AGENTS.md` tree. That tree also lists `docs/plans/v1-implementation-log.md`, which section 3 never named.

### A binary run outside the throwaway environment

While checking the CLI synopsis for the README, `nix develop -c cargo run -q -- --help` ran the binary outside the section 0 environment. clap printed the help and the process exited during argument parsing, before it read any config, state, or tmux server, so nothing was touched. Later runs went back to the section 0 environment.

