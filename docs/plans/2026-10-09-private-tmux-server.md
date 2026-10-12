# Plan: Run every Ringleader session on a private tmux server

> Status: active. On 2026-10-12 the owner accepted the recommendation in every row of section 2. The real-terminal checks of milestone 0 remain.

This plan supersedes the v1 decisions about the user's tmux server, the Ctrl-Q return binding, and the inside-tmux and outside-tmux split. Those are in `docs/plans/v1-bootstrap.md` sections 2, 5, 6, 8, and 16. The v1 plan stays as a historical record. Follow `AGENTS.md` for everything this plan does not change.

## 1. Why

v1 runs agent sessions on the user's own tmux server, the default socket or the one in `$TMUX`. To return to the TUI, it installs a root C-q binding on that server before each attach. It saves the user's binding in `bindings.json` and restores it on exit, at startup after a crash, and on a server restart. That has three costs:

- **Ringleader writes state it does not own.** The binding covers every client and session on the user's server. It outlives a crashed TUI until the next start. Save and restore race with the user's own edits: review finding R3-1 showed that a second install overwrites a binding the user changed mid-run, and exit restores the stale one. No save and restore of shared state can be made fully correct.
- **Two attach flows.** Outside tmux, the TUI suspends and runs an attach child. Inside tmux, it runs `switch-client` on the user's server and keeps polling. Deciding which flow applies takes a `/proc` walk of the caller's parent chain (`CallerProcess`) and a query to the user's server.
- **Size.** The binding subsystem and the inside-tmux path are about 1,900 lines of source and tests, plus a 3,822-line `list-keys` fixture (section 5).

The goal: Ringleader behaves the same whether it runs inside tmux or outside, and it never reads or writes the user's tmux server.

## 2. Decisions

### Locked by the owner

- Every Ringleader session runs on a private tmux server that Ringleader owns: `tmux -S <private socket> -f <generated config>`.
- One attach flow in every context. The TUI suspends, runs `tmux attach-session` as a foreground child with `TMUX` and `TMUX_PANE` removed, and resumes when the child exits. Inside the user's tmux the client is nested. The return key, bound in the generated config, detaches it.
- Ringleader never reads or writes the user's tmux server. No code path runs `bind-key`, `unbind-key`, or `list-keys` against any server other than the private one.

### Accepted by the owner

The owner accepted each recommendation on 2026-10-12. The alternatives stay as a record of what was weighed.

