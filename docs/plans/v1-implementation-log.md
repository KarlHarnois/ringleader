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
