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

