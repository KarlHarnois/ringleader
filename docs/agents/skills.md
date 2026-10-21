# Skills

**Every rule in this file binds each change to one of these:**

- a skill under `.agents/skills/` or `skills/`
- how a launch gives an agent the built-in skill
- the CLI, when the change makes a statement in the built-in skill false

## Repository skills

**A repository skill is a skill for the agents that work in this repository.**

- Put a procedure that an agent invokes on demand in a repository skill at `.agents/skills/<name>/SKILL.md`.
- Link each repository skill from `.claude/skills/<name>` with a relative symlink: Claude Code reads a repository's skills only from `.claude/skills/`.
- Never name a Ringleader type, path, or rule in a repository skill: each repository skill ports unchanged to other repos.

## The built-in skill

**The built-in skill is the skill in `skills/`, which the binary embeds and each launch hands to its agent.**

- If a change makes a statement in the built-in skill false, update the skill in the same commit.
- Never write the built-in skill into a worktree, a repo, or an agent's own config directory.
- Pass `--plugin-dir` to claude right after the program, on every launch. Claude never restores the flag when it resumes a conversation.
- Merge the skill folder into the `OPENCODE_CONFIG_CONTENT` that opencode already gets. Never replace the variable: the user's value carries the user's own settings.
- Never set `OPENCODE_CONFIG_DIR`: opencode 2 then ignores the user's whole global config.
- If publishing the skill fails, log the failure and launch the agent without the skill.
- Keep the `built_in_skill = false` opt-out: a managed Claude Code policy can stop claude from starting with `--plugin-dir`.
