# Ringleader agent guide

This file is the living reference for working in this repository. If a change makes a statement in this file false, update the statement in the same commit.

## Terms

| Term | Meaning |
| --- | --- |
| Project owner | The person you work for in this repository |
| User | A person who runs Ringleader. The project owner is also a user. |
| Agent | A program from Ringleader's config that a session runs in its tmux pane |
| Repo | A git repository that a session can work in, including a fixture repo |
| This repository | Ringleader's own source repository |
| Private tmux server | The tmux server that Ringleader starts on its own socket |
| Manual run | A run of the `ringleader` binary that you start by hand, TUI or CLI |
| Marked branch | A branch that carries the `ringleaderManaged` git config marker |

## Values

**If no rule covers a case, choose what these values favor.** If two values conflict, the earlier value wins.

### The user's own setup stays untouched

- Ringleader runs the private tmux server and never changes the user's tmux server, tmux config, or key bindings.
- Ringleader deletes or renames only its marked branches.
- Ringleader assumes no particular personal setup.
- Four rules come from this value:
  - Every tmux call passes the private tmux server's socket.
  - Every manual run uses throwaway state.
  - Ringleader marks each branch it creates with `ringleaderManaged`.
  - This repository stays consumer-agnostic.

### Refuse before losing work

If an action could destroy work, Ringleader stops and says why instead of guessing.

- Delete measures uncommitted files and unmerged commits, and asks before it deletes.
- Rename refuses a branch that has been pushed.
- A failed create or rename undoes what it did.
- A refused action exits with code 3.

### Fast and light

- A key press shows its effect on the next frame.
- Ringleader's CPU time, memory, and child processes stay small however many sessions it lists and however long it runs.

### Intuitive, with several ways to do each thing

Each way comes from an ecosystem users already know, and keeps the meaning it has there. A user who guesses a key is right.

| Ecosystem | Keys |
| --- | --- |
| Vim | `j k` move. `gg G` jump to the ends. `H M L` select the top, middle, or bottom row on screen. `/` opens the session finder. |
| fzf and readline | `Ctrl-J Ctrl-K` and `Ctrl-N Ctrl-P` move in the repo search. `Ctrl-W Ctrl-U` edit text. |
| Command palettes | `Ctrl-K` opens the session finder. |

- The arrows work wherever `j k` do.
- `?`, `Esc`, and `q` each close the help.
- Both the TUI and the CLI offer each of these actions:
  - create
  - delete
  - rename
  - restart
  - restart all
  - attach

### Side work never blocks or breaks the main action

- Typing an agent's startup input and looking up pull requests run in the background.
- Ringleader logs each failure of side work and never raises it.
- If a tool such as `gh` is missing, Ringleader turns off that tool's feature instead of failing.
- Logs go to a file, never to the terminal.

### Less is more

Ringleader stays minimal in what it shows, what it asks for, and what it documents.

- Hint lines name only Ringleader's own keys, never the editing keys every terminal user knows.
- The README stays short and holds no architecture.
- Convention wins over configuration:
  - Every setting except the agent list has a working default.
  - Ringleader finds repos by scanning home.
  - A session's branch and tmux session follow from its name.
  - Ringleader finds the diff base without a setting.
  - The new session dialog starts on the repo Ringleader was launched in, else on the newest session's repo.
- Add a setting only for a case that convention cannot serve.

## Ground rules

### Functions

- Keep each function small. Make each function do one thing at one level of abstraction.
- Place each caller above the functions it calls.
- Never write a flag argument.
- Never write a side effect that the function's name does not announce.
- Follow command-query separation: a function either changes state or answers a question, never both.
- State each piece of knowledge once.
- Talk to direct collaborators instead of reaching through chains of objects.
- Return a failure as a `Result` whose message names the operation that failed.
- Return absence as an `Option`.

### Comments

**A comment is a failure to express intent in code.** A doc comment is a comment and follows the same rules.

For each comment:

1. Revisit the code the comment explains.
2. Improve the code until the comment is unnecessary. Rename a variable, extract a function, introduce a type, or restructure the control flow.
3. If the comment does not convincingly fall under an exception below, delete the comment.
4. Otherwise, justify the comment to the project owner before you commit.

The exceptions are:

- a legal header
- non-obvious intent that structure cannot encode
- a warning of a real consequence
- a TODO that references a tracked issue

Never add a comment to justify messy code or to stand in for a better name. Never announce a surviving comment in the commit body. A commit that adds an unjustified comment is not done.

### Object-oriented Rust

- Keep state in structs with private fields. Never write a public data field.
- Put behavior in `impl` blocks.
- Use traits as interfaces.
- Use composition instead of inheritance.
- Inject dependencies through constructors.
- Never write a free function except `main` and `#[test]` functions: Rust requires those to be free.
- Make each test helper that is not a `#[test]` function a method or associated function of a fixture struct.
- If one file's tests use a fixture struct, put the struct in that file's `mod tests`.
- If tests in several files share a fixture struct, put the struct in `src/test_support/`.

### Names

**Names reveal intent.**

- Name a type with a noun.
- Name a method that acts with a verb.
- Name an accessor after what it returns.
- Never put an abbreviation, a number series, or a noise word in a name.
- Never write a magic number or a magic string. If a value's meaning is not obvious, make the value a named constant.

### Pattern names

**If a type's role matches an established pattern, name the type after that pattern instead of inventing a name.**

- If your first name for a type is a gerund, look for a pattern.
- If your first name for a type says how the type works, not its role, look for a pattern.
- If no pattern fits, name the type after its domain role.
- Never reshape a type to fit a pattern.
- Never give a type a pattern name for a role it does not play.

A hook is a user-configured action that runs when an event happens. The type that runs an agent's hooks is therefore `AgentHooks`, with `after_start` and `after_rename`, never `AgentTyping`.

The codebase already uses these patterns:

- adapters
- guards
- plans
- requests
- fakes
- fixtures

### `nix develop`

**Build, test, and run every tool inside `nix develop`.** The host `PATH` can carry other versions of `cargo`, `rustc`, and `tmux`.

- If `direnv allow` has run in this repository, `.envrc` loads the same dev shell through direnv.
- Run the commands in this file as written. Their `nix develop -c` prefix makes them work without direnv.
- Stage each new file by path with `git add` before any `nix` command: flake commands ignore files outside the git index.

### Ad-hoc tmux commands

**An ad-hoc tmux command is a tmux command you run by hand, such as a binding experiment.** Run every ad-hoc tmux command on a throwaway server:

1. If a manual run made `<tempdir>`, use that directory. Otherwise, make a directory with `mktemp -d` under `/tmp`. That directory is `<tempdir>`.
2. Run each command as `env -u TMUX -u TMUX_PANE tmux -u -S <tempdir>/x.sock -f /dev/null ...`. Without that prefix, a tmux command reaches the project owner's tmux server: `nix develop` passes `$TMUX` through.
3. End with `kill-server` on `<tempdir>/x.sock`.

- Keep the socket path under 100 bytes: tmux rejects a longer path with `File name too long`.
- Never touch these tmux servers:
  - the default server
  - the server named in `$TMUX`
  - the project owner's private tmux server, at `$XDG_RUNTIME_DIR/ringleader/tmux/tmux.sock`

### Manual runs

**Every manual run uses throwaway state and fixture repos.** `main` resolves the real XDG paths, which hold the private tmux server's socket.

**The run prefix is the command prefix below, followed by the hermetic git environment.** The prefix's `HOME=<tempdir>/home` is the hermetic git environment's `HOME`.

```sh
env -u TMUX -u TMUX_PANE -u RINGLEADER_CONFIG \
  XDG_CONFIG_HOME=<tempdir>/config XDG_STATE_HOME=<tempdir>/state \
  XDG_DATA_HOME=<tempdir>/data XDG_CACHE_HOME=<tempdir>/cache \
  XDG_RUNTIME_DIR=<tempdir>/run HOME=<tempdir>/home SHELL=sh
```

- Each throwaway XDG directory keeps the run off the project owner's real one, including the real private tmux server.
- `HOME`, `SHELL`, and the hermetic git environment keep the project owner's shell and git startup files out of the run.

To make a manual run:

1. Make `<tempdir>` with `mktemp -d /tmp/rl.XXXXXX`.
2. In `<tempdir>`, run `mkdir -p config/ringleader state data cache run repos home`.
3. Write `<tempdir>/config/ringleader/config.toml` with one stand-in agent: `name = "stand-in"` and `command = "sh"`. Never configure `claude` or `opencode` as the agent.
4. Build fixture repos under `<tempdir>/repos` with the hermetic git environment.
5. Start an outer tmux server under the ad-hoc tmux command rules. The outer server stands in for the user's tmux server. Drive the outer server with `send-keys`.
6. Run each check in the table below in a pane of the outer server. Run the binary by absolute path from `<tempdir>/repos`, so no real repo is in scope.
7. After each check, assert that the outer server's `list-keys` output and session list are unchanged.
8. Run `kill-server` on the outer server. Then run `kill-server` with `-S <tempdir>/run/ringleader/tmux/tmux.sock`.
9. Remove `<tempdir>`.

