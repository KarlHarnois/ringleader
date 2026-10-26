---
name: managing-ringleader-sessions
description: Hand a task to a new Ringleader session, send a prompt to another session, or list, rename, restart, stop, or delete sessions, through the ringleader CLI. Use this skill from inside a Ringleader session whenever the user asks to start a session for a task, to pass work or a prompt to another session, to spin up a worktree, or to check on the other sessions.
---

# Managing Ringleader sessions

**Run every Ringleader action through the `ringleader` CLI.** This agent runs inside a Ringleader session, so the CLI reaches the sessions the user sees in Ringleader.

## Terms

- The **current session** is the Ringleader session this agent runs in.
- The **main checkout** is the repo's own directory, which git lists first in `git worktree list`.
- A **handoff prompt** is the prompt this skill writes for another session's agent.

## Find the current session

**Run these steps before any action that needs the current session's agent, repo, or name.**

1. Run `git rev-parse --show-toplevel`. Its output is the current worktree, with symlinks resolved.
2. Run `ringleader list --json`. The current session is the entry whose `worktree`, passed through `realpath`, equals the current worktree. Compare the resolved paths, since a `worktree` value can still contain a symlink.
3. Run `git worktree list --porcelain`. The path after the first `worktree` is the main checkout.

If no entry matches, this agent runs outside Ringleader. Ask the user which agent and repo to use, and wait for the answer.

## Start a new session

**Unless the user names another value, start a new session with these defaults:**

| Setting | Flag | Default |
| --- | --- | --- |
| Agent | `--agent <agent>` | The `agent` of the current session |
| Repo | `--repo <path>` | The main checkout |
| Checkout | none | A new worktree |
| Base | none | The repo's base branch |
| Name | `--name <name>` | A name this skill picks, per step 1 |
| Mode | `--plan` | Plan mode, per step 2 |

- If the user asks for the main checkout, pass `--main-checkout`. Otherwise, never pass it.
- If the user asks to build on an existing branch, pass `--base-branch <branch>`. Otherwise, never pass it.
- To build on the current work, the branch is the current session's `branch`.

1. Pick the name: two to four lowercase words that summarize the task, joined by hyphens, such as `fix-login-timeout`. The name is also the new branch. If `ringleader list --json` already shows the name, pick another.
2. Pick the mode:
   - If the user asks to implement, fix, or change something concrete, leave out `--plan`.
   - Otherwise, pass `--plan`. Plan mode covers these requests:
     - investigating
     - designing
     - planning
     - reviewing
     - every request whose intent is unclear
3. Write the handoff prompt, per Write a handoff prompt.
4. Run this command. In plan mode, `<mode>` is `--plan`. In build mode, leave `<mode>` out.
   - If the shell tool takes a timeout, give the command at least 180 seconds.
   - The command can wait for another Ringleader to finish a change, then for the new agent to start.

   ```sh
   ringleader new --name <name> --repo <main checkout> --agent <agent> <mode> --prompt "$(cat <<'PROMPT'
   <handoff prompt>
   PROMPT
   )"
   ```

5. Tell the user the line the command printed, the agent, and the mode.

## Write a handoff prompt

**The other session's agent sees none of this conversation, so the handoff prompt MUST stand alone.** Include each of these:

- the goal, in one or two sentences
- the context from this conversation that bears on the goal: files, findings, and decisions
- the constraints the user stated
- what done looks like
- the name of the current session, as the source of the task

Never put a secret into a handoff prompt: the prompt lands in the agent's transcript.

## Send a prompt to an existing session

1. Find the target in `ringleader list --json`. If its `state` is neither `working` nor `waiting`, tell the user and stop.
2. If the target is the current session, stop: a prompt sent there types into this agent's own prompt box.
3. Write the handoff prompt, per Write a handoff prompt.
4. Run this command. If the shell tool takes a timeout, give the command at least 90 seconds. The command waits for the target's agent to reach its prompt.

   ```sh
   ringleader send <name> "$(cat <<'PROMPT'
   <handoff prompt>
   PROMPT
   )"
   ```

## Other actions

| Action | Command | Rule |
| --- | --- | --- |
| List | `ringleader list --json` | Report each session's `name`, `state`, and `branch`. |
| Rename | `ringleader rename <old> <new>` | Run only when the user asks. |
| Restart | `ringleader restart <name>...` | Run only when the user asks. |
| Stop | `ringleader stop <name>...`, or `ringleader stop --repo <repo>` for every waiting or exited session of a repo | Run only when the user asks. `ringleader restart <name>` resumes it. |
| Delete | `ringleader delete <name>...` | Run only when the user asks. Never delete the current session. |

- If the user asks for `--force` or `--forget`, pass that flag to `delete`. Otherwise, never pass either flag.
- Never run `ringleader attach`: it takes over this agent's terminal.

## Handle the exit code

| Exit code | Meaning | Action |
| --- | --- | --- |
| 0 | Success | Report what the command printed. |
| 1 | Invalid input or another failure | Follow the rule below this table. |
| 2 | No session or repo has that name | Report the name, and stop. |
| 3 | A conflict with existing state | Follow the rule below this table. |
| Any other code | The command did not finish | Report the exit code and stderr, and stop. |

- On exit code 1, read stderr. If stderr says the agent has no `plan_args`, rerun the command once without `--plan`. Then tell the user the session started in build mode. Otherwise, report stderr and stop.
- On exit code 3, read stderr. If stderr says the name already exists as a session, branch, or worktree, rerun the command once with another name. Otherwise, report stderr and stop. Stderr names the conflict, such as a held lock, a stopped session, or an agent away from its prompt.
- If a rerun fails too, report its stderr and stop.
