# Source folders

**This file holds the rules for which folder under `src/` a file goes in.**

**A file goes where the first numbered rule it meets sends it.** Each layer section below lists its numbered rules.

**A module's importers are the production files that import the module, outside `#[cfg(test)]` code.**

## Every folder

- Never name a folder after a file it contains: clippy's `module_inception` lint fails `-D warnings`.
- Never silence `module_inception` with `#[allow]` or `#[expect]`.
- Never nest a folder inside a layer's subfolder, except under `ui/dialogs/`.
- Unless the project owner asks for a new folder, never make a new folder outside `adapters/`.

## `adapters/`

1. A null adapter goes in the folder of the technology it stands in for.
2. A file that talks to an outside system goes in the folder named after that system.
3. A file that talks to no outside system goes in the folder of a system whose files import the file.

- Never put a file at the `adapters/` root.
- If a file talks to an outside system that has no folder, make a folder named after the system.

## `application/`

**A use-case folder is a folder that rule 1 names.**

**A shared check is a module with both of these properties:**

- The module decides whether an action may proceed.
- The module's importers are not all in one use-case folder.

1. An entry module goes in its use-case folder. The entry modules are these:
   - `session_creation` and `session_restart` in `start/`
   - `session_deletion` in `deletion/`
   - `session_renaming` in `renaming/`
   - `session_merge` in `merge/`
   - `session_monitor` in `monitor/`
   - `repo_catalog` in `repos/`
   - `doctor` in `diagnostics/`
2. A shared check goes in `checks/`.
3. These modules go at the root:
   - `session_service`
   - `service_context`
   - `session_lifecycle`
   - `session_access`
   - `pull_request_catalog`
4. A module with importers in two or more use-case folders goes at the root.
5. A module with importers in one use-case folder goes in that folder.
6. Any other module goes at the root.

## `ui/`

1. A file whose primary type starts and collects one kind of background job goes in `background_jobs/`.
2. A file whose primary type maps a key press to an action, without running the action, goes in `keys/`.
3. A file whose primary type builds, lays out, or draws the rows of the session table goes in `table/`.
4. A file of the help screen goes in `help/`.
5. A file of the home screen goes in `home/`.
6. Any other file goes at the root.

## `ui/dialogs/`

1. A search part that both `repo_search/` and `session_finder` use goes in `fuzzy_search/`.
2. A file of the repo search that the new session dialog's repo picker runs goes in `repo_search/`.
3. A file of the new session dialog goes in `new_session/`.
4. Any other file goes at the root.

## `domain/`

1. A config type goes at the root.
2. A type of an entity goes in the folder named after that entity in the plural.
3. Any other type goes at the root.

## `test_support/`

**A hand-written fake is a test type that stands in for a port or for a driving adapter's channel.**

1. A file whose primary type is a hand-written fake goes in `fakes/`.
2. Any other file goes at the root.
