# Ringleader agent guide

This file and the reference files it names are the living reference for working in this repository. If a change makes a statement in any of them false, update the statement in the same commit.

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

### Let the code speak

The code, its names, and its tests describe what Ringleader does. This file and its reference files hold only what the code cannot say.

- Never write prose that restates what the code shows. Such prose includes a file listing, a type's responsibilities, and the steps a function takes.
- If a name, a type, or a test can carry an intent, carry the intent there instead of in prose. A test name MAY state the reason for the behavior the test checks.
- An automated check is a test, a lint, or a CI command. If an automated check can enforce a rule, enforce the rule there instead of in prose.
- If a reader of the code alone would make a mistake without a statement, keep the statement. Otherwise, delete the statement.

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

## Reference files

**Each reference file holds rules that apply only to one kind of task. Before you start that task, read the file.** A reference file binds as fully as this file does.

| Reference file | Read the file before you |
| --- | --- |
| `docs/agents/completion-steps.md` | run a metrics tool or the completion steps |
| `docs/agents/manual-runs.md` | make a manual run |
| `docs/agents/new-ports.md` | add a port |
| `docs/agents/uml-viewer.md` | open uml-viewer |
| `docs/agents/features/pull.md` | change the pull that the `p` key starts |
| `docs/agents/features/direnv.md` | change how create or restart launches an agent, or which variables the agent gets |
| `docs/agents/features/agent-hooks.md` | change `AgentHooks`, the exit wait for its typing job, or the check that an agent is at its prompt |
| `docs/agents/features/scratch-directories.md` | change a session's scratch directory |
| `docs/agents/features/background-jobs.md` | change the TUI's observations, preview captures, diff measurements, or pull request lookups |

**Never `@`-import a reference file:** an import loads the file in every session.

## Architecture

**Ringleader follows hexagonal architecture, also called ports and adapters.** The core is the `domain` and `application` layers.

- The core reaches the outside through traits it owns, which adapters outside the core implement.
- The core never spells out an outside tool's syntax. Git refs, git config keys, tmux targets, and command lines belong to the adapters.
- If the core shows the user a command, the core asks a port for the command. For example, `BranchOwnership` asks `Workspace::branch_marking_command`.

**Dependencies point inward only.** The tests in `main.rs` check the imports of every layer.

### `domain`

`domain` holds the entities, value types, and pure rules.

### `application`

`application` holds the use cases and the ports.

- `application` never reads the system clock.
- An `application` error says what kind of failure happened. Only `CliExit` in `cli` turns an error into an exit code.
- A port is a trait in `application/ports/`, named after the role it plays, such as `Workspace` or `Multiplexer`.
- If a port has an error type, the error type lives in the port's file.
- A port's error describes the failure in the port's terms. The error has no field for the tool's subcommand, exit status, or output.
- Nothing outside the adapters branches on which tool failed.

### `adapters`

`adapters` holds the driven adapters. A driven adapter implements a port with git, tmux, files, or `/proc`.

- `adapters` also holds the process-level types that `CompositionRoot` uses directly, such as `ProcessEnvironment`, `XdgPaths`, and `AgentLauncher`.
- Name each port adapter after its technology and its port, such as `GitWorkspace` or `TmuxMultiplexer`.

### `ui` and `cli`

`ui` and `cli` are the driving adapters.

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

### Known violations

**Each issue below tracks code that breaks a rule of this file.** Never use the code an issue names as precedent for new code. Run `gh issue view <number>` to see the files and functions an issue names.

| Issue | Violation |
| --- | --- |
| #58 | The core's messages spell out tool command lines. |
| #59 | Port errors carry tool details, and two TUI types branch on those details. |
| #60 | `ActionGate` decides which actions each session state allows. |
| #61 | `CompositionRoot::run_tui` calls the TUI's startup use cases. |

### New ports

**If `application`, `ui`, or `cli` needs a capability from outside the process, route the capability through a port.** None of those layers imports `adapters`.

- `CompositionRoot` and the types in `adapters` reach the outside directly.
- Logging needs no port. Every layer writes through the `log` macros, and `CompositionRoot` installs `AppLog` behind them.

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

## Layout rules

### Files

- **Put one primary type in each file.** Name the file after that type in snake case.
- Supporting types that the primary type owns live in the same file.
- Unit tests live inline.
- A `mod.rs` file only declares and re-exports.
- Never merge primary types into one file.

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

**Otherwise, keep the type in its primary type's file.** Such support types include a parser result, a doctor finding, a session spec, and a dialog's in-progress state.

- `cli.rs` owns every clap argument struct, such as `ListArgs` and `NewArgs`, as a support type of `Cli`.
- `src/test_support/` is `#[cfg(test)]` only and never appears in the binary. It holds the hand-written doubles and fixtures that tests across modules share.

### Process inputs

**Only `main` reads the process inputs, and `main` injects them.** The process inputs are these:

- the environment
- the working directory
- the executable path
- the XDG paths
- the terminal of standard input

`clippy.toml` refuses every other read. Never add an `#[expect(clippy::disallowed_methods)]`: each existing one marks a reader that `main` calls, or the `PATH` that tests inherit.

Confine each `unsafe` block to one adapter method.

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

### Hint lines

**A dialog's hint line lists only two kinds of keys:**

- keys whose meaning is Ringleader's own, such as `Enter create`, `Ctrl-S create`, and `Esc cancel`
- keys that choose from a list, such as `↑ ↓ j k choose`

A hint line never lists a text-editing key: every terminal user already expects those keys. Text-editing keys include cursor movement, `Ctrl-U`, and `Ctrl-W`.

Add a new editing key to `TextField` with its usual terminal meaning, never to a hint.

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
- Git adapter tests run real git in `tempfile` repos with the hermetic git environment. Set that environment on each `Command`.
- UI tests render with `TestBackend`.
- UI tests drive `EventLoop` with scripted input, a recording screen mode, and a recording shell runner.
- No test calls crossterm or needs a terminal.
- No test reads wall-clock time to classify a session.
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
- `docs/agents/` holds only the reference files that this file names.
- This repository holds no `CLAUDE.md` or `CLAUDE.local.md`: either file would hide this file from Claude Code.
- If one of those files is ever needed, make its first line `@AGENTS.md`.
