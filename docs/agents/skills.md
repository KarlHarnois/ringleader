# Skills

**Every rule in this file binds each change to one of these:**

- a skill under `.agents/skills/` or `skills/`
- how a launch gives an agent the built-in skills
- the CLI, when the change makes a statement in a built-in skill false

## Repository skills

**A repository skill is a skill for the agents that work in this repository.**

- Put a procedure that an agent invokes on demand in a repository skill at `.agents/skills/<name>/SKILL.md`.
- Link each repository skill from `.claude/skills/<name>` with a relative symlink: Claude Code reads a repository's skills only from `.claude/skills/`.
- Never name a Ringleader type, path, or rule in a repository skill: each repository skill ports unchanged to other repos.

## Built-in skills

**A built-in skill is a skill in `skills/`, which the binary embeds and each launch hands to its agent.**

- If a change makes a statement in a built-in skill false, update that skill in the same commit.
- List each built-in skill in the Skills section of `README.md`.
- Never write a built-in skill into a worktree, a repo, or an agent's own config directory.
- Pass `--plugin-dir` to claude right after the program, on every launch. Claude never restores the flag when it resumes a conversation.
- Merge the folder that holds the built-in skills into the `OPENCODE_CONFIG_CONTENT` that opencode already gets. Never replace the variable: the user's value carries the user's own settings.
- Never set `OPENCODE_CONFIG_DIR`: opencode 2 then ignores the user's whole global config.
- Put the folder that links `ringleader` to the launching build first on the agent's `PATH`, after the `.envrc` loads. A session inherits the `PATH` of whatever created it. Without the link, its `ringleader` can be another build or none.
- If publishing the built-in skills fails, log the failure and launch the agent without them.
- Keep the `built_in_skill = false` opt-out: a managed Claude Code policy can stop claude from starting with `--plugin-dir`.
