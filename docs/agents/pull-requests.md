# Pull requests

**Every rule in this file binds each pull request and each push to one.** CI skips a draft pull request, and runs on a ready one.

1. Open the pull request as a draft with `gh pr create --draft`.
2. Push to the draft freely while you work.
3. When the working commands pass, run `gh pr ready`.
4. Run `gh pr checks --watch`.

**If CI fails, reproduce the failure with the narrowest working command, fix the cause, and push.** Then run `gh pr checks --watch` again.

- If CI also fails on `master` without the change, stop and show the failing output to the project owner.
- NEVER reproduce a CI failure with the full suite or a Nix build.
- If a CI log shows a failure that no working command reproduces, fix the cause from the log.
- If the same failure survives three fixes, stop and show the failing output to the project owner.