| Check | How to run the binary |
| --- | --- |
| Plain | In a plain pane, with the run prefix |
| Nested | In the same pane, with the run prefix minus `-u TMUX -u TMUX_PANE`, so the client opens nested |
| Shell | In a `t` shell of a session. The binary MUST refuse with exit code 3. |

**If a check fails, run steps 8 and 9, then report the failure to the project owner.** A failed check is one of these:

- a check whose step 7 assertion fails
- a Shell check whose binary does not refuse with exit code 3

Send every tmux command for a manual run's private tmux server with the run prefix and `-S <tempdir>/run/ringleader/tmux/tmux.sock`.

### Hermetic git environment

**The hermetic git environment is these variables:**

- `GIT_CONFIG_GLOBAL=/dev/null`
- `GIT_CONFIG_NOSYSTEM=1`
- `HOME` set to a throwaway directory
- fake values for `GIT_AUTHOR_NAME`, `GIT_AUTHOR_EMAIL`, `GIT_COMMITTER_NAME`, and `GIT_COMMITTER_EMAIL`

Create each fixture repo with `git init -b <branch>`. Never rely on `init.defaultBranch`.

### This repository

- The private GitHub repository exists and is the `origin` remote. Never create another repository. Never add another remote.
- **Keep this repository consumer-agnostic.** Never reference or assume a specific downstream setup, local directory layout, or personal configuration.
- Give examples and fixtures generic names, such as `example-project`.
- Capitalize the name Ringleader in human-facing prose.
- Keep identifiers lowercase:
  - the binary, the crate, and the repository name
  - the flake input and config paths
  - the tmux prefix
  - JSON fields
- The git config marker `ringleaderManaged` keeps git's camelCase.

## Commands

```sh
nix develop -c cargo fmt --check
nix develop -c cargo clippy --all-targets -- -D warnings
nix develop -c cargo test
nix build --no-link .#checks.x86_64-linux.readme-version-badge
nix build .#
nix flake check
```

**The Commands are the six commands above.** `.github/workflows/ci.yml` runs all six on pushes to `master` and on merge-queue runs. The workflow runs the first four on pull requests too.

**A change is done when the Commands pass.** If the change edits a file under `src/`, the change is done only after the completion steps also pass. If one of the Commands also fails without the change, stop and show its output to the project owner.

## Architecture

**Ringleader follows hexagonal architecture, also called ports and adapters.** The core is the `domain` and `application` layers.

- The core never imports an adapter, `crossterm`, `ratatui`, or `std::process`.
- The core never runs tmux or git, and never touches the filesystem or the terminal.
- The core reaches the outside through traits it owns, which adapters outside the core implement.
- The core never spells out an outside tool's syntax. Git refs, git config keys, tmux targets, and command lines belong to the adapters.
- If the core shows the user a command, the core asks a port for the command. For example, `BranchOwnership` asks `Workspace::branch_marking_command`.

**Dependencies point inward only.** An inner layer never imports an outer layer.

### `domain`

`domain` holds the entities, value types, and pure rules. `domain` imports nothing else from the crate.

### `application`

`application` holds the use cases and the ports, and imports only `domain`.

- `application` never shells out, never touches the filesystem directly, and never reads the system clock.
- An `application` error says what kind of failure happened. Only `CliExit` in `cli` turns an error into an exit code.
- A port is a trait in `application/ports/`, named after the role it plays, such as `Workspace` or `Multiplexer`.
- If a port has an error type, the error type lives in the port's file.
- A port's error describes the failure in the port's terms. The error has no field for the tool's subcommand, exit status, or output.
- Nothing outside the adapters branches on which tool failed.

### `adapters`

`adapters` holds the driven adapters. A driven adapter implements a port with git, tmux, files, or `/proc`.

- `adapters` also holds the process-level types that `CompositionRoot` uses directly, such as `ProcessEnvironment`, `XdgPaths`, and `AgentLauncher`.
- `adapters` imports `application` and `domain`, never `ui` or `cli`.
- Name each port adapter after its technology and its port, such as `GitWorkspace` or `TmuxMultiplexer`.

### `ui` and `cli`

`ui` and `cli` are the driving adapters.

- `ui` and `cli` import `application` and `domain`, never `adapters` or each other.
- `ui` and `cli` reach the use cases only through the `SessionService` facade.
- `SessionService` composes these use cases, which `ui` and `cli` never call directly:
  - `SessionLifecycle`
  - `SessionMonitor`
  - `SessionAccess`
  - `RepoCatalog`
  - `PullRequestCatalog`
  - `Doctor`
- A `ui` or `cli` type that receives a port, such as `Lock`, calls that port itself.
- `ui` and `cli` hold no business rules. `domain` or `application` decides what each session state allows, so the TUI and the CLI cannot disagree.

### `main.rs`

`main.rs` is the composition root, and the only production code that imports every layer.

- `main.rs` builds and wires. The Wiring section says what `main.rs` builds.
- The driving adapter, not `main.rs`, calls the use cases a run starts with. For example, `Cli::execute` calls them for the CLI.

### Test code

- `src/test_support/` MAY import any layer: it fakes the ports and drives the outer layers under test.
- An inline `#[cfg(test)]` module follows its layer's rule. Any inline `#[cfg(test)]` module MAY also import `src/test_support/`.

### Known violations

**The code breaks these rules in four places.** Each place is a violation to fix. Never copy a violation as precedent.

1. Advice in these files spells out tmux and git command lines:
   - `domain/unmanaged_session.rs`
   - `application/busy_worktree.rs`
   - `application/server_audit.rs`
   - `application/delete_outcome.rs`
   - `application/deletion_guards.rs`
   - `application/session_renaming.rs`

   The tmux command lines repeat the exact-target syntax that `TmuxScripts::exact` owns.
2. `WorkspaceError`, `MultiplexerError`, and `PullRequestError` each name the subcommand that failed. `WorkspaceError` and `PullRequestError` also keep the subcommand's exit status and stderr in fields. `ServiceError::TimedOut` names the tool, and `ui/diff_queue.rs` and `ui/home_state.rs` branch on the tool.
3. `ui/action_gate.rs` decides which actions each session state allows, such as restart only for an Exited or Stopped session. The use cases enforce their own versions of those rules.
4. `CompositionRoot::run_tui` in `main.rs` calls the TUI's startup use cases itself: `restore_server_socket`, `refuse_own_pane`, `load_registry`, and `reapply_server_config`.

### New ports

**If `application`, `ui`, or `cli` needs a capability from outside the process, route the capability through a port.** None of those layers imports `adapters`.

- `CompositionRoot` and the types in `adapters` reach the outside directly.
- Logging needs no port. Every layer writes through the `log` macros, and `CompositionRoot` installs `AppLog` behind them.

To add a port:

1. Define the trait in a new file under `application/ports/`, named after the role.
   - If any of the trait's methods can fail, define an error type beside the trait.
   - Re-export the trait and its error type from `ports/mod.rs`.
2. Implement the trait in `adapters/`, in a type named after the technology and the role.
3. Add a hand-written fake in `src/test_support/`.
   - If the use cases need the port, wire the fake into `ServiceFixture`.
   - Otherwise, wire the fake into `CliHarness` or `UiHarness`.
4. Inject the port as a trait object.
   - If the use cases need the port, add the port to a bundle in `ServicePorts` as an `Arc<dyn Trait>`.
   - Otherwise, hand the port from `CompositionRoot` to the type that uses it, as with `Lock`.
5. Build the adapter in `CompositionRoot` and pass it in.
   - If the host can lack the technology behind the port, pass a null adapter. A null adapter does nothing on such a host. `NullPullRequests` is the null adapter for a host without `gh`.
   - A port is never an `Option`.

### Driving adapter I/O

**A driving adapter's own channel to the user needs no port.**

- For the TUI, the channel is the terminal, including the shell the TUI runs there.
- For the CLI, the channel is the standard streams.
- Keep the channel's I/O in `ui` or `cli`.
- Put the channel behind a trait beside its real implementation, or behind a std `Read` or `Write`.
- Put the channel's fake in `src/test_support/`.
- The TUI owns `InputSource`, `ScreenMode`, and `ShellRunner` this way. The CLI writes through the `Write` streams that `CliOutput` holds.
- A test seam inside `adapters`, such as `AgentLauncher`'s `ExecSeam`, has the same shape. Its fake lives in the adapter's own tests.

## Metrics

**The metrics tools are `dryer`, `crapper`, and `mutator`.** They are local code-health tools that you run in the completion steps.

