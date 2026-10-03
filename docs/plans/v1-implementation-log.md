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