| # | Question | Recommendation | Alternative |
|---|---|---|---|
| D1 | Where does the socket live? | `$XDG_RUNTIME_DIR/ringleader/tmux/tmux.sock`, falling back to `$XDG_STATE_HOME/ringleader/tmux/tmux.sock` when `XDG_RUNTIME_DIR` is unset or relative, as `handoff_directory` falls back. The `tmux` directory is created with mode 0700, as `handoff` is. Refuse, naming the path, when it reaches 100 bytes or contains a comma, since `$TMUX` separates its fields with commas. | `${TMUX_TMPDIR:-/tmp}/ringleader-<uid>/tmux.sock` with an owner and mode check. |
| D2 | Where does the generated config live? | `$XDG_STATE_HOME/ringleader/tmux.conf`, beside the other state files. | Next to the socket in the runtime directory. |
| D3 | What happens to the `tmux_prefix` config key? | Remove it. New sessions use a fixed `ringleader-` constant, so new tmux names match v1 names. Registry entries keep their recorded prefix. An old `tmux_prefix` line is ignored silently, because `RawConfig` has no `deny_unknown_fields`, and the README upgrade note says so. | Keep it configurable. |
| D4 | How does a Stopped entry prove that nothing still runs in its worktree before restart or rename? | Check `/proc` through the existing `HostProbe::processes_in(worktree, excluded_sessions)` for every Stopped entry, whatever its recorded socket, and refuse while any process runs there. Delete already counts these processes in its guards. Remove `has_session_on`, which reads the recorded socket and so would read the user's server. `/proc` cannot see an exited v1 session that survives as a dead pane (`remain-on-exit on`), so results for v1 entries always include the hand `kill-session` command. | Keep `has_session_on` as a read-only `-N` exception for foreign sockets until v1 entries are gone. |
| D5 | What if the TUI or a CLI `attach` runs from a pane of the private server itself, such as a `t` shell or an agent pane? | Use tmux's own test: the caller is in a private pane when its terminal (`ProcessEnvironment::terminal`) equals a `#{pane_tty}` on the private server. The TUI then refuses to start, and CLI `attach` refuses, both with exit 3 and a message to run Ringleader from another terminal. Other CLI commands work there. An editor or terminal started from a `t` shell inherits `$TMUX` but has its own terminal, so it is not refused. | Let CLI `attach` run `switch-client` on the private server instead. |
| D6 | Unregistered sessions on the private server that lack the prefix or valid metadata, made by hand or by an agent's own tmux call. | Show them as `Unmanaged` rows. A session whose name lacks the prefix is labeled `=<tmux name>`. `=` is outside the slug alphabet, so the label never equals a managed name, and it matches the exact target the hand commands use. Ringleader still never kills an `Unmanaged` session. | Keep hiding sessions without the prefix, as v1 does on the shared server. |
| D7 | How does a changed `return_key`, or a release that changes the generated config, reach a server that is already running? | At TUI start, after `r`, and in `new` and `restart` once they hold the write lock, compare the running server's `@ringleader_config` hash with the current render. On a mismatch, unbind the previous key named in `@ringleader_return_key`, then re-source the render through `source-file -` on stdin, since tmux treats a `source-file` path as a glob pattern. Skip this while the config failed to load, because the defaults would replace the user's key. In that state, help names the key the running server reports. | Apply only at the next server start, which happens when the last session ends. Doctor warns about the mismatch. |
| D8 | The `exit-unattached` refusal at creation and its doctor check. | Remove both. The generated config sets `exit-unattached off`, and only a hand edit on the private server could turn it on. | Keep the `#{exit-unattached}` field in the creation done line as a guard. |
| D9 | Should the private server load an optional user file for preferences such as `history-limit`, colors, or copy-mode keys? | Yes, as the last and optional milestone. The generated config names the file by its literal path in single quotes (`source-file -q '<config home>/ringleader/tmux.conf'`) and never writes `$XDG_CONFIG_HOME`, which tmux would expand from the server's environment. Ringleader leaves the line out, and doctor warns, when the path contains `'`, `[`, `*`, `?`, or `\`. Ringleader's essential lines come after it, so they win. It is never the user's main `tmux.conf`. | No user file. |
| D10 | The inside-tmux bell, which rings every client on the user's server through `list-clients`. | Remove it. The TUI is suspended during every attach, so it rings only while the home screen shows, through BEL on its own terminal. | None. Under one flow the TUI cannot poll while attached. |
| D11 | Leftover v1 `bindings.json` after a crashed v1 TUI. | Doctor reports each record's server socket and saved line, plus the hand command to restore it. Ringleader never runs that command and never deletes the file. | A one-time repair at startup, which keeps about 950 lines of binding code for a release. |
| D12 | Minimum tmux, given that `extended-keys-format` arrived in 3.5. | Keep 3.2 and leave `extended-keys-format` out. Agents then get Shift+Enter as `CSI 27;2;13~` on tmux 3.5 and newer, where `xterm` is the default format, and as `CSI 13;2u` on 3.2 to 3.4, which have no format option. Milestone 0 checks that Claude Code and opencode accept both forms. If one needs csi-u, render `set -s extended-keys-format csi-u` only when the server's tmux is 3.5 or newer. | Raise the minimum to 3.5, which leaves out the tmux in Debian 12 (3.3a) and Ubuntu 24.04 (3.4). |
| D13 | `mouse on` also enables tmux's default right-click pane menu, which offers Kill, Respawn, and Split on the agent pane. | Unbind `MouseDown3Pane` and `M-MouseDown3Pane` in the generated config. Keep wheel scrolling and selection. The status-line menus cannot open, because `status` is off. | Keep the menus. |

## 3. Design

### 3.1 The private server

- `XdgPaths` gains `tmux_socket()` (D1) and `tmux_config()` (D2). `DiagnosticPaths` carries both for doctor and help.
- `CompositionRoot::tmux` builds every `TmuxCommand` with `-S <socket>` and `-f <config>`. The `#[cfg(test)]` builders `on_test_server` and `with_config_file` fold into the production constructor. Tests keep passing their own socket and `/dev/null` or a fixture config.
- Every tmux child drops `TMUX` and `TMUX_PANE` from its environment through `ProcessEnvironment::without`. The explicit `-S` already picks the server, and an attach to another server works with `TMUX` set. The reason is the server environment. A server started from a pane of the user's tmux records that `TMUX` and `TMUX_PANE` in its global environment. Panes override `TMUX`, but `run-shell` commands still see the outer pane id in `TMUX_PANE` (section 7, check 7c).
- `TmuxCommand` keeps the caller's `TMUX` and `TMUX_PANE` in a separate caller-identity value, read once by `main`, because the child environment no longer carries them.
- Agent environment names are still removed from every tmux child, and `SessionSpec` still narrows them again at creation. Ringleader is now always the first client of its server, so this matters more than before.
- `-u` stays on every call. `-N` stays on every call except `new-session`, so no query, attach, or doctor call ever starts a server.
- Before the call that can start the server, the multiplexer creates the `tmux` directory with mode 0700 (a reboot empties the runtime directory) and writes the generated config through `DurableFile`. It writes from settings that `SessionSpec` carries, the same way it already carries the agent environment union. Only `create_session` can start the server, and only lock holders call it (create and restart). The TUI's lifetime lock and the CLI's write lock are the same exclusive lock, so only one process ever writes the config.
- `exit-empty` stays at its default. The server exits with its last session, and the next `new-session` starts it again with the current config.
- tmux records a `-S` path as given and does not resolve symlinks, so canonicalize the `tmux` directory to keep `#{socket_path}` equal to the recorded path.

### 3.2 The generated config

The renderer is a new adapter, `TmuxServerConfig` in `src/adapters/tmux_server_config.rs`. Its output is checked against a golden fixture, `tests/fixtures/tmux-server.conf`. Every line below loaded cleanly on 3.6a and behaved as described in a nested setup (section 7). The final content, in this order:

```
set -su terminal-features
source-file -q '<config home>/ringleader/tmux.conf'
set -g key-table root
set -g prefix None
set -g prefix2 None
set -g status off
set -g mouse on
unbind-key -q -T root MouseDown3Pane
unbind-key -q -T root M-MouseDown3Pane
set -s escape-time 10
set -g default-terminal tmux-256color
set -s extended-keys always
set -as terminal-features ',*:extkeys'
set -as terminal-features ',*:RGB'
set -s set-clipboard on
set -s exit-unattached off
set -g destroy-unattached off
set -g detach-on-destroy on
set -g @ringleader_return_key 'C-q'
set -g @ringleader_config '<hash>'
bind-key -T root 'C-q' detach-client
```

- **Milestones.** Milestone 2 renders every line except three. The `source-file -q` line arrives with milestone 7 (D9). The `@ringleader_return_key` and `@ringleader_config` lines arrive with milestone 6 (D7).
- **Order.** The first line resets the one option the file appends to. Re-sourcing the file then never piles up duplicate `terminal-features` entries (D7). The user file comes next. Ringleader's own lines come last, so they win over the user file.
- **Quoting.** The return key is single-quoted wherever it appears. tmux parses `}` and `C-}` as syntax and expands a bare `~` to the home directory. A syntax error anywhere in a `-f` file drops the whole file at server start, and the start still exits 0. The server would then run with prefix C-b, status on, and no return binding. `ReturnKey` rejects quotes and newlines, so single quotes are always safe.
- **No way out without the binding.** With `prefix None`, the return key is the only way to detach. So `create_session` checks, in a labeled query after the creation invocation, that the root key table holds a `detach-client` binding. If it does not, the session is rolled back by id, and the create fails with the config path and the hand command `tmux -S <socket> show-messages`. The check accepts any key bound to `detach-client`, so a server still running an older render passes.
- **Explicit defaults.** `prefix2`, `escape-time`, `default-terminal`, `exit-unattached`, `destroy-unattached`, and `detach-on-destroy` already have these values on 3.6a. They are set anyway, because older releases differ. For example, the 3.2a man page gives 500 ms as the `escape-time` default. With `detach-on-destroy off`, a client whose session is killed would move to another agent's session instead of returning to the TUI. `unbind-key -q` (present in 3.2a) keeps a second sourcing from failing on a key that is already unbound.
- **Extended keys.** `extended-keys always` and `*:extkeys` are both required for Shift+Enter to reach an agent through two layers. Without `*:extkeys`, the nested client never asks the user's tmux for extended keys. Without `always`, agents that only send the kitty keyboard request get plain Enter, because 3.6a ignores that request. `extended-keys-format` stays out (D12).
- **Config errors are silent at server start.** `new-session` and `start-server` return 0 when `-f` holds a line with an unknown option. The other lines still apply, and the error appears over the first attached pane. A `source-file` on a running server returns 1 with the error. So a test on the pinned tmux must show that the golden fixture loads cleanly, both at server start and when it is sourced again on the running server. The D7 reapply reports `source-file` errors.
- **The hash.** `@ringleader_config` is the lowercase hex FNV-1a 64-bit hash of every rendered line except its own. `TmuxServerConfig` computes it from named constants, so the value does not change between Rust releases, as `std::hash::DefaultHasher`'s would. No new crate is needed.
- **Left at tmux defaults.** `history-limit` (2,000 lines) and `focus-events` (off) used to come from the user's config. Milestone 0 decides whether either needs a value here. Otherwise the D9 user file is the place for them.
- **Per-session `destroy-unattached off`** in the single creation invocation stays as it is. It costs nothing and survives a hand `set -g`. On 3.6a the option also accepts `keep-last`, so it is not a boolean there.

### 3.3 One attach flow

- `TmuxMultiplexer::attach` always runs `attach_in_foreground`: a `tmux -S <socket> -f <config> attach-session -t <pane or window id>` child with inherited stdio, waited on with `waitpid(WUNTRACED)`. The `inside_tmux` field, `TmuxScripts::switch_client`, and the switch tests go.
- `EventLoop` drops its inside branches. Enter and a live `t` both restore the terminal, run `App::attach` or `App::open_shell`, then resume and clear. A `t` on a Stopped session still runs the ephemeral `$SHELL` through `ShellRunner`. Consider resolving the target before the screen leaves, since today the reconcile runs after the restore.
- CLI `attach` runs the same child, so the return key brings the user back to their shell with exit 0. The CLI and TUI attach build the same command.
- `SessionAccess` loses `ReturnMode`, `prepare_return`, `AttachOutcome`, and the binding field. `ShellOutcome::Attached` becomes a unit variant. `StatusReport::attached` and `StatusMessage::Warning` go, since nothing else builds them.
- With `prefix None` and no `suspend-client` binding, the stop branch in `attach_in_foreground` becomes defensive. Keep it.

### 3.4 Caller identity and the own-server case

- **The D5 check.** One `-N` query, `list-panes -a -F 'ringleader-tty #{pane_tty}'` on the private server, compared with `ProcessEnvironment::terminal`. No server, or no terminal, means the caller is not in a private pane. This is the test tmux itself applies before it refuses a nested attach: it refuses only when `TMUX` is set and the client's terminal is a pane of the target server, with `sessions should be nested with care, unset $TMUX to force`. Ringleader removes `TMUX` from every child, so tmux's check never fires, and Ringleader's must replace it. `attach-session` also moves every client of the target session. A nested attach from a private pane would move the user's real client off the TUI, and the return key would then detach that real client.
- **`caller_pane`.** It returns `$TMUX_PANE` only when the socket field of `$TMUX` (the text before the first comma) names the private socket, compared canonically. Pane ids are per server, so a `%3` from the user's server can name an unrelated `%3` on the private server.
- **`caller_session`.** Delete uses it to refuse the session the caller runs in, and it goes through `caller_pane`. An agent's subprocess has no terminal, so the `$TMUX` match, not the tty test, is the right check here. An agent can still run `ringleader delete` on its own session, so the guard stays.
- **Launch directory.** Delete also refuses when the launch directory that `main` captures lies inside the worktree. This covers a shell in a v1 session's `t` window on the user's server, which `caller_session` cannot see and whose processes `processes_in` skips as the caller's own.
- **Removed.** `is_inside_client`, `pane_shows_caller`, `runs_inside_tmux`, and `CallerProcess` go.

### 3.5 Naming, prefixes, and reconciliation

- `NamingScheme` keeps its `tmux_prefix` field. New sessions fill it from one constant, used by `Config::defaults` too. Registry entries keep their recorded prefix, and rename keeps rebuilding the tmux name from it.
- The `tmux_prefix` config key, its load check, its `RawConfig` field, and its template line go (D3). `check_prefix` stays for `branch_prefix`.
- `place_unregistered` no longer returns when a session lacks the prefix. It labels the session `=<tmux name>` (D6). A stripped name that is not a valid slug still becomes `Unmanaged` through `adoption`.
- Labeled output with `ringleader-done`, rollback by id, the tag move, hook warnings, exact targets, and metadata validation all stay. The user, and every agent pane whose `$TMUX` now names the private server, can still run tmux commands against it.
- The `OtherSocket` pairing mismatch stays. It becomes rare: a hand-made namesake, or a private socket path that changed between runs.
- The generated config binds the return key with no condition, so everything that says Ctrl-Q depends on the prefix goes in milestone 2: `StatusNote`'s prefix branch in `status_bar.rs`, the `tmux_prefix` parameter threaded through `App` and `HomeState`, the help paragraph and `HelpView::new`'s prefix argument (which is `Dialog`'s only use of the prefix), and the Ctrl-Q clause of doctor's naming-drift warning.

### 3.6 Migration from v1

- **v1 entries.** An entry whose recorded socket is not the current private socket is a v1 entry. That also covers a private socket that moved because `XDG_RUNTIME_DIR` changed. The registry shape and schema version stay at 1, so a new binary reads v1 files as they are.
  - It lists as `Stopped`, because the TUI observes only the private server. Selecting it shows a status note naming its socket and the hand commands.
  - Attach and a live `t` refuse with: `<name> runs on the tmux server at <socket>, which Ringleader no longer uses. Attach there with tmux -S <socket> attach -t '=<tmux name>:', or end it with kill-session and press s to restart it here.` The ephemeral `t` shell already works.
  - Restart and rename refuse while `/proc` shows processes in the worktree (D4). Once no process runs there, restart recreates the session on the private server in the same worktree and records the new socket, with no new code. Delete removes it normally. Because `/proc` cannot see an exited v1 session kept as a dead pane, the restart, rename, and delete results for a v1 entry name its old socket and the `kill-session` command.
  - Ringleader never kills, binds, or queries anything on the old server.
- **Unregistered v1 sessions** on the user's server are invisible to Ringleader. The README upgrade note gives the hand command to find them.
- **`bindings.json`** from a crashed v1 TUI stays where it is. Doctor reports it (D11) through a read-only `LeftoverBindings` port, whose adapter keeps the read half of today's `BindingRecords`. The binding it describes is v1's conditional `if-shell` binding on the user's server, which only acts in sessions named with the old prefix.
- **`tmux_prefix`** lines in existing config files keep loading and do nothing (D3).

### 3.7 Doctor

`doctor.rs` is at 194 lines before its tests. The private-server rows move into a new `ServerAudit` in `src/application/server_audit.rs`.

| Check | Change |
|---|---|
| tmux client version, minimum 3.2 | Keep. |
| tmux server version | Point at the private server only. A client and server mismatch means the private server predates a tmux upgrade: warn and advise ending that server once its sessions are idle. |
| `exit-unattached` | Remove (D8). |
| Naming drift | Drop the Ctrl-Q clause (milestone 2). Compare `worktree_root` and `branch_prefix`, and `tmux_prefix` against the constant. |
| Stopped entry live on its recorded socket | Replace with the `/proc` check for every Stopped entry (D4). An entry on the private socket that is Stopped while `/proc` shows processes in its worktree means the socket was removed under a running server. Warn, with the hand fix: recreate the `tmux` directory with mode 0700, then send SIGUSR1 to the old server, which makes tmux recreate its socket. |
| New: private server location | Info line with the socket path, the config path, and the hand command to inspect the server, `tmux -S <socket> ls`. Info when the state-directory fallback applies. Problem when the socket path reaches 100 bytes or contains a comma, or when the `tmux` directory is not mode 0700 or not owned by the user. This needs a new `HostProbe` method that returns a directory's owner and mode. |
| New: private server state | Query with `-N`. No server is info. A live server is info with its version and session count. Any other failure, such as `Permission denied`, is a problem. Doctor never starts a server. |
| New: config drift | Warning when the running server's `@ringleader_config` differs from the current render (D7). A new `Multiplexer` method reports whether the running server matches. The adapter compares against its own render, so the application never sees the hash. |
| New: return key | Warning when `return_key` is a key agents need, such as `C-c`, `C-d`, `Enter`, or `Escape`. When `$TMUX` is set, an info line that the user's own tmux must not bind that key in its root table. Doctor cannot check that without reading the user's server. |
| New: v1 entries | A warning per entry on another socket, with the hand commands and whether `/proc` shows processes in its worktree. It says that `/proc` cannot tell an exited v1 session from one that has ended. No tmux call. |
| New: leftover `bindings.json` | A warning with each record's socket and saved line, and the hand command. No tmux call (D11). |

### 3.8 Config, help, README, and AGENTS.md

- **Config.** Add `return_key`, default `C-q`, through `RawConfig`, `ConfigDraft`, `ConfigChecks`, `Config`, and the first-run template. It is a new domain type, `ReturnKey` in `src/domain/return_key.rs`. It accepts optional `C-`, `M-`, and `S-` modifiers followed by one printable non-space ASCII character or a named key such as `F1` to `F12`, `Escape`, or `Home`. It rejects empty values, whitespace, quotes, `\`, and newlines, and accepts only keys a terminal can send (section 7): after `C-` a letter other than `i` or `m`, stored in lowercase, or `]`, `^`, or `_`; no `S-` before a character; and no `C-` or `S-` before Enter, Tab, BTab, Escape, BSpace, or Space, except `C-Space`. It gives the single-quoted tmux spelling for the config and a display form for help and the README.
- **Help.** `HelpView::new` takes the return key, the socket path, and the config path instead of `tmux_prefix`. The return section says the return key detaches back to the TUI from any session. It also says sessions have no prefix key and the mouse is on, and that inside your own tmux the session opens nested, so your tmux must not bind the return key. The prefix sentence and the `env -u TMUX -u TMUX_PANE ringleader` sentence go. The bell sentence becomes "the bell rings only while this screen shows". The Files section adds the socket and the config path.
- **README.** Replace the Ctrl-Q paragraph with the return key paragraph above. Add a short upgrade note: sessions from earlier versions stay on your own tmux server, `ringleader doctor` lists them, end each there and run `ringleader restart <name>`, and `tmux_prefix` no longer does anything. Add the hand command to inspect the private server.
- **AGENTS.md.** In the same commit as the behavior each rule describes:
  - The ad-hoc tmux rule also never touches the owner's real private server.
  - The manual-run rule no longer says production `TmuxCommand` passes no socket or config. The throwaway `XDG_RUNTIME_DIR` and `XDG_STATE_HOME` now keep a run off the owner's real server and state. `TMUX_TMPDIR`, the `tmux-<uid>` directory, and the holder server step go. Typed commands pass `-S <tempdir>/run/ringleader/tmux/tmux.sock`. The cleanup step names that socket.
  - The outside and inside checks become three: a plain pane of an ad-hoc outer server with `TMUX` unset, the same pane with `TMUX` set (nested), and a `t` shell on the private server (D5 refusal). Each ends by asserting that the outer server's `list-keys` output and session list are unchanged.
  - Wiring: replace the whole bullet "Only the TUI installs the Ctrl-Q return binding. The CLI never touches tmux key bindings." with "Key bindings come only from the generated config of the private server, which `create_session` writes and the TUI and the mutating commands reapply (D7). Every tmux call passes `-S <private socket>`, so Ringleader never reads or writes the user's tmux server. The TUI and the CLI attach through the same foreground child." Add that only a lock holder writes the generated config.
  - Remove the sentence about the binding guard and exit notice.
  - The tmux test rule adds that tests of the generated config start `TmuxTestServer` with the rendered file, and that tests press keys through a client, never with `send-keys -t <pane>`.
  - The tree loses `ports/return_binding.rs`, `caller_process.rs`, `tmux_return_binding.rs`, `binding_records.rs`, `key_binding.rs`, and `fake_return_binding.rs`. It gains `domain/return_key.rs`, `adapters/tmux_server_config.rs`, `application/ports/leftover_bindings.rs`, `adapters/leftover_bindings_file.rs`, `test_support/fake_leftover_bindings.rs`, and `application/server_audit.rs`. It updates the comments of `session_access.rs`, `session_monitor.rs` (no bell), `diagnostic_paths.rs`, `xdg_paths.rs`, `tmux_command.rs`, `tmux_listing.rs`, and `tmux_test_server.rs`.

## 4. Milestones

One commit per milestone, after every command under "Commands" in `AGENTS.md` passes. The crate is a binary with no library target, so every new item needs a production caller in the commit that adds it, and every removal takes its last caller with it. Each commit also updates the help text, README, and `AGENTS.md` passages it makes false.

0. **Real-terminal checks.** Run the list at the end of section 7 and record the results there. Confirm D12 and D13 on real terminals, and decide whether `history-limit` and `focus-events` need values in section 3.2. No code.
1. **Remove the inside bell** (D10): `ring_bell`, the `list-clients` parser and fixture, the port method, the fake support, and the inside branch of `Bell`. It is independent of the rest and goes away under any outcome.
2. **Switch to the private server.** This is the largest commit, and its parts cannot land apart. A private server combined with the inside-tmux `switch-client` path breaks the inside case. Once attach drops `prepare_return`, the return binding has no caller, and its startup repair would keep acting on the user's server. Every new type below has its first production caller here. It contains:
   - `ReturnKey`, the `return_key` key and template line, `TmuxServerConfig` with its golden fixture (without the D7 and D9 lines), `XdgPaths::tmux_socket` and `tmux_config`, and `DiagnosticPaths`, shown by help's Files section.
   - Sections 3.1, 3.3, and 3.4: the production `TmuxCommand` with `-S` and `-f`, the stripped child environment, caller identity, the D5 refusal, the launch-directory delete refusal, the post-create binding check, and one attach path. `is_inside_client`, `CallerProcess`, the inside flag, and `switch_client` are removed.
   - The return binding's removal: the port, the adapter, `KeyBinding`, the fake, `BindingGuard`, `ExitNotice`, the startup repair, the `list-keys` fixture, `TmuxCommand::change_with_input`, and `Call::with_input`.
   - The `LeftoverBindings` port, adapter, and fake, with doctor's leftover `bindings.json` row (D11), so the read half of `BindingRecords` keeps a caller.
   - The prefix texts listed at the end of section 3.5.
3. **Migration and liveness.** Section 3.6 and D4: v1 entry classification, the attach refusal and status note, the hand commands in results, and the `/proc` check for every Stopped entry in both `SessionLifecycle` and `RegistryAudit`. This brings the doctor row "Stopped entry live on its recorded socket" forward from section 3.7. It removes `has_session_on`, `FakeMultiplexer::with_session_on_socket`, and `TmuxCommand::on_socket`. Until this lands, `has_session_on` still reads a v1 entry's recorded socket, so push milestones 2 and 3 together.
4. **Prefix.** Remove the `tmux_prefix` key (D3) and stop filtering by prefix, with the `=<tmux name>` label (D6).
5. **Doctor.** The rest of section 3.7: `ServerAudit`, the `HostProbe` owner-and-mode method, the location, state, return key, and v1 rows, and the removal of the `exit-unattached` check and creation refusal (D8).
6. **Config drift** (D7): the hash and the two user options in the render, the `Multiplexer` method, the reapply at TUI start, after `r`, and in `new` and `restart`, the read-only skip, and doctor's drift row. Stdin input comes back to `TmuxCommand` and `Call` with the reapply, its first caller.
7. **Optional user file** (D9), if the owner wants it.

When every milestone is in, flip the status header to `completed`.

### Implementation status

As of 2026-10-11, milestones 1 to 7 are in, one commit each, and they implement the accepted recommendation in each D row. Milestone 0 is partly done: the tmux 3.2 check and the capture of the agents' keyboard requests ran, and section 7 records the results and the two fixes they led to. The rest needs the owner's real terminals and logged-in agents. Until it runs, the D12 and D13 checks and the defaults for `history-limit` and `focus-events` stay unconfirmed. If an agent turns out to need csi-u, D12 already says what to render.

Where the implementation departs from the text above:

- The v1 doctor row from section 3.7 landed with milestone 3 instead of milestone 5, so doctor names the earlier server from the same commit that removes `has_session_on`, as the README upgrade note says.
- The leftover `bindings.json` row moved into `ServerAudit` with milestone 5, which keeps `doctor.rs` under 200 lines.
- `TmuxTestServer` starts every test server with the rendered config, not only the tests of the generated config, because the post-create check in `create_session` needs a `detach-client` binding.
- The `kill-session` notes in the restart, rename, and delete results come from `SessionService::earlier_server`, which the CLI and the TUI query before the command, instead of from new outcome types.
- A changed user file from D9 does not change the hash, so it reaches a running server only when the server next starts. The README says so.
- A reapply whose `source-file` exits 1 still counts as applied when the server then holds the current hash, because tmux runs every line of the render after a failing line in the user file. tmux's error goes to the log. A reapply that leaves the old hash still fails.
- Rename records the entry on the private socket, as restart does. Without that, an entry from an earlier server kept its old socket under the new tmux name, and every hand command named a session that does not exist there.
- The naming drift row warns only when `worktree_root` or `branch_prefix` differ from the config. An entry recorded under an earlier `tmux_prefix` gets an info line, since no setting can clear it.
- The D5 own-pane check skips dead panes. tmux keeps reporting an exited pane's terminal after freeing it, and Linux hands that `/dev/pts` number to the next terminal opened.
- A reapply writes the render to the generated config file before sourcing it, so the file help and doctor name matches the running server.
- The private server's config handling lives in `PrivateServer` rather than `TmuxMultiplexer`, which keeps the multiplexer below its size on `master`.
- The hand commands for an entry on an earlier server give `tmux -S <socket> switch-client` for use inside that tmux next to the `attach` that section 3.6 names, because tmux refuses that `attach` from a pane of the same server.
- A live `t` repeats the D5 own-pane check that attach makes, since the TUI only warns when the startup check fails for a reason other than a conflict.
- Help, doctor, the README, and the template say that the user's tmux must not use the return key as its prefix either, since an outer prefix swallows it too.
- The generated config sets `key-table root` after the user file, since a session key table set there would hide the root return binding while the post-create check still passed.
- The launch-directory delete guard applies to a full delete and its measurement, not to `delete --forget`, which removes nothing on disk.
- The status bar wraps a long message or note over up to a quarter of the terminal height, so the hand commands fit. The earlier-server note leads with the `kill-session` command.
- D4 is narrower for an entry on the private socket. Restart and rename refuse only while a process in the worktree descends from a tmux server whose command line names the private socket, through `HostProbe::processes_under_server`. Counting every process blocked them whenever an editor or a shell sat in the worktree. An entry on an earlier server still counts every process, since `/proc` is the only evidence there.
- Doctor's lost socket warning from section 3.7 needs both a missing socket file and such a process, so an open editor no longer triggers it.
- The generated config binds the return key in `copy-mode` and `copy-mode-vi` as well as `root`, and a reapply unbinds the previous key in all three. In copy mode tmux looks a key up in the mode's table first, and those tables already bind keys `ReturnKey` accepts, such as `M-x`.
- Doctor compares only the access bits of the socket directory, since Linux copies a parent's setgid bit to new subdirectories and `chmod 700` keeps it.

## 5. What goes away

| Item | Lines |
|---|---|
| `src/adapters/tmux_return_binding.rs` | 604 |
| `src/adapters/key_binding.rs` | 225 |
| `src/adapters/binding_records.rs` (its read half moves to `LeftoverBindingsFile`) | 103 |
| `src/adapters/caller_process.rs` | 78 |
| `src/test_support/fake_return_binding.rs` | 64 |
| `src/application/ports/return_binding.rs` | 29 |
| `tests/fixtures/tmux-bindings.txt` | 3,822 |
| `tests/fixtures/tmux-list-clients.txt` | 5 |
| Inside-tmux detection, its tests, and the `switch-client` path in `main.rs`, `tmux_command.rs`, `tmux_multiplexer.rs`, `tmux_scripts.rs`, `event_loop.rs`, `app.rs`, `service_context.rs`, `session_service.rs`, and `service_fixture.rs` | about 300 |
| `BindingGuard`, `ExitNotice`, `NoticeHandle`, and their tests in `app.rs` | about 170 |
| `ReturnMode`, `AttachOutcome`, `prepare_return`, and their tests in `session_access.rs` | about 80 |
| The inside bell (`ring_bell`, the `list-clients` parser, the port method, the fake support) | about 90 |
| `exit-unattached` refusal and doctor check | about 50 |
| `tmux_prefix` key, load check, status note, and their tests | about 60 |
| `has_session_on`, `on_socket`, and their tests | about 80 |

That is 1,103 lines in deleted source files and about 830 more from files that stay. Against that come perhaps 500 to 600 new lines for `ReturnKey`, `TmuxServerConfig`, caller identity, `LeftoverBindings`, migration, `ServerAudit`, and config drift.

## 6. Tests the milestones must add

- **Config and paths (milestone 2).** `return_key` defaults to `C-q`. `C-q`, `M-x`, `F12`, `~`, `}`, and `M-}` pass. Empty, `C-`, a quote, `\`, a space, a newline, and `C-}` each fail with `return_key` named. The template loads. The socket lands under `XDG_RUNTIME_DIR`, falls back to the state directory when the variable is unset or relative, and a path of 100 bytes or more, or with a comma, is refused with the path named.
- **Generated config (milestone 2).**
  - The render matches the golden fixture.
  - A `TmuxTestServer` started with it reports `prefix` None and `status` off, and sourcing the file again on that running server returns 0.
  - Each key class (`C-q`, `M-x`, `F12`, `~`, `}`, `M-}`) loads through `-f` at server start and through `source-file -` on a running server, and `list-keys -T root` shows it bound to `detach-client`.
  - Every printable character that `ReturnKey` accepts, alone and after `C-` and `M-`, binds to `detach-client` through `source-file -` on one running server. A character that fails is added to `ReturnKey`'s reject list.
  - Keys are pressed through a client with `send-keys -K -c <client>`, never with `send-keys -t <pane>`, which skips the key tables (section 7, check 3b).
  - The return key on a control-mode client exits the client from an agent window, from a `t` shell window, and from a session without metadata.
  - `C-b` reaches the pane.
  - A config path containing `[1]` loads through `-f`.
  - A fixture `HOME` with a `.tmux.conf` that sets a marker option leaves no marker.
  - A create on a server whose root table holds no `detach-client` binding rolls back and fails with the config path.
- **Isolation (milestone 2).**
  - Every production `TmuxCommand` argv carries `-S <private socket>` and `-f <config>`.
  - The attach child's environment lacks `TMUX` and `TMUX_PANE` when the snapshot has them.
  - A second `TmuxTestServer` stands in for the user's server and is named in the fixture snapshot's `TMUX` and `TMUX_PANE`. Create, observe, preview, shell open, attach preparation, and doctor leave that server's `list-keys` output and session list byte for byte unchanged.
- **Caller identity (milestone 2).**
  - `caller_session` returns None when `$TMUX` names another server, even when that `TMUX_PANE` id exists on the private server.
  - A terminal that is a private pane refuses the TUI start and CLI `attach` with exit 3, even with `TMUX` unset.
  - An inherited `$TMUX` with a terminal that is not a pane starts the TUI.
  - Delete refuses when the launch directory is inside the worktree.
- **One flow (milestone 2).** Attach and live-shell open suspend and resume the screen whether or not the fixture snapshot has `TMUX`. CLI and TUI attach build the same command.
- **Leftover bindings (milestone 2).** A `bindings.json` fixture gives a doctor warning with each socket and saved line, and no tmux call.
- **Migration (milestone 3).**
  - `sessions.json` with v1 sockets loads unchanged.
  - A v1 entry refuses attach with the hand commands.
  - Restart of a v1 entry with no processes in its worktree creates it on the private socket, records that socket, and names the old socket and the `kill-session` command in its result.
  - Restart and rename refuse while `/proc` shows processes in the worktree, for a v1 entry and for an entry on the private socket.
  - No `FakeMultiplexer` call targets a v1 socket.
- **Prefix (milestone 4).**
  - With `tmux_prefix = "agents-"` still in the config, a new session is named `ringleader-<name>`.
  - An unregistered session named `work` lists as `Unmanaged` with the label `=work`, and one named `ringleader-Bad_Name` lists as `Unmanaged`. Delete refuses both.
  - An unregistered `fix-auth` next to a managed `fix-auth` lists both, and `attach fix-auth` targets the managed one.
- **Doctor (milestone 5).** One test per new or changed finding in section 3.7.
- **Config drift (milestone 6).**
  - On a `TmuxTestServer` started with a render that binds `C-q`, a TUI start with `return_key = "M-x"` leaves `C-q` unbound, binds `M-x`, and updates `@ringleader_config`.
  - A matching hash sends no tmux change.
  - A render with an invalid line puts the `source-file` error in the status bar.
  - A config that failed to load skips the reapply.
  - `new` reapplies before `create_session`.
- **User file (milestone 7).**
  - A user file that sets `history-limit 5000` and `status on` gives a server with `history-limit` 5000 and `status` off.
  - A missing user file loads cleanly.
  - A config home with `[` leaves the line out and draws the doctor warning.
- **Help (milestone 2).** Help names the configured return key, the socket path, and the config path, and no longer mentions `tmux_prefix` or `env -u TMUX`.

## 7. tmux checks

These checks ran on the pinned tmux 3.6a on 2026-10-09, using throwaway servers only. An OUTER server started with `-f /dev/null` stood in for the user's tmux. An INNER server started with the candidate config stood in for the private server. A scripted pseudo-terminal played the real OUTER client and typed raw bytes, because `send-keys -t <pane>` skips the client's key tables. Every check below passed.

| # | Check | Result |
|---|---|---|
| 1 | Config lines | Every line in section 3.2 loads with exit 0. An unknown option looks like `invalid option: no-such-option` and stays hidden until the first attach. After `prefix None` the prefix table still holds its bindings, but no key reaches it, so it needs no unbinding. |
| 2 | Nested attach | `env -u TMUX tmux -S <inner> attach -t agent` from an OUTER pane attaches as `tmux-256color` with UTF-8. The window gets every row, and no INNER status line shows. The OUTER status line takes one row from the inner client. |
| 2b | Nesting refusal | An attach from an OUTER pane to INNER with `TMUX` still set works. Only an attach from an INNER pane to INNER is refused, with `sessions should be nested with care, unset $TMUX to force` and exit 1. |
| 3 | Return key | Typed C-q (byte 0x11) detaches the inner client. The OUTER pane prints `[detached (from session agent)]`, and the attach exits 0. |
| 3b | Outer binding | With `bind -n C-q` in OUTER's root table, typed C-q and `send-keys -K -c <client> C-q` are both caught by OUTER, and the inner client stays attached. `send-keys -t <pane> C-q` skips OUTER's table and detaches, so tests must press keys through a client. |
| 4 | Prefix | Typed C-b c makes a window on OUTER, and the agent pane gets nothing. C-b C-b (OUTER's send-prefix) delivers `^B` to the agent. INNER never acts on C-b. |
| 5 | Shift+Enter | The agent gets a distinct Shift+Enter only when OUTER `extended-keys` is `on` or `always`, and INNER has `always` (or `on` plus an app request) together with `terminal-features ',*:extkeys'`. With INNER `extended-keys-format csi-u` it arrives as `CSI 13;2u`. With the 3.6a default, `xterm`, it arrives as `CSI 27;2;13~`. With OUTER `off`, the tmux default, every combination gives plain `^M`. 3.6a ignores kitty keyboard requests. Setting `extended-keys` on a running server does not change panes that already exist. |
| 6 | OSC 52 | A copy reaches OUTER's buffers and terminal only when INNER and OUTER both have `set-clipboard on`. OUTER's default, `external`, drops it. v1 has the same condition, since agents already run on the user's server. |
| 7a | TERM | The nested client's terminal is OUTER's `default-terminal`. Agent panes see `TERM=tmux-256color`, `COLORTERM=truecolor`, and 256 colors. Nested clients already have RGB and clipboard, so `*:RGB` only matters outside tmux. |
| 7b | `-u` | Under `LC_ALL=C` the attach child without `-u` renders `héllo ✓` as `h_llo _`. With `-u` it renders correctly. Keep `-u`. |
| 7c | Server environment | A server started from an OUTER pane records the OUTER `TMUX` and `TMUX_PANE` in its global environment. `run-shell` then sees the OUTER pane id. Strip both from every child. |
| 7d | Destroyed session | With `detach-on-destroy on` (the default), killing the attached session ends the client with `[exited]` and exit 0. With `off`, the client moves to another session. An exit code of 0 cannot tell a detach from a destroyed session. With `exit-empty on`, the server exits after its last session. |
| 7e | Mouse and paste | The wheel puts the INNER agent pane into copy mode, whatever OUTER's `mouse` setting. Bracketed paste arrives intact. `mouse on` brings the default right-click pane menu (D13). |
| 8 | End to end | A fresh server with the config plus `extended-keys-format csi-u`: Shift+Enter is distinct by both routes, an OSC 52 copy reaches OUTER, and C-q detaches with exit 0. |

A plan review also ran these checks on throwaway servers:

- `bind-key -T root C-} detach-client` and `bind-key -T root } detach-client` are syntax errors, and `bind-key -T root ~ detach-client` fails with `unknown key: /home/<user>`.
- A `-f` file with a syntax error is dropped whole at server start, and the start exits 0.
- Single-quoted `'C-}'` and `'~'` bind correctly.
- `$VAR` in a config line expands from the server's global environment.
- `source-file -q` on a path containing `[1]` loads nothing and exits 0.
- tmux 3.2a sends Shift+Enter as `CSI 13;2u`.

Milestone 0 runs these in real terminals, because the pseudo-terminal only proves the tmux side:

- **Shift+Enter outside tmux.** In each terminal the owner uses, check that the terminal honors `ESC[>4;2m`, that Shift+Enter makes a newline in Claude Code and opencode, and that the terminal does not catch Ctrl+Q itself.
- **Shift+Enter inside a real user tmux.** Try `extended-keys` off, on, and always. Shift+Enter is expected to work with `on` and `always` and to be lost with `off`. Decide what help and doctor tell users who have `off`.
- **Agent keyboard requests.** Find which request Claude Code and opencode send: kitty `CSI >1u` or modifyOtherKeys `CSI >4;1m`. Check that they accept both Shift+Enter forms from D12, and that `always` does not break readline in the `t` shell (Shift+Enter, Ctrl+Enter, Alt keys).
- **OSC 52.** Check that a copy reaches the OS clipboard outside tmux, and nested with the user's `set-clipboard` set to `on` and to `external`.
- **Truecolor** outside tmux with `*:RGB`, including on a terminal without truecolor.
- **Mouse.** Click and drag selection, copy, wheel scroll, and the D13 unbinding.
- **Latency and focus.** Escape and Alt latency through two layers with `escape-time 10` on each, and focus events when the user's `focus-events` is off.
- **Resize.** Resizing the real terminal during a nested attach.
- **Redraw.** How the TUI redraws after the attach child prints `[detached ...]` or `[exited]`.
- **tmux 3.2.** The whole config on tmux 3.2, the minimum supported version.

### Milestone 0 results

The implementing agent ran two of the items above on 2026-10-11, on throwaway servers only.

**tmux 3.2a**, taken from nixos-21.11:

- The generated config loads with no errors at server start, and sourcing it again on the running server returns 0. The D7 reapply through `source-file -` works.
- The full test suite passes on 3.2a except the four tests that press keys with `send-keys -K`, which tmux added in 3.3. A run of the real binary covered the same ground instead. In a plain pane and nested inside an outer 3.2a server, Ctrl-Q detaches and the attach exits 0, the TUI attaches and returns, a `t` shell refuses with exit 3, and the outer server's `list-keys` output and session list stay unchanged.
- tmux 3.2a cannot bind `C-$`, `C-%`, `C-&`, `C-*`, `C-{`, `C-|`, `C-}`, or `C-~`, and reports `unknown key`. `ReturnKey` now refuses them. Every named key `ReturnKey` accepts binds, and so does every other accepted character, alone or after `C-` or `M-`.
- tmux 3.2a treats `C-A` and `C-a` as one key, where 3.6a keeps them apart. Both bind.
- `#{server_sessions}` does not exist on 3.2a, so doctor now counts the sessions with `list-sessions`.

**Agent keyboard requests.** Each agent ran under `script`, which logs every byte the agent writes, in a pane of a 3.6a server started with the generated config. The environment was clean and the home a throwaway, so Claude Code stopped at its first-run screen.

- Claude Code 2.1.296 sends `CSI <u`, `CSI >5u` (kitty flags 1 and 4), and `CSI >4;2m` (modifyOtherKeys 2) at startup, without waiting for answers. It then queries `CSI ?u`, XTVERSION, and DA1, and turns on bracketed paste and focus events.
- opencode 1.19.0 queries `CSI ?u` and XTVERSION, gets no kitty answer from tmux, and sends `CSI >4;1m` (modifyOtherKeys 1). It never pushes kitty flags. It also turns on bracketed paste and SGR mouse tracking (1000, 1002, 1003, 1006), and wraps some requests in `DCS tmux;` passthrough, which tmux drops.
- Neither agent depends on the kitty protocol inside tmux. Both ask for modifyOtherKeys, which tmux honors, so D12's choice to leave `extended-keys-format` out stands unless an agent rejects one of the Shift+Enter forms.

**Which return keys a terminal reaches.** A real tmux client on a pseudo-terminal was fed the raw bytes a terminal sends, on 3.6a and 3.2a:

- `C-q`, `C-]`, `C-^`, `C-_`, and `C-Space` detach on both. `C-Q` and `C-A` never fire on 3.6a, because Ctrl-Q and Ctrl-A send the bytes for `C-q` and `C-a`. `C-@` and `C-[` fire only on 3.2a.
- `S-x` never fires, because Shift+x types `X`. `C-1`, `C-2`, `C-/`, `C-?`, `C-Enter`, `C-Tab`, and `C-Escape` never fire.
- `C-i` and `C-m` fire on 3.2a but never on 3.6a, which reads their bytes as Tab and Enter. `C-h` and `C-j` fire on both.
- `M-x`, `M-X`, `C-M-x`, `C-Up`, `M-Up`, `C-F1`, `S-F1`, and `BTab` detach on both.

`ReturnKey` now accepts only keys in the reachable groups, and a test types the bytes into a client nested in a second test server.

Still open: whether each agent turns `CSI 27;2;13~` and `CSI 13;2u` into a newline, which needs a logged-in prompt, and every other item in the list above. Because opencode turns on mouse tracking, tmux's default wheel binding hands the wheel to opencode instead of entering copy mode. The mouse check should confirm what right-click does in opencode with `MouseDown3Pane` unbound (D13).

## 8. Accepted limits

- **Nesting inside the user's tmux.**
  - A root binding of the return key in the user's own tmux consumes it before the inner client sees it. Help and doctor say so, and `return_key` lets the user pick another key.
  - The user's outer `extended-keys` and `set-clipboard` settings still decide what reaches agents. They already do in v1. With tmux's defaults (`extended-keys off`, `set-clipboard external`), Shift+Enter arrives as plain Enter and OSC 52 copies are dropped.
- **No polling while attached.** The TUI is suspended during every attach, in both contexts. The bell rings only while the home screen shows (D10).
- **Agent sessions leave the user's tmux.** They no longer appear in the user's `tmux ls`, `choose-tree`, or status bar. The README and doctor give the `tmux -S` command to inspect them.
- **Exited v1 sessions.** A v1 session whose agent exited survives as a dead pane, which `/proc` cannot see. Restart or delete of its entry can therefore leave it on the user's server. The results and doctor name its socket and the `kill-session` command.
- **A removed runtime directory.** If logind removes `/run/user/<uid>` while the private server still runs (no linger and `KillUserProcesses` off), the socket file disappears and every entry lists as Stopped. Restart and rename then refuse while `/proc` shows processes in the worktree. Doctor warns and gives the hand fix (section 3.7). A create of a new session still starts a second server.
- **Shared runtime directory.** Two Ringleader setups with different `XDG_STATE_HOME` but the same `XDG_RUNTIME_DIR` share one server, and each would adopt the other's sessions from their metadata. Accepted, because the manual-run setup in `AGENTS.md` sets both variables.
- **Hand edits.** The user and any agent pane can change options on the private server. The defenses listed in section 3.5 stay for that reason.

## 9. Out of scope

- Pinning tmux through the flake, so the wrapper always uses the tmux the tests ran against. It is a separate, small change worth doing once this lands.
- A session-scoped `key-table`. It was considered and rejected, because a session's default key table replaces root for that session, so the user's own `bind -n` keys would stop working inside agent sessions.
- A user-owned binding in the user's `tmux.conf`, with `prefix d` as the fallback. It was rejected because the inside-tmux return needs a helper and still runs on the user's server.