- CI never builds or runs the metrics tools. Never add them to `checks`, to the default dev shell, or to `.github/workflows/ci.yml`.
- The flake builds the metrics tools as `apps`, so `nix flake check` only evaluates them.
- The `metrics` dev shell extends the default one and holds the metrics tools.
- `nix flake check` still downloads the four upstream inputs. If an upstream input can no longer be fetched, CI's flake check fails until someone repins or removes the input.
- The metrics tools write snapshots under `.metrics/`, which is gitignored.

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

### Running `mutator`

**Never run `mutator` on the whole repository:** it runs every covered mutant, and many tests start real tmux servers.

- Name each module or file to mutate as an argument. One run MAY name several. A run's files share one baseline test run.
- The flake applies `patches/mutator-copy-debug-build.patch` to `mutator`. With the patch, each worker under `target/mutation-workers/` starts from a copy of `target/debug` without its `incremental` directory. A worker's build then recompiles only the `ringleader` crate.
- `flake.nix` pins `mutator`, and the `crapper` that `mutator` imports, to the revisions the patch was tested with. `nix flake update` therefore leaves both in place.
- If you move either pin, move both pins together. Then run `nix develop .#metrics -c mutator --scan src/main.rs`. The `mutator` build refuses a patch that applies only with fuzz. The scan imports every `crapper` module that `mutator` uses.
- If the `mutator` build fails on the patch, rewrite the patch against the new `mutator` revision.
- If `nix develop .#metrics -c mutator --scan src/main.rs` fails for any other reason, stop and show its output to the project owner.
- Keep `--max-workers 2` and `--timeout-factor 60`.
- A mutant that times out counts as killed. Under load, a worker's build can take several times longer than the baseline test run. A smaller factor then records kills that no test made.
- If you change `--max-workers` or `--timeout-factor`, raise the timeout factor before the worker count: parallel builds slow each other.
- If a snapshot exists, a rerun runs only survivors and sites in changed functions. A kill recorded by a timeout therefore stays.
- If a run used a smaller timeout factor, rerun its files once with `--mutate-all`.
- Never pass `--changed` with a path: `mutator` then drops the path and mutates every changed file in `git status`.
- Never pass `mutator` a filtered `--test-command`. Many modules are tested only through their callers. A module-path filter runs zero tests, exits 0, and lets every mutant survive.
- Never edit a file in this repository while a `mutator` run is in progress. An edit made during a run reaches the mutant builds. `mutator` also overwrites an edited file when the file's mutants finish.
- Never start a `mutator` run in the background. If the shell moves a run to the background, wait for the run to end.

### Coverage

**`crapper` and `mutator` need `cargo-llvm-cov` and the LLVM tools named by `LLVM_COV` and `LLVM_PROFDATA`.** The `metrics` shell and the apps set both variables.

If `cargo llvm-cov` fails, from a missing tool or one failing test, every function scores 0% coverage. The run still exits 0. Before you trust a snapshot, check the output for `Rust coverage will score 0%`.

### uml-viewer

uml-viewer reads `.metrics/crap.edn` and `.metrics/mutate/`. uml-viewer ignores `.metrics/dry.edn`, so `dryer` output stays a text report and never reaches the diagram. uml-viewer needs a display, and downloads Maven dependencies on its first run.

Write this policy to the gitignored `.uml-viewer/ringleader.policy.edn`:

```edn
{:title "Ringleader"
 :sources [{:lang :rust :root "src" :prefix "ringleader"}]
 :hierarchical true
 :out ".uml-viewer/ringleader.edn"}
```

**Leave the policy-level `:prefix` unset.** The overlay joins a `crate::module::Type` snapshot to the module class through the class id's crate path. Stripping the crate name from the id breaks that join.

The `metrics` shell sets `$UML_VIEWER_DEPS`, which points the Clojure CLI at the `uml-viewer` flake input. Inside `nix develop .#metrics`, generate the IR, then open the viewer:

```sh
clojure -Sdeps "$UML_VIEWER_DEPS" -M:ir .uml-viewer/ringleader.policy.edn
viewer_tmux_dir=$(mktemp -d /tmp/uml.XXXXXX)
env -u TMUX -u TMUX_PANE TMUX_TMPDIR="$viewer_tmux_dir" \
  clojure -Sdeps "$UML_VIEWER_DEPS" -M:run --restart .uml-viewer/ringleader.edn
rm -rf "$viewer_tmux_dir"
```

**Always pass `--restart`.** Without `--restart`, the viewer starts `grok --yolo --trust` as its companion, in a new session on the project owner's tmux server. The viewer then opens on a blank canvas until that companion answers. `--restart` skips the companion and loads the EDN at once.

The viewer still looks for a companion with `tmux has-session`. The `env` prefix sends those lookups to an empty private directory, which the commands remove after the viewer exits.

### Completion steps

**The completion steps are the four numbered steps below.** Run them after the Commands pass. A `crapper` run takes about a minute. A step 4 run takes about 20 seconds per file in the run, plus about 6 seconds per mutation site.

| Term | Meaning |
| --- | --- |
| Test code | `#[cfg(test)]` code and every file under `src/test_support/` |
| Production code | Every other line under `src/` |
| Changed function | A production function the change's diff adds or edits |
| Changed file | A file under `src/` that holds a changed function |
| Added line | A line the change's diff adds |
| Untestable function | A production function that needs a terminal or reads the process environment. The Tests rules keep both out of every test. |
| Step edit | Any change to a file under `src/` after step 1 first runs. A change made to pass a later Commands run is a step edit too. |
| Edited test | A test that a step edit adds, rewrites, or deletes |

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
4. Run
   `nix develop .#metrics -c mutator --reuse-coverage --max-workers 2 --timeout-factor 60 <files>`.
   `<files>` is every changed file, separated by spaces. If a rule below
   splits `<files>` into parts, make one run per part. Otherwise, pass all of
   `<files>` to one run. `--reuse-coverage` reads the coverage step 1 wrote.
   Step 4 passes when every changed function's row reads 0 under `Survived`
   and `Uncovered`. A survivor or uncovered site that this section has you
   report to the project owner does not count.
   - The run limit is the longest timeout your shell allows for one command.
     Give each step 4 run the run limit as its timeout.
   - Estimate each run's time with the step 4 run time at the top of this
     section. If twice the estimate passes the run limit, split the run into
     smaller runs. The estimate is an average, and load slows a run.
   - If twice one file's estimate alone passes the run limit, stop and show
     the estimate to the project owner.
   - Never judge step 4 by its exit code. Exit 3 means a mutant survived
     anywhere in the run, and a run with uncovered sites exits 0.
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
  - Skip each step 3 run that passed on the current production code.
  - If a changed file's functions passed step 4 on the current production
    code, leave the file out of `<files>`.
  - Never leave out of `<files>` a changed file that an edited test covers or
    covered before the step edit.
  - If `<files>` is then empty, skip step 4.
- If the step edit rewrote or deleted an existing test, find each file in
  `<files>` that the test covered. Split `<files>` into two step 4 runs:
  - Run the covered files with `--mutate-all`. Without the flag, a rerun keeps
    every kill recorded for a function whose text did not change.
  - Run the other files without the flag: `--mutate-all` reruns every killed
    mutant of every file in its run.
  - If either run would name no file, skip that run.

**If a step edit happened since the Commands last passed, run the Commands again
after the completion steps pass.** None of the metrics tools runs `clippy`.

## Layout

