---
name: checking-hexagonal-boundaries
description: This skill checks whether an architectural decision respects a project's hexagonal architecture, also called ports and adapters. A decision is a proposed design or the code a branch changes. The skill applies the project's own documented architecture rules first, then generic hexagonal checks. It reports each broken rule with its evidence and the placement that fixes it, and edits no file. Use this skill before you add a port, an adapter, or a type that another layer imports. Also use it when the user asks to check, validate, review, or audit a design, a plan, or a branch for layering, dependency direction, ports, adapters, or core purity, or asks where code belongs. Trigger phrases include "hex arch", "wrong layer", "missing port", and "business logic in an adapter".
---

# Checking hexagonal boundaries

**Never edit a file while you run this skill:** the user decides what to fix.

## Terms

- A **decision** is a choice of where a responsibility lives: its layer, its port, or its adapter.
- A **proposal** is a decision written as prose: a plan, a design, or a question about where code goes.
- A **code change** is a decision written as code: the changes on a branch that its base lacks.
- The **core** is the code that holds the business rules and the use cases.
- A **technology** is a system outside the process, such as a database, a file system, or a child process.
- A **port** is an interface that the core owns and calls to reach a technology.
- A **driven adapter** is code that implements a port with a technology.
- A **driving adapter** is code that turns outside input into calls to the core.
- The **composition root** is the one place that builds the adapters and wires them to the core.
- A **role** is a value in the Role column of the folder name table.
- The **layer map** gives each source folder or file its role.
- The **project rules** are the architecture rules that the project's documents state.
- A **hexagonal check** is one row of the Hexagonal checks table.
- A **finding** is one place where a decision breaks one project rule or one hexagonal check.

## Procedure

1. Load the project rules and the layer map, per Project rules.
2. Pick the mode:
   - If the decision is prose, or a file of prose, run Check a proposal.
   - Otherwise, run Check a code change.
3. Check the known violations, per Known violations.
4. Write the report, per Report.

## Project rules

1. Read each of these files that exists at the repository root: `AGENTS.md`, `CLAUDE.md`, `ARCHITECTURE.md`, and `README.md`.
2. Read each file that those files import or name for architecture work.
3. Build the layer map from what those files state.
4. If those files give no folder a role, infer the layer map from folder names, per the folder name table. Mark the map as assumed in the report.
5. If no folder maps to the core, ask the user for the layer map and stop.

**A project rule wins over a hexagonal check it contradicts.**

**The folder name table maps common folder names to roles:**

| Role | Common folder names |
| --- | --- |
| Core | `domain`, `core`, `model`, `entities`, `application`, `usecases` |
| Ports | `ports`, `interfaces`, `boundaries` |
| Driven adapters | `adapters`, `infrastructure`, `infra`, `persistence`, `driven` |
| Driving adapters | `cli`, `ui`, `web`, `api`, `http`, `controllers`, `driving` |
| Composition root | the program's entry point, such as `main` |

If a folder matches no row, read its files to decide its role.

## Check a proposal

**Never skip a check because the proposal has no code yet.**

1. List each responsibility the proposal places, with the layer, port, and adapter the proposal gives it.
2. List each dependency between layers that the proposal adds.
3. Check each listed item against every project rule and every hexagonal check.
4. If the proposal leaves a check undecidable, report an unsure finding with the question that decides it.

## Check a code change

1. Find the base. If the user named a base, use it. Otherwise, the base is the output of `git rev-parse --abbrev-ref origin/HEAD`. If that command fails, ask the user for the base and stop.
2. If the user named a branch, the head is that branch. Otherwise, the head is `HEAD`.
3. Run `git merge-base <base> <head>`. Its output is the fork point. If the command fails, report its error to the user and stop.
4. If the head is not `HEAD`, the changed files are the files `git diff --name-only <fork point> <head>` lists.
5. If the head is `HEAD`, the changed files are the files `git diff --name-only <fork point>` and `git ls-files --others --exclude-standard` list.
6. Skip each changed file that the layer map gives no role.
7. Read each remaining changed file whole, and its diff against the fork point. If the head is not `HEAD`, read each file with `git show <head>:<path>`.
8. Check each import and each module path in those files against the layer map.
9. Check each of those files against every project rule and every hexagonal check.

## Hexagonal checks

| Check | A break looks like |
| --- | --- |
| Dependencies point inward | The core imports an adapter, a UI library, or an IO library. A driving adapter imports a driven adapter. |
| The core owns its ports | A port lives in an adapter's folder. The core names a concrete adapter type. |
| Ports speak the core's language | A port's name or any part of its signature names a technology. An error carries an exit code, an output stream, or a status code. |
| The core spells out no technology's syntax | The core builds a technology's own syntax, such as a command line, a query, or a URL. |
| The core holds every business rule | An adapter branches on a domain state, computes a domain answer, or enforces an invariant. |
| Driving adapters enter through the core | A driving adapter calls a driven adapter, or reaches past the core's entry points. |
| The core reads no ambient input | The core reads the clock, the environment, the working directory, or a random source. |
| Each port stands for a technology | A port has no implementation outside the core. |
| The composition root only builds and wires | The composition root runs a use case or branches on a business rule. |
| The core runs on fakes | A new port has no fake. A core test starts a process, opens a socket, or touches the file system. |
| Names state the role | A port is named after a technology. An adapter's name omits its technology or its port. |

## Known violations

**A known violation is a break that the project's documents already list as known.** Never report a known violation as a finding. If the decision copies or extends a known violation, report a finding.

If a known violation links an issue, read the issue to see the code it names. If you cannot read the issue, match on the listed description, and say so in the report.

## Report

**Print the report in the chat.** If you run as a subagent, return the report as your final message. NEVER list a check that passes.

**If a project rule and a hexagonal check both cover a break, cite the project rule.**

1. A verdict line. If there is no finding, write `Respects the architecture`. If every finding is unsure, write `Undecided: <m> questions`. Otherwise, write `Breaks the architecture: <n> findings, <m> of them unsure`.
2. The layer map, one line per role. If the map is assumed, say so.
3. A findings table with one row per finding and these columns:

   | Column | Content |
   | --- | --- |
   | Rule | The project rule's document, section, and sentence, or the hexagonal check's name |
   | Where | `file:line` for a code change, or the proposal's sentence |
   | Break | What breaks the rule, in one sentence |
   | Fix | The layer, port, or adapter that holds the responsibility instead |
   | Sure | `yes`, or the question that decides the finding |

4. Candidate rules: each broken hexagonal check that no project rule states, one line each.
5. The known violations that the decision touches.

If a part is empty, write `None` under it.
