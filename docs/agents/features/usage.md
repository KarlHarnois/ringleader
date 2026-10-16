# Usage

**Every rule in this file binds each change to how Ringleader measures an agent's usage.** A probe is a run of an agent's own command that reports the agent's usage.

**Never read an agent's credentials.** The probe's command authenticates itself.

- Never read Claude Code's OAuth token: Claude Code refreshes the token itself, and a second refresher logs Claude Code out.

**Run every probe in Ringleader's usage directory, never in a worktree.** A worktree's `.envrc` or project settings could switch the probe to another account.

- Give a probe the variables the agent gets at launch, without the worktree's `.envrc`. The agent's `set_env` picks the account, for example with `CLAUDE_CONFIG_DIR`.

**Measure only while the usage view is open, one agent at a time.** A Claude probe starts a whole Claude Code process.

**Classify a Claude limit by its `kind`, never by its label.** Show Claude's limits in the order the server sent. Claude Code marks `usage_report` as experimental, and the server owns the labels and the order.