```
.
├── Cargo.toml              # rust-version = "1.95", edition = "2024", license = "MIT"
├── Cargo.lock
├── rustfmt.toml            # max_width = 100
├── LICENSE                 # MIT
├── .gitignore              # /target, /result, /result-*, /.direnv, /.claude/worktrees, /.metrics, /.uml-viewer, /uml-viewer-log.txt
├── flake.nix
├── flake.lock
├── patches/
│   └── mutator-copy-debug-build.patch # each mutator worker starts from a copy of target/debug
├── .envrc                  # direnv: use flake, loads the dev shell
├── .github/workflows/ci.yml
├── README.md               # human readers, terse
├── AGENTS.md               # agent repo documentation, living
├── tests/fixtures/         # fake config, registry, tmux and git output
└── src/
    ├── main.rs             # fn main and CompositionRoot
    ├── cli.rs              # Cli struct, clap definitions, dispatch
    ├── cli/
    │   ├── cli_output.rs         # CliOutput, writeln! output that stops at a broken pipe
    │   ├── cli_exit.rs           # CliExit, the exit of one command and the exit code of each service and lock error
    │   ├── cli_session.rs        # CliSession, signal checks, worker waits, the startup input wait the CLI and TUI end with, and the exit of one command
    │   ├── session_commands.rs   # SessionCommands: list, new, delete, attach, rename, restart
    │   ├── list_report.rs        # ListReport, the list text and JSON
    │   ├── repo_argument.rs      # RepoArgument, the --repo name or path resolution
    │   ├── repo_commands.rs      # RepoCommands: repo add, remove, list
    │   └── delete_report.rs      # DeleteReport, the delete warnings and results
    ├── domain/
    │   ├── mod.rs
    │   ├── session.rs            # Session
    │   ├── session_name.rs       # SessionName
    │   ├── session_state.rs      # SessionState
    │   ├── session_metadata.rs   # SessionMetadata
    │   ├── session_observation.rs # SessionObservation
    │   ├── session_classifier.rs # SessionClassifier, the pure state classifier
    │   ├── diff_stat.rs          # DiffStat
    │   ├── merge_bases.rs        # MergeBases, the best common ancestors a session diff may measure from, and the one it prefers
    │   ├── session_summary.rs    # SessionSummary
    │   ├── unmanaged_session.rs  # UnmanagedSession, a live tmux session Ringleader does not manage, its label, and the note or kill-session advice that explains why
    │   ├── agent.rs              # Agent
    │   ├── agent_mode.rs         # AgentMode, build or plan, the mode a new session's agent starts in
    │   ├── repo.rs               # Repo
    │   ├── repo_name.rs          # RepoName
    │   ├── repo_draft.rs         # RepoDraft, a [[repos]] entry before the load checks
    │   ├── base_branch.rs        # BaseBranch, a requested, configured, or origin default base branch and the branches it is looked up as
    │   ├── base_branch_ladder.rs # BaseBranchLadder, the origin default, main, and master branches tried when no base branch is set, never a recorded session's branch
    │   ├── branch.rs             # Branch, a local or remote-tracking branch named as users type it, and origin's copy of a branch
    │   ├── branch_tips.rs        # BranchTips, the commits the existing ones of some branches point at
    │   ├── config.rs             # Config, the parsed config.toml
    │   ├── config_draft.rs       # ConfigDraft, the raw config values the Config constructor checks
    │   ├── config_checks.rs      # ConfigChecks, the checks every config load runs
    │   ├── naming_scheme.rs      # NamingScheme, the worktree root and tmux prefix a session records
    │   ├── pull_request.rs       # PullRequest, its number, state, and opening time
    │   ├── hot_key.rs            # HotKey, a key a terminal can send, which every key binding is made of
    │   ├── terminal_text.rs      # TerminalText, text written for a terminal, and the same text without its escape sequences
    │   └── key_bindings.rs       # KeyBindings, the return key that detaches back to Ringleader, the keys that switch to the next and previous session, and the keys that toggle between the agent and the shell
    ├── application/
    │   ├── mod.rs
    │   ├── session_service.rs    # SessionService, the facade the UI and CLI call
    │   ├── service_context.rs    # ServiceContext and LoadedConfig, what main injects into the facade
    │   ├── service_ports.rs      # ServicePorts with the StatePorts, RepoPorts, TmuxPorts, and DesktopPorts bundles
    │   ├── service_error.rs      # ServiceError, the kinds of failure the use cases report
    │   ├── session_lifecycle.rs  # SessionLifecycle: create, delete, rename, restart, merge
    │   ├── busy_worktree.rs      # BusyWorktree, the tmux server processes that keep a stopped entry from restarting, with the advice for each case
    │   ├── unseen_session.rs     # UnseenSession, a tmux server Ringleader cannot see that may still hold a stopped entry's session, which rename refuses and delete refuses or warns about
    │   ├── session_creation.rs   # SessionCreation: create_session with its rollbacks
    │   ├── start_point_resolver.rs # StartPointResolver, the commit a new worktree starts from, or why there is none
    │   ├── base_branch_resolver.rs # BaseBranchResolver, the base branch a session's diff follows: its recorded branch while that still exists, else its repo's configured one
    │   ├── base_branch_ladders.rs # BaseBranchLadders, the base branch ladder of a checkout, led by origin's default and skipping the repo's session branches, which the start point, diff base, and upstream resolvers share
    │   ├── worktree_rollback.rs  # WorktreeRollback, removes the worktree and branch of a failed create
    │   ├── creation_plan.rs      # CreationPlan and CreateRequest
    │   ├── use_checks.rs         # UseChecks, the checks a create or a restart runs before acting
    │   ├── session_renaming.rs   # SessionRenaming: rename_session with its branch rollback
    │   ├── branch_renaming.rs    # BranchRenaming, moves a session's branch to its new name, back when the rename fails, and drops the upstream left on the old name once the rename is kept
    │   ├── rename_request.rs     # RenameRequest, the old and new names
    │   ├── branch_availability.rs # BranchAvailability, refuses a branch name that exists or holds other branches, for create and rename
    │   ├── session_restart.rs    # SessionRestart: restart of an Exited or Stopped session
    │   ├── restart_report.rs     # RestartReport, the outcome of each restart in a restart all, and the summary the TUI shows
    │   ├── session_scratch.rs    # SessionScratch, makes a session's scratch directory before its agent starts, removes it with the session, and logs either failure
    │   ├── session_envrc.rs      # SessionEnvrc and EnvrcChoice, whether a session's agent starts through its worktree's .envrc, with the warning when it cannot
    │   ├── start_outcome.rs      # StartOutcome, the session a create or restart started, its .envrc warning, and the socket recovery note
    │   ├── session_merge.rs      # SessionMerge: merge of origin's copy of the base branch into a Waiting session
    │   ├── merge_report.rs       # MergeReport, what a pull did to a session and whether its agent was asked to resolve the conflicts
    │   ├── upstream_resolver.rs  # UpstreamResolver, the branch a pull fetches from origin, or why there is none
    │   ├── agent_hooks.rs        # AgentHooks, schedules an agent's startup input in the background and types its rename input
    │   ├── worktree_inspection.rs # WorktreeInspection, the worktree checks restart, delete, and doctor share
    │   ├── session_deletion.rs   # SessionDeletion: delete_session steps 5 to 8 and forget
    │   ├── deletion_guards.rs    # DeletionGuards: delete_session steps 2 to 4
    │   ├── branch_ownership.rs   # BranchOwnership, refuses to delete or rename a branch without the Ringleader marker
    │   ├── delete_impact.rs      # DeleteImpact and DeleteConfirmation
    │   ├── delete_outcome.rs     # DeleteOutcome
    │   ├── worktree_containment.rs # WorktreeContainment, shared by delete step 3 and reconciliation
    │   ├── session_monitor.rs    # SessionMonitor: observation, reconciliation, diff measurement, and the background jobs for observation, diff measurement, and preview capture
    │   ├── diff_base_resolver.rs # DiffBaseResolver, the commit a session diff measures from
    │   ├── session_reconciler.rs # SessionReconciler, pairs registry entries with live tmux sessions
    │   ├── reconciliation.rs     # Reconciliation, the reconciled entries and rows
    │   ├── session_access.rs     # SessionAccess: attach, open_shell, and open_terminal, and the refusal from a pane of the private server
    │   ├── repo_catalog.rs       # RepoCatalog: discover, find, add, and remove repos
    │   ├── pull_request_catalog.rs # PullRequestCatalog, looks up the pull requests of sessions in the background
    │   ├── repo_merge.rs         # RepoMerge, joins configured and scanned repos, configured entries winning, and finds the config entry and the sessions of a repo by its canonical path
    │   ├── home_repo_search.rs   # HomeRepoSearch, the repos under home the repo search offers
    │   ├── default_repo.rs       # DefaultRepo, the launch repo or the newest session's repo
    │   ├── typed_paths.rs        # TypedPaths, a typed path read as a directory or as a repo
    │   ├── repo_overrides.rs     # RepoOverrides: the add_repo and remove_repo workers
    │   ├── pending.rs            # Pending, a worker result the TUI polls and the CLI waits on
    │   ├── doctor.rs             # Doctor
    │   ├── diagnostic_paths.rs   # DiagnosticPaths, the program, state, and private tmux server paths doctor and help report, and the log file help shows
    │   ├── tool_version.rs       # ToolVersion, the major and minor numbers of a tmux or git version
    │   ├── doctor_report.rs      # DoctorReport, the doctor's info, warning, and problem lines
    │   ├── registry_audit.rs     # RegistryAudit, the doctor's checks of each registry entry
    │   ├── orphan_audit.rs       # OrphanAudit, the doctor's search for worktrees and marked branches no session claims
    │   ├── server_audit.rs       # ServerAudit, the doctor's checks of Ringleader's tmux server and the key bindings
    │   └── ports/
    │       ├── mod.rs
    │       ├── session_registry.rs   # SessionRegistry trait
    │       ├── workspace.rs          # Workspace trait
    │       ├── dirty_state.rs        # DirtyState, the uncommitted files and nested repos of a worktree
    │       ├── head.rs               # Head, whether a checkout's HEAD is unborn, detached, or on a branch
    │       ├── unmerged_commits.rs   # UnmergedCommits, commits on no other ref with the tip and HEAD
    │       ├── merge_outcome.rs      # MergeOutcome, whether a worktree's merge of a branch from origin was up to date, merged, or conflicted
    │       ├── multiplexer.rs        # Multiplexer trait
    │       ├── agent_launch.rs       # AgentLaunch, the argv, environment, and worktree an agent starts with
    │       ├── config_source.rs      # ConfigSource trait
    │       ├── repo_scanner.rs       # RepoScanner trait
    │       ├── scratch_space.rs      # ScratchSpace trait
    │       ├── directory_environment.rs # DirectoryEnvironment trait and Envrc, a directory's .envrc with its bytes, whether it is allowed or denied, and the command that allows it
    │       ├── pull_requests.rs      # PullRequests trait
    │       ├── terminal_emulator.rs  # TerminalEmulator trait and TerminalEmulatorError, opens a terminal window in a directory
    │       ├── host_probe.rs         # HostProbe trait
    │       ├── clock.rs              # Clock trait
    │       ├── lock.rs               # Lock trait
    │       └── interrupts.rs         # Interrupts trait
    ├── adapters/
    │   ├── mod.rs
    │   ├── agent_launcher.rs     # AgentLauncher, the hidden __exec-agent mode, owns AgentHandoff and the exec seam, and execs direnv in front of a second launcher for a direnv launch
    │   ├── launcher_command.rs   # LauncherCommand and PreparedLaunch, the launcher words a pane runs and the handoff file they name
    │   ├── agent_handoff.rs      # AgentHandoff, the handoff file a launch writes and the launcher takes
    │   ├── handoff_contents.rs   # HandoffContents and EnvrcStep, the argv, inherited and explicit variables, worktree, and .envrc step a handoff file carries
    │   ├── app_log.rs            # AppLog, the append-mode app.log behind env_logger
    │   ├── process_environment.rs # ProcessEnvironment, the environment snapshot main captures once, and the shell it names
    │   ├── xdg_paths.rs          # XdgPaths, the config, state, data, cache, and runtime directories, the scratch root, and the private tmux server's socket and config
    │   ├── directory_repo_scanner.rs # DirectoryRepoScanner
    │   ├── directory_scratch_space.rs # DirectoryScratchSpace, one private scratch directory per worktree inside Ringleader's cache directory
    │   ├── mount_table.rs        # MountTable, the network and FUSE mount points the scan skips, and whether anything is mounted inside a scratch directory
    │   ├── toml_config_source.rs # TomlConfigSource
    │   ├── raw_config.rs         # RawConfig, the config.toml shape before the load checks
    │   ├── config_file_writer.rs # ConfigFileWriter, the read-only checks and mode-keeping config write
    │   ├── json_session_registry.rs # JsonSessionRegistry
    │   ├── file_lock.rs          # FileLock
    │   ├── fnv_hash.rs           # FnvHash, the FNV-1a hash of the server config and of a worktree path
    │   ├── gh_pull_requests.rs   # GhPullRequests, looks up the pull requests of a repo's branches in one gh GraphQL call
    │   ├── gh_query.rs           # GhQuery, the aliased GraphQL query for a repo's branches
    │   ├── gh_listing.rs         # GhListing, the viewer's or repo owner's newest pull request per branch in a gh response
    │   ├── null_pull_requests.rs # NullPullRequests, finds no pull request on a host without gh
    │   ├── direnv_directory_environment.rs # DirenvDirectoryEnvironment, reads a directory's .envrc, asks direnv whether it is allowed, and allows it
    │   ├── null_directory_environment.rs # NullDirectoryEnvironment, finds no .envrc on a host without direnv
    │   ├── system_terminal_emulator.rs # SystemTerminalEmulator, asks xdg-terminal-exec which terminal it opens, starts the terminal program in a directory, in a session of its own, and logs a failed exit
    │   ├── terminal_program.rs   # TerminalProgram, xdg-terminal-exec with --dir, the program $TERMINAL names, or ghostty +new-window with --working-directory, since a running Ghostty ignores the directory of a plain launch
    │   ├── null_terminal_emulator.rs # NullTerminalEmulator, opens no window on a host without a graphical display or a terminal, and says which it lacks
    │   ├── bounded_command.rs    # BoundedCommand, runs git, tmux, gh, direnv, and xdg-terminal-exec children in a new session with a deadline
    │   ├── deadlines.rs          # Deadlines and CallKind, the deadline of each kind of child call
    │   ├── command_output.rs     # CommandOutput, a finished child's status and output
    │   ├── host_path.rs          # HostPath, lenient canonicalization and directory listing
    │   ├── git_workspace.rs      # GitWorkspace
    │   ├── git_runner.rs         # GitRunner, runs a GitInvocation through BoundedCommand
    │   ├── git_invocation.rs     # GitInvocation, one git call with its child environment
    │   ├── git_porcelain.rs      # GitPorcelain, parses git's machine-readable output
    │   ├── git_refs.rs           # GitRefs, HEAD, the default branch a remote's HEAD names, and the tips of branches
    │   ├── git_diffs.rs          # GitDiffs, the merge bases of a worktree's HEAD and its diff stat
    │   ├── branch_ref.rs         # BranchRef, the git ref name of a Branch and the Branch a ref name spells
    │   ├── git_branches.rs       # GitBranches: compare-and-swap delete, rename, the managed-branch marker, and whether a branch has been pushed
    │   ├── branch_marker.rs      # BranchMarker, the git config entry that marks a branch Ringleader created, and the command that sets it by hand
    │   ├── path_option.rs        # PathOption, a command line option that takes a path in the same argument, such as --dir=<path>
    │   ├── shell_word.rs         # ShellWord, one word of a command Ringleader advises, quoted when a shell would split or expand it
    │   ├── branch_section.rs     # BranchSection, the git config section and upstream keys of one branch
    │   ├── upstream_merger.rs    # UpstreamMerger, fetches a branch from origin and merges origin's copy into a worktree, listing any conflicted paths
    │   ├── submodule_commits.rs  # SubmoduleCommits, the submodule walk of unmerged_commits
    │   ├── system_clock.rs       # SystemClock, reads `SystemTime`
    │   ├── system_host_probe.rs  # SystemHostProbe
    │   ├── live_process.rs       # LiveProcess, one /proc entry, whether it is this user's tmux server of a socket, and signals to it
    │   ├── tmux_command_line.rs  # TmuxCommandLine, the socket a tmux argv names in its global flags and the process name tmux gives a server it starts
    │   ├── metadata_json.rs      # MetadataJson, the @ringleader_meta JSON each session carries
    │   ├── signal_hook_interrupts.rs # SignalHookInterrupts, the SIGHUP, SIGINT, SIGTERM, and SIGQUIT flags
    │   ├── durable_file.rs       # DurableFile, synced temp file renamed over the target
    │   ├── tmux_command.rs       # TmuxCommand, the private server's `-S` and `-f`, `-u` and `-N`, a child environment without `TMUX` and `TMUX_PANE`, the caller's identity
    │   ├── server_files.rs       # ServerFiles, the private server's socket and generated config, the 0700 socket directory, and whether a server still starts under the socket's lock
    │   ├── tmux_error.rs         # TmuxError
    │   ├── tmux_sequence.rs      # TmuxSequence, `;`-separated commands with `#` and `;` escaping
    │   ├── tmux_output.rs        # TmuxOutput, labeled lines and the zero-session errors
    │   ├── tmux_scripts.rs       # TmuxScripts, every tmux command sequence the multiplexer sends, exact targets, start directories
    │   ├── tmux_launch.rs        # TmuxLaunch, ringleader-created and ringleader-done handling, rollback, tag move
    │   ├── tmux_agent_input.rs   # TmuxAgentInput, waits for the agent's prompt and types lines into its pane
    │   ├── tmux_listing.rs       # TmuxListing, parses list-panes, list-windows, list-keys, and server rows
    │   ├── tmux_multiplexer.rs   # TmuxMultiplexer
    │   ├── key_switch.rs         # KeySwitch, the copy-mode bindings each newly bound key displaces, saved in server options, and the ones each dropped key displaced, replayed
    │   ├── private_server.rs     # PrivateServer, the private server's config files, return binding check, state, config reapply that unbinds the keys the config dropped, and lost socket recovery
    │   ├── running_config.rs     # RunningConfig, the hash and key bindings a running server reports
    │   ├── tmux_server_config.rs # TmuxServerConfig, the generated config of the private tmux server with its return, session switch, and shell toggle bindings, its hash, and the script that reapplies it
    │   └── tmux_shell_toggle.rs  # TmuxShellToggle, the tmux command a shell toggle key runs: the agent window from the shell window, else the shell window, opened in the worktree when missing
    ├── ui/
    │   ├── mod.rs
    │   ├── app.rs                  # App
    │   ├── action_gate.rs          # ActionGate, which actions the selected row allows
    │   ├── back_key.rs             # BackKey, Ctrl-Q, which closes any dialog and never quits
    │   ├── key_sequence.rs         # KeySequence, the g that waits for a second g to make gg
    │   ├── home_state.rs           # HomeState, the polled rows, selection, and preview
    │   ├── session_list.rs         # SessionList, rows sorted by repo, state, and name, the selected session or heading, the folded repos, and the viewport
    │   ├── list_selection.rs       # ListSelection, the selected session or repo heading
    │   ├── observation_feed.rs     # ObservationFeed, the observation background job and its timer
    │   ├── preview_feed.rs         # PreviewFeed, the preview capture background job for the selected row and its timer
    │   ├── diff_queue.rs           # DiffQueue, round-robin diff measurement background jobs for visible rows, at most two at a time
    │   ├── pull_request_feed.rs    # PullRequestFeed, the pull requests of visible rows and their timer
    │   ├── bell.rs                 # Bell, rings on a Working to Waiting edge
    │   ├── shell_runner.rs         # ShellRunner, runs the ephemeral t shell
    │   ├── blocking_operation.rs   # BlockingOperation, a create, delete, rename, restart, or merge run behind a busy message
    │   ├── status_report.rs        # StatusReport, the status bar message and how outcomes become one
    │   ├── text_fit.rs             # TextFit, padding and ellipsis truncation
    │   ├── event_loop.rs           # EventLoop
    │   ├── terminal_guard.rs       # TerminalGuard
    │   ├── home_view.rs            # HomeView
    │   ├── empty_state.rs          # EmptyState, the no-sessions message with the logo above it when it fits
    │   ├── session_table.rs        # SessionTable, the home screen's session list, drawn as a tree of repos
    │   ├── session_tree.rs         # SessionTree, the sorted rows as repo headings and branch lines, and the lines a viewport shows
    │   ├── viewport.rs             # Viewport, the lines the session table shows, which move only to keep the selection in view, and the lines H M L select
    │   ├── repo_heading.rs         # RepoHeading, a repo's heading line with its number and fold arrow, and its session and state counts while folded
    │   ├── repo_group.rs           # RepoGroup, the repo a row is listed under, or other
    │   ├── colored_row.rs          # ColoredRow, a row's text split around its coloured cells
    │   ├── state_indicator.rs      # StateIndicator, the glyph and color of a session state
    │   ├── table_columns.rs        # TableColumns, column widths and dropping, and RowCells, the cell texts of one row
    │   ├── preview_panel.rs        # PreviewPanel, the Full tier's preview
    │   ├── preview_text.rs         # PreviewText, the rows of a preview capture, parsed once, and the rows that fit the preview
    │   ├── help_view.rs            # HelpView
    │   ├── label_column.rs         # LabelColumn, the help's padded label and the indent of its wrapped rows
    │   ├── legend.rs               # Legend, the help's symbols and their meanings, wrapped under a label
    │   ├── status_bar.rs           # StatusBar
    │   ├── theme.rs                # Theme
    │   └── dialogs/
    │       ├── mod.rs
    │       ├── dialog.rs                # Dialog, the open dialog, its key routing and placement
    │       ├── dialog_slot.rs           # DialogSlot, the dialog App shows, if any
    │       ├── dialog_labels.rs         # DialogLabels, full or abbreviated field labels
    │       ├── text_field.rs            # TextField, typing and paste rules for name and path fields
    │       ├── new_session_dialog.rs    # NewSessionDialog
    │       ├── new_session_view.rs      # NewSessionView, draws the n dialog's fields, mode, error, and hints into its area
    │       ├── new_session_layout.rs    # NewSessionLayout, the rows the n dialog gives its repo search, mode, prompt, and error
    │       ├── prompt_field.rs          # PromptField, the n dialog's multiline prompt on ratatui-textarea
    │       ├── new_session_field.rs     # NewSessionField, the n dialog's fields in Tab order with their labels and hints
    │       ├── repo_picker.rs           # RepoPicker, the n dialog's Repo field and its search
    │       ├── chosen_repo.rs           # ChosenRepo, the default repo until the search picks another
    │       ├── repo_finder.rs           # RepoFinder, the fuzzy search over the repos under home
    │       ├── repo_filter.rs           # RepoFilter, ranks repo paths against a fuzzy query
    │       ├── fuzzy_ranker.rs          # FuzzyRanker, ranks names or paths against a fuzzy query
    │       ├── match_list.rs            # MatchList, the ranked matches of a fuzzy search and the cursor over them
    │       ├── repo_scan.rs             # RepoScan, one home search and its result
    │       ├── search_query.rs          # SearchQuery, a finder's typed query shown after its prompt
    │       ├── typed_repo_check.rs      # TypedRepoCheck, the git check of a typed repo path
    │       ├── home_path.rs             # HomePath, a path shown from ~ when it is under home
    │       ├── agent_picker.rs          # AgentPicker, the n dialog's agent list
    │       ├── delete_session_dialog.rs # DeleteSessionDialog
    │       ├── rename_session_dialog.rs # RenameSessionDialog
    │       └── session_finder.rs        # SessionFinder, the / and Ctrl-K search over session names that attaches to the pick
    └── test_support/               # #[cfg(test)] doubles and fixtures shared by unit tests
        ├── mod.rs
        ├── tmux_test_server.rs      # TmuxTestServer: private socket, generated config, managed sessions, control-mode and nested clients, kill-server on drop
        ├── git_test_repo.rs         # GitTestRepo: hermetic git environment and fixture repos
        ├── service_fixture.rs       # ServiceFixture: every fake wired into a SessionService
        ├── fake_session_registry.rs # FakeSessionRegistry
        ├── fake_workspace.rs        # FakeWorkspace
        ├── fake_multiplexer.rs      # FakeMultiplexer
        ├── fake_config_source.rs    # FakeConfigSource
        ├── fake_repo_scanner.rs     # FakeRepoScanner
        ├── fake_scratch_space.rs    # FakeScratchSpace
        ├── fake_directory_environment.rs # FakeDirectoryEnvironment
        ├── direnv_test_home.rs      # DirenvTestHome: a temporary HOME, XDG_DATA_HOME, and XDG_CONFIG_HOME for tests that run a real direnv
        ├── executable_script.rs     # ExecutableScript: writes a test script through a child process, so running the script never fails with Text file busy
        ├── fake_pull_requests.rs    # FakePullRequests
        ├── fake_terminal_emulator.rs # FakeTerminalEmulator
        ├── log_capture.rs           # LogCapture, the test logger that keeps warnings for assertions
        ├── fake_host_probe.rs       # FakeHostProbe
        ├── fake_clock.rs            # FakeClock
        ├── call_hooks.rs            # CallHooks, the hooks FakeMultiplexer and FakeWorkspace run before a call
        ├── gate.rs                  # Gate: a test gate that blocks a call until it is opened or a deadline passes
        ├── held_calls.rs            # HeldCalls: holds the calls that reach a call hook on a Gate and counts them
        ├── deadline.rs              # Deadline: polls a condition until it holds or a time limit passes
        ├── recording_screen_mode.rs # RecordingScreenMode, the screen mode that records each switch
        ├── ui_harness.rs            # UiHarness: scripted input and a TestBackend wrapper for EventLoop tests
        ├── cli_harness.rs           # CliHarness: a Cli run against the fakes with captured output
        ├── fake_lock.rs             # FakeLock
        └── fake_interrupts.rs       # FakeInterrupts
