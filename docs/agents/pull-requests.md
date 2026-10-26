# Pull requests

**Every rule in this file binds each pull request and each push to one.** CI skips a draft pull request and runs on a ready one.

1. If the branch has no pull request, open one with `gh pr create --draft`.
2. You MAY push to a draft pull request at any time.
3. When the working commands pass, run `gh pr ready`.
4. Run `gh pr checks --watch`.

## CI failures

**If CI fails, run these steps:**

1. Check whether `master` fails the same check, as the next section says. If it does, stop and show the failing output to the project owner.
2. Reproduce the failure with the narrowest working command. If no working command reproduces the failure, read the cause from the CI log.
3. Fix the cause, push, and run `gh pr checks --watch` again.

**NEVER reproduce a CI failure with a command that `AGENTS.md` bans locally.**

**If the same failure survives three fixes, stop and show the failing output to the project owner.**

## The check on `master`

1. Run `gh run list --branch master --workflow CI --status completed --limit 5`.
2. Take the newest run whose conclusion is not `cancelled`. If all five are cancelled, treat `master` as passing.
3. Run `gh run view <id> --json jobs --jq '.jobs[] | {name, conclusion}'` on that run.
4. If the job that failed on the pull request shows `failure` there, `master` fails the same check.
