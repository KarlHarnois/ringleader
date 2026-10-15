# Completion steps

**This file holds the rules for running the metrics tools and the completion steps.** The Metrics section of `AGENTS.md` defines the metrics tools.

## Running the metrics tools

**Run the metrics tools from the repository root.**

- Pass `dryer` and `crapper` the `src` path: a pathless run also walks nested checkouts such as `.claude/worktrees`. A pathless run scores every copy under the same namespaces, and `crapper` reruns coverage for each copy.
- Never run `dryer --changed`: it compares the changed files only with each other. It misses code copied from an unchanged file.

```sh
nix develop .#metrics -c dryer src             # duplicate candidates; writes .metrics/dry.edn
nix develop .#metrics -c crapper src           # CC plus coverage plus CRAP; writes .metrics/crap.edn
nix develop .#metrics -c mutator --max-workers 2 --timeout-factor 60 src/<module>.rs
nix run .#dryer -- src                         # the apps also work outside any dev shell
nix run .#crapper -- --help
nix run .#mutator -- --help
```

## Running `mutator`

**Never run `mutator` on the whole repository:** it runs every covered mutant, and many tests start real tmux servers.

- Name one module or file per run.
- Keep `--max-workers 2` and `--timeout-factor 60`.
- Each run builds the crate from scratch in new worker directories under `target/mutation-workers/`. A mutant that times out counts as killed.
- The default factor, 10 times the warm baseline, ends those builds early. Every mutant then reads as killed before a test runs.
- If you change either value, raise the timeout factor before the worker count: parallel cold builds slow each other.
- If a snapshot exists, a rerun runs only survivors and sites in changed functions. A kill recorded by a timeout therefore stays.
- If a run used a smaller timeout factor, rerun that file once with `--mutate-all`.
- Never pass `--changed` with a path: `mutator` then drops the path and mutates every changed file in `git status`.
- Never pass `mutator` a filtered `--test-command`. Many modules are tested only through their callers. A module-path filter runs zero tests, exits 0, and lets every mutant survive.

## Coverage

**`crapper` and `mutator` need `cargo-llvm-cov` and the LLVM tools named by `LLVM_COV` and `LLVM_PROFDATA`.** The `metrics` shell and the apps set both variables.

If `cargo llvm-cov` fails, from a missing tool or one failing test, every function scores 0% coverage. The run still exits 0. Before you trust a snapshot, check the output for `Rust coverage will score 0%`.

## The steps

**The completion steps are the four numbered steps below.** Run them after the Commands in `AGENTS.md` pass. A `crapper` run takes about a minute. A step 4 run takes two to three minutes.

| Term | Meaning |
| --- | --- |
| Test code | `#[cfg(test)]` code and every file under `src/test_support/` |
| Production code | Every other line under `src/` |
| Changed function | A production function the change's diff adds or edits |
| Changed file | A file under `src/` that holds a changed function |
| Added line | A line the change's diff adds |
| Untestable function | A production function that needs a terminal or reads the process environment. The Tests rules in `AGENTS.md` keep a terminal out of every test, and `clippy.toml` keeps the process environment out. |
| Step edit | Any change to a file under `src/` after step 1 first runs. A change made to pass a later Commands run is a step edit too. |

**Never run a completion step alongside another command:** the tests start real tmux servers and share `target/`.

**Only changed functions decide whether step 1 or step 4 passes.** Never work on the score or the mutants of a function the change did not touch.

**If either case below holds for a command in these steps, stop and show its output to the project owner:**

- The command fails to run.
- The command exits with a code this section does not name.

1. Run `nix develop .#metrics -c crapper src`. Step 1 passes when every changed
   function scores 8 or less.
   - If the output contains `Rust coverage will score 0%`, the coverage run
     failed and no score is real. Fix the cause, then rerun step 1.
   - If a changed function scores above 8, reduce the function's complexity or
     add tests for it.
2. Run `nix develop .#metrics -c dryer src`. Step 2 passes when every candidate
   that includes an added line is resolved.
   - If both sides of a candidate are builders whose whole body is one
     `Self { field: value, ..self }` expression, the candidate is resolved.
     Such builders configure fakes and plans across the codebase and make up
     most candidates.
   - If any other candidate that includes an added line is real duplication,
     remove the duplication.
   - Otherwise, resolve the candidate by telling the project owner why the
     pair is not real duplication.
3. For each changed file, run `nix develop .#metrics -c mutator --scan <file>`.
   The first output line, `Scan: N mutation sites in <file>`, gives the file's
   site count. If N is over 50, tell the project owner and offer to split the
   file. Step 3 always passes.
4. For each changed file, run
   `nix develop .#metrics -c mutator --reuse-coverage --max-workers 2 --timeout-factor 60 <file>`.
   `--reuse-coverage` reads the coverage step 1 wrote. Step 4 passes when every
   changed function's row reads 0 under `Survived` and `Uncovered`. A survivor
   or uncovered site that this section has you report to the project owner
   does not count.
   - Never judge step 4 by its exit code. Exit 3 means a mutant survived
     anywhere in the file, and a run with uncovered sites exits 0.
   - If the run exits 2, the baseline tests failed. Fix the baseline tests,
     which is a step edit.
   - If a changed function that is not untestable has an uncovered site, add
     a test that runs the site.
   - If a test can kill a survivor, add or strengthen that test.
   - If no test can kill a survivor and removing the code it mutates keeps the
     behavior, remove that code.
   - Otherwise, tell the project owner which mutant survived and why no test
     can kill it.

**If a change edits an untestable function, move the function's logic into
functions that tests reach.** Move logic out until the untestable function has
complexity 2 or less. Complexity 2 is the most that scores 8 or less at 0%
coverage. Then tell the project owner which of the untestable function's sites
step 4 reports as uncovered.

**After a step edit, run `nix develop -c cargo fmt`, then restart the completion
steps from step 1.**

- If the step edit changed only test code:
  - Still restart from step 1. `crapper` is the only coverage check for a
    function with no mutation sites.
  - Skip each step 3 or step 4 run that passed on the current production code.
  - Never skip a step 4 run for a changed file that the edited tests cover.
- If the step edit rewrote or deleted an existing test, find each changed file
  that test covered. Add `--mutate-all` to the next step 4 run of each such
  file. Without the flag, a rerun keeps every kill recorded for a function
  whose text did not change.

**If a step edit happened since the Commands last passed, run the Commands again
after the completion steps pass.** None of the metrics tools runs `clippy`.