```

## Layout rules

### Files

- **Put one primary type in each file.** Name the file after that type in snake case.
- Supporting types that the primary type owns live in the same file.
- Unit tests live inline.
- A `mod.rs` file only declares and re-exports.
- **The Layout tree is the file contract.** Every listed file exists and holds its one primary type.
- Never merge primary types into one file.
- If a commit adds or removes a file under `src/`, update the Layout tree in that commit. A move or a rename counts as both.

### File size

**The size target of a source file is 200 lines, excluding tests.** The files under `src/test_support/` count as tests.

- If a file is over the size target, extract its largest behavior-bearing type into its own file.
- If a file's only behavior-bearing type is its primary type, the file MAY stay over the size target.
- Even if the clap argument structs keep `cli.rs` over the size target, they stay in `cli.rs`.
- Never split one type's own behavior across new collaborators only to meet the size target.

### Which types get a file

**If a type meets all three tests below, give the type its own file.** Merging files later is mechanical, and splitting a file that grew is disruptive.

- The type is used across modules.
- The type carries behavior or invariants.
- The type changes for its own reasons.

**Otherwise, keep the type in its primary type's file, with no tree entry.** Such support types include a parser result, a doctor finding, a session spec, and a dialog's in-progress state.

- `cli.rs` owns every clap argument struct, such as `ListArgs` and `NewArgs`, as a support type of `Cli`.
- `src/test_support/` is `#[cfg(test)]` only and never appears in the binary. It holds the hand-written doubles and fixtures that tests across modules share.

