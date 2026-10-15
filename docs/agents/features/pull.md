# Pull

**Every rule in this file binds each change to the pull.**

**A pull merges origin's copy of a session's base branch into the session's worktree.** The `p` key starts a pull.

**If any guard below fails, the pull refuses.** The guards keep the merge from changing files under a turn that is still running.

- The session is `Waiting`.
- The session's agent is at its prompt, as `docs/agents/features/agent-hooks.md` defines.
- The worktree has no uncommitted changes.
- The worktree is on the session's branch.

**The pull fetches the first branch below that applies:**

1. the base branch the session recorded from `--base-branch`
2. the repo's configured `base_branch`
3. origin's default branch
4. the first of `origin/main` and `origin/master` that exists

- The pull drops any `origin/` prefix from a recorded or configured base branch.
- If a session works on `main` or `master`, case 4 skips that branch.
- Unlike the diff, which looks for `main` and `master` locally, the pull looks for them on origin.
- The pull fetches only the chosen branch.
- If origin has no branch by the name the pull fetches, the pull refuses.
- The pull never deletes origin's copy of a branch origin dropped: the copy can be the last ref holding commits.

**If origin no longer has the recorded base branch, the pull checks for a local branch of that name.**

- If a local branch of that name remains, the pull refuses. An unpushed base would otherwise give way to origin's default.
- Otherwise, the pull falls back to cases 2 to 4. This case arises after the base branch landed and its session was deleted.

**A pull runs these steps:**

1. Check the guards.
2. Fetch the base branch from origin.
3. Observe the session again, and check the guards on that fresh state. The fetch's network time then stays outside the window the guards close.
4. If the fetched branch does not hold the commit the session started from, refuse. This check catches a session that started from another branch only after that branch moves away from the fetched one.
5. Merge the fetched branch with `--no-edit`.
6. If the merge stops on conflicts, leave the merge in progress, and type the conflict line into the agent.
7. If the merge fails without conflicts, undo the merge with `git merge --abort` or `git reset --merge HEAD`. A merge commit that a hook refuses fails this way. A merge killed at its deadline before git recorded it fails this way too.
   - If the merge commit had already landed, keep the commit and report it as landed. A slow `post-merge` hook that gets killed leaves such a commit.

The pull also refuses a worktree with staged changes or a merge already in progress. That refusal makes the undo in step 7 safe.

**The conflict line names the first 20 conflicted paths and counts the rest.** The line asks the agent to resolve the conflicts and commit.

- The pull types the conflict line through `type_if_ready`.
- If the agent left its prompt, the pull types nothing and the status bar shows an error.
