# Usage

**Every rule in this file binds each change to how Ringleader measures an agent's usage.** A probe is a run of an agent's own command that reports the agent's usage.

**Never read an agent's credentials.** The probe's command authenticates itself.

- Never read Claude Code's OAuth token: Claude Code refreshes the token itself, and a second refresher logs Claude Code out.

**Take a provider's key only from the variables the agent gets.** An example is an `OLLAMA_API_KEY` that the agent names in `env`. Each agent can use its own account, so one key for every agent can report another account's quota.

**Never put a key that Ringleader reads in a child process's arguments or environment.** Any process on the host can read a child's arguments through `ps`. Any process of the same user can read its environment through `/proc`. Pass such a key on the child's standard input instead.

**Run every probe in Ringleader's usage directory, never in a worktree.** A worktree's `.envrc` or project settings could switch the probe to another account.

- Give a probe the variables the agent gets at launch, without the worktree's `.envrc`. The agent's command needs them to authenticate, and the agent holds them already. The agent's `set_env` picks the account, for example with `CLAUDE_CONFIG_DIR`.

**Measure once when the TUI starts, then only while the usage view is open.** A Claude probe starts a whole Claude Code process.

**Measure every agent at the same time.** The view then waits for the slowest probe instead of the sum of all probes.

**Classify a Claude limit by its `kind`, never by its label.** Show Claude's limits in the order the server sent. Claude Code marks `usage_report` as experimental, and the server owns the labels and the order.

**Label opencode's cost as a list price, never as the amount billed.** opencode prices each token at the model's list price, and a subscription such as Ollama Cloud bills differently.

**Open opencode's database read-only.** Without `-readonly`, `sqlite3` creates a missing database file.

**If the direct read of opencode's database fails, ask `opencode db` instead.** opencode owns the database's path and schema, and a new opencode release can change either.