### Process inputs

**No code below `main` does any of these:**

- call `std::env::current_exe`
- read the process environment. The process environment includes `$TMUX`, `$TMUX_PANE`, `$HOME`, and `$SHELL`.
- read the process working directory
- resolve XDG paths
- ask which terminal standard input is

`main` reads these inputs once and injects them. Environment values travel in the `ProcessEnvironment` snapshot.

`ProcessEnvironment::capture` reads the terminal name of standard input with `libc::ttyname_r` on descriptor 0. That call is the type's only `unsafe` block.

- Every `unsafe` block carries one `// SAFETY:` comment that states why the call is sound.
- Confine each `unsafe` block to one adapter method.

### tmux targets

**Every tmux target is exact.** An exact target is one of these:

- `=<session>:` from `TmuxScripts::exact`
- a session id
- a window id
- a pane id

**Never pass a bare `-t <name>`:** tmux resolves a name with no exact match by prefix. A bare `-t fix` would reach `fix-auth`.

- If a session's name starts with `$`, target the session by its id. tmux reads `=$0:` as the session whose id is `$0`.
- `TmuxMultiplexer` looks up a session's id by its name.

### tmux start directories

**Every `new-session`, `new-window`, and `respawn-window` passes `-c` with the worktree.** Without `-c`, tmux starts the pane in the caller's directory and still exits 0.

