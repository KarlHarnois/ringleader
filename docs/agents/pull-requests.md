# Pull requests

**Every rule in this file binds each pull request and each push to one.** CI skips a draft pull request and runs on a ready one.

1. If the branch has no pull request, open one with `gh pr create --draft`.
2. You MAY push to a draft pull request at any time.
3. When the working commands pass, suggest a release label to the project owner, as the next section says.
4. Run `gh pr ready`.
5. Run `gh pr checks --watch`.

## Release labels

**The labels of a pull request set the version that its merge releases.**

| Label | The change | Example |
| --- | --- | --- |
| none | fixes a defect or changes no behavior | `v0.1.4` to `v0.1.5` |
| `release:minor` | adds a feature, or breaks a config, a CLI command, or a key before 1.0 | `v0.1.5` to `v0.2.0` |
| `release:major` | is the release the project owner names as 1.0, or breaks behavior after 1.0 | `v0.2.0` to `v1.0.0` |

**Add a release label only after the project owner confirms it.**

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
