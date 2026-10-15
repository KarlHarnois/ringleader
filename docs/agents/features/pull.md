# Pull

**Every rule in this file binds each change to the pull.** A pull merges origin's copy of a session's base branch into the session's worktree.

**If any of its guards fails, the pull refuses.** The guards keep the merge from changing files under a turn that is still running.

- Check the guards again after the fetch, on a fresh observation of the session. The fetch's network time then stays outside the window the guards close.
- Refuse a worktree with staged changes or a merge already in progress. That refusal keeps the undo of a failed merge safe.
- If a merge fails without conflicts, undo the merge. If the merge commit already landed, keep the commit and report it as landed. A slow `post-merge` hook killed at its deadline leaves such a commit.
- Never delete origin's copy of a dropped branch: the copy can be the last ref that holds the branch's commits.
- If origin no longer has the recorded base branch and a local branch of that name remains, refuse the pull. Without that refusal, an unpushed base would give way to origin's default.
- The pull looks for `main` and `master` on origin, unlike the diff, which looks for them locally.