- A command Ringleader sends gets its `-c` from `TmuxScripts::start_in`.
- A binding in the generated config passes `-c '#{session_path}'`. Ringleader renders the config before any session exists, and `create_session` starts each session in its worktree.

## Wiring

### Entry points

- `main.rs` holds `fn main` and `CompositionRoot`.
- `CompositionRoot`'s methods build the adapters, the `SessionService`, and the TUI or CLI run.
- Argument parsing and dispatch stay in `Cli`.
- `ringleader` with no subcommand opens the TUI.
- Any subcommand runs the CLI and never starts the TUI.
- The hidden `__exec-agent` path loads no config, opens no log, and takes no lock.

### Locks

| Entry point | Lock it takes |
| --- | --- |
| The TUI | The lifetime lock |
| `new` | The write lock |
| `delete` | The write lock |
| `rename` | The write lock |
| `restart` | The write lock |
| `repo add` | The write lock |
| `repo remove` | The write lock |
| `list` | None |
| `doctor` | None |
| `attach` | None |
| `repo list` | None |
| `__exec-agent` | None |

### Logs

Logs go to `$XDG_STATE_HOME/ringleader/app.log` through `env_logger`.

### Sessions and branches

- **A session's branch is its name.** A session's tmux session is the tmux prefix followed by its name.
- Create and rename never record a branch that differs from the session's name.
- Rename always moves the branch with the name.
- Rename refuses a branch that has been pushed: the remote branch and any pull request keep the old name.
- Rename never moves the worktree: the running agent and its transcripts are keyed by the worktree path.
- A session recorded on another branch keeps working. Such a session comes from one of these:
  - a session created before the branch prefix was dropped
  - an adopted tmux session
  - a tmux session renamed by hand
- If such a session's branch is marked and not pushed:
  - `doctor` warns about the session.
  - A rename of the session to a new name moves its branch.

### Generated tmux config

**Key bindings come only from the generated config of the private tmux server.** `create_session` writes the generated config.

- If the running server's `@ringleader_config` hash differs from the current render, three callers re-source the generated config:
  - the TUI at start
  - `new`
  - `restart`
- `r` re-sources the generated config whenever the server runs. A `tmux source-file` run from a pane can change the server without changing the `@ringleader_config` hash.
- Only a lock holder writes or re-sources the generated config, so only one process ever writes the generated config.
- If `config.toml` failed to load, Ringleader skips every re-source.

### Private tmux server

- Every tmux call passes `-S <private socket>` and `-f`, so Ringleader never reads or writes the user's tmux server.
- Ringleader checks a registry entry recorded on another socket only through `/proc`.
- The TUI and the CLI attach through the same foreground child.

### Blocking operations

**The TUI runs each blocking operation behind a busy message.** The blocking operations are:

- create
- delete
- rename
- restart
- restart all
- pull
- the delete measurement

After a blocking operation, the event loop drops the key and paste events queued while the operation ran.

### Background jobs

**No background job blocks input.**

A background job is a `Pending` worker that runs the tmux, git, or gh calls of one TUI task. The task is one of these:

- an observation
- a preview capture
- a diff measurement
- a pull request lookup

An observation is one tmux listing of every session. The TUI reconciles each observation with the registry. A background job is in flight from its start until the event loop collects it.

- The event loop collects finished observations, preview captures, and diff measurements on every wake.
- The event loop starts and collects pull request lookups only on its 100 millisecond tick.
- While an observation, a preview capture, or a diff measurement is in flight, the event loop wakes every 16 milliseconds.
- A pull request lookup never shortens the event loop's wait: one lookup can take 20 seconds per repo.
- The TUI shows a preview capture only under the row the capture was taken for.

### Synchronous observations

**A synchronous observation is an observation the TUI runs on its own thread instead of as a background job.**

- The TUI runs a synchronous observation before its first frame. Without that observation, the first frame and the keys typed at launch see no sessions.
- The TUI runs a synchronous observation after every attach, shell, reload, and blocking operation.
- A synchronous observation drops any observation in flight: an observation listed before a delete would bring the deleted session back.
- A synchronous observation restarts the `state_poll_ms` interval.

### Diff measurements

- Diff measurements read git through their own `GitWorkspace`. `CompositionRoot` hands that `GitWorkspace` to `RepoPorts::with_diff_workspace`.
- Every read of the diff `GitWorkspace` has the 5 second diff deadline.
- A slow repo therefore keeps a measurement in flight for at most 5 seconds per git call. The event loop stays on its 16 millisecond wakes for that time.
- Create, pull, and the other use cases read under the 60 second deadline.

### Restart

**Restart revives an Exited or Stopped session in its worktree.**

| State | Restart | The agent starts with |
| --- | --- | --- |
| Exited | Respawns the agent in the session's tmux session | The agent's `args` |
| Stopped | Recreates the session's tmux session | The agent's `args`, then its `resume_args` |

- A Stopped session lost its tmux server, such as to a reboot. `resume_args` resumes the conversation that the lost server cut off.
- Never add `resume_args` to a respawn: a respawn is the fallback for a resume that found no conversation and exited.
- Create never adds `resume_args`.

**Restart all is a restart of every Exited or Stopped session, one at a time, in name order.** `ringleader restart --all` and the `S` key start a restart all.

- Never let one reconciliation serve several restarts in a restart all. Each recreation saves the registry from its reconciliation, so a shared reconciliation drops the sockets of earlier recreations.
- A failed restart never stops the rest. Restart all reports each failure.
- `restart --all` exits with the code of its first failed restart. If no restart failed, it exits 0.
- If a signal arrives during a restart all, the current restart finishes and Ringleader skips the rest.

### Pull

**A pull merges origin's copy of a session's base branch into the session's worktree.** The `p` key starts a pull.

**If any guard below fails, the pull refuses.** The guards keep the merge from changing files under a turn that is still running.

- The session is `Waiting`.
- The session's agent is at its prompt, as the Agent hooks section defines.
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

### Hint lines

**A dialog's hint line lists only two kinds of keys:**

- keys whose meaning is Ringleader's own, such as `Enter create`, `Ctrl-S create`, and `Esc cancel`
- keys that choose from a list, such as `↑ ↓ j k choose`

A hint line never lists a text-editing key: every terminal user already expects those keys. Text-editing keys include cursor movement, `Ctrl-U`, and `Ctrl-W`.

Add a new editing key to `TextField` with its usual terminal meaning, never to a hint.

### Scratch directories

**A session's scratch directory is `$XDG_CACHE_HOME/ringleader/tmp/<hash>`.** The hash is the FNV-1a hash of the recorded worktree path, folded to 8 hex digits.

- Keep the scratch directory's path short: tools put unix sockets under `TMPDIR`, and a socket path cannot pass 107 bytes.
- The scratch root is never `/tmp` or Ringleader's own `TMPDIR`, for these reasons:
  - systemd ages `/tmp` under an idle agent.
  - `/tmp` can be `noexec` or small.
  - `TMPDIR` differs between the process that creates a session and the process that deletes it. Every agent's `TMPDIR` is its own scratch directory, and `nix develop` exports a new `TMPDIR` per shell.
- Create and restart make the scratch directory. Unless the agent's `set_env` sets `TMPDIR`, they start the agent with `TMPDIR` set to the scratch directory.
- If making the scratch directory fails, the agent starts with the inherited `TMPDIR`.
- Delete removes the scratch directory after the worktree. Forget keeps the scratch directory.
- Delete keeps a scratch directory that holds a mount point: removing it would delete the files of the mounted filesystem.
- If the mount table cannot be read, delete keeps every scratch directory.
- Ringleader logs a failure to make or remove a scratch directory. The failure never fails the use case.

### direnv launches

**A direnv launch starts the agent as `direnv exec <worktree> <agent command> <agent args>`.** A plain launch starts the agent without direnv.

Create and restart choose the launch by the first case below that matches. Restart includes the respawn of an Exited session and the recreation of a Stopped one.

1. If the worktree has no `.envrc`, or `direnv` is not on Ringleader's `PATH`, do a plain launch with no warning.
2. If direnv reports the worktree's `.envrc` as allowed, do a direnv launch.
3. If direnv reports the worktree's `.envrc` as denied, do a plain launch with no warning.
4. If both conditions below hold, run `direnv allow <worktree>`, then do a direnv launch.
   - The worktree's `.envrc` is byte-identical to the `.envrc` of the session's repo path.
   - direnv reports the repo path's `.envrc` as allowed.
5. Otherwise, do a plain launch and warn the user. The warning names the worktree and says that its `.envrc` is not allowed. The warning says that the agent started without that `.envrc`. The warning tells the user to review the `.envrc` and run `direnv allow <worktree>`.

- If `direnv` is missing, `DirenvDirectoryEnvironment::when_installed` gives `CompositionRoot` a `NullDirectoryEnvironment`.
- Never allow a denied `.envrc`: `direnv deny <worktree>` is how a user keeps one session's agent off that `.envrc`.
- **Never run `direnv allow` on an `.envrc` that differs by any byte from the repo path's `.envrc`.** A branch can change its `.envrc`, and allowing the changed file runs code the user has not reviewed.
- A repo path with no `.envrc` differs from every worktree `.envrc`.
- If `direnv allow` fails, do a plain launch and warn with direnv's error.
- A warning never fails the create or the restart. `new` and `restart` print the warning on stderr and exit 0. The TUI shows the warning in the status bar.

### direnv variables

| Term | Meaning |
| --- | --- |
| Baseline variables | The variables every agent starts with, including `HOME` and `XDG_*` |
| Inherited variables | The baseline variables, plus each variable in the agent's `env` list that Ringleader's environment sets |
| Explicit variables | The variables the second launcher sets on top of the `.envrc` environment, listed below |

- Ringleader's `direnv status` and `direnv allow` calls run with only the baseline variables. An `.envrc` the user allowed in their own shell therefore counts as allowed.
- The `direnv exec` in the agent's pane runs with the inherited variables. The `.envrc` load therefore sees the agent's `env` values.
- If the agent's `env` list names a set `DIRENV_CONFIG`, the `.envrc` load reads that direnv config. Ringleader's `direnv status` and `direnv allow` calls do not read it.

The explicit variables are:

- each variable in the agent's `env` list that Ringleader's environment sets
- the `SHELL` Ringleader started with
- the agent's `set_env` values
- the `TMPDIR` of the scratch directory

### direnv launch steps

1. The launcher in the agent pane writes a second handoff file.
2. The launcher execs `direnv exec <worktree> ringleader __exec-agent --handoff <second file>` with only the inherited variables.
3. The second launcher runs inside the environment the `.envrc` produced. The second launcher execs the agent with the explicit variables on top.

- Each step execs the next, so the agent stays the pane's own process.
- The explicit variables keep `SHELL`: a dev shell exports its own non-interactive bash, which would replace the user's shell.
- Every other variable, including `PATH`, keeps the value the `.envrc` produced.
- Every handoff write, CLI command, and TUI start deletes the handoff files older than 60 seconds.
- The cleanup keeps a second handoff file while the process that wrote the file still runs. `direnv exec` keeps that process through the `.envrc` load, which can outlast 60 seconds.
- The `t` shell never loads the `.envrc`.
- Rename keeps the running agent and its environment.

### Agent hooks

**After create and restart, `AgentHooks` schedules the agent's `startup_input` in the background.** The use case returns once the tmux session exists. A typing job is the background work that types one agent's `startup_input`.

- One typing job runs per session at a time. `AgentHooks` skips a second start for the same session.
- The job finds the agent pane once, when the job starts. Before each line, the job checks only the prompt conditions below.
- The job never rechecks the session's name or registry entry. A rename during the job's wait therefore does not reach the job.
- The job waits up to 60 seconds for the first prompt.
- Before each later line, the job pauses briefly so the agent can take the previous line. The job then waits up to 2 seconds for the prompt to come back.
- The job sends each line as its text, then sends Enter 300 milliseconds later. An agent reads text and an Enter that arrive together as a paste. The agent then keeps the Enter in its draft and never submits the line.
- The TUI polls the job while the job runs.
- If the agent is at its prompt right after a rename, `AgentHooks` types the agent's `rename_input`. Otherwise, `AgentHooks` skips the `rename_input`.
- Ringleader logs a skipped or failed typing. The typing never fails the use case.

**On exit, the CLI and the TUI each run these steps.** The CLI runs them after its command, and the TUI runs them after restoring the screen.

1. Release the lock.
2. Note the wait on stderr.
3. Wait for the typing job. If a signal arrives, stop waiting.

**If all three conditions below hold, the agent is at its prompt:**

| Condition | Reason |
| --- | --- |
| The pane is out of copy mode. | Copy mode would take the keys instead of the agent. |
| The pane shows the cursor. | Claude hides the cursor in its trust and permission dialogs. |
| The text before the cursor on its row is a bare prompt marker and a blank. That text holds no letters or digits. | An agent still starting has drawn no marker. An unsent draft puts its text before the cursor. |

## CLI exit codes

| Code | Meaning |
| --- | --- |
| 0 | Success |
| 1 | A user or validation error, including an argument error, and any other failure |
| 2 | A session name that was not found |
| 3 | A conflict. Conflicts include a refused guard, an `Unmanaged` target, a live and healthy session, and a held lock. |

**If a signal interrupts a command, the command's behavior depends on when the signal arrives:**

| The signal arrives | The command |
| --- | --- |
| Before its use case starts | Never runs the use case |
| During its use case | Finishes the use case, then ends by that signal |
| While it waits for queued startup input | Stops waiting, logs the abandoned input, and ends by that signal. The use case still succeeded. |

## Tests

- All tests are unit tests inside `src/`, in `#[cfg(test)] mod tests` or `src/test_support/`.
- `tests/fixtures/` holds only fixtures, with fake data.
- Never write a `tests/*.rs` integration test.
- Run application tests against the hand-written fakes in `src/test_support/`, wired by `ServiceFixture`.
- Never add a mocking crate.
- Test one concept per test.
- Test only through public interfaces.
- Git adapter tests run real git in `tempfile` repos with the hermetic git environment. Set that environment on each `Command`, never through `std::env::set_var`.
- Use a child process to write every file that a test executes. A file that the test process writes itself can fail to execute with `Text file busy`. If the file is a script, write the script with `ExecutableScript`.
- UI tests render with `TestBackend`.
- UI tests drive `EventLoop` with scripted input, a recording screen mode, and a recording shell runner.
- No test calls crossterm or needs a terminal.
- No test reads wall-clock time to classify a session.
- No test reads the process environment, except its own `PATH`.
- Mark a constructor, accessor, or helper that only tests call with `#[cfg(test)]`: `cargo clippy --all-targets -- -D warnings` then stays clean without any dead-code allowance.

### tmux tests

- tmux tests never skip.
- Each tmux test starts its own server through `TmuxTestServer`, on a private socket under `/tmp`.
- The test server loads the rendered generated config. If the test is about the config itself, the test server loads a fixture config instead.
- Each tmux test builds its snapshot from the test process's own `PATH`.
- A test that needs a client holds a control-mode client.
- Press keys through the control-mode client with `send-keys -K -c <client>`. Never press keys with `send-keys -t <pane>`, which skips the key tables.
- Only a test of a nested client's own parsing MAY type raw terminal bytes with `send-keys -t <pane> -H`. That test types into a pane of a second test server that holds the nested client.

## Commits

- Give each change one commit. Make the commit after the change is done, as the Commands section defines done.
- Start the title with a capital letter, write it in the imperative mood, and keep it under 72 characters.
- Write the body in plain language, and say what changed.
- Push only when the project owner asks.

## Documentation

- Keep `README.md` terse and human-facing.
- `README.md` covers only these topics:
  - what Ringleader is
  - install
  - first run
  - the CLI synopsis
  - the exit codes
- Never put architecture or the rules of this file in `README.md`.
- This repository holds no `CLAUDE.md` or `CLAUDE.local.md`: either file would hide this file from Claude Code.
- If one of those files is ever needed, make its first line `@AGENTS.md`.
