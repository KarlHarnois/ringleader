# Source folders

**This file holds the folder rules that the folder names under `src/` cannot show.**

**A layer folder is a folder directly under `src/`.** AGENTS.md decides which layer folder a file goes in.

**A file's importers are the other files that import the file outside `#[cfg(test)]` code and `src/test_support/`.**

**Inside a section, the first numbered rule that a file meets places the file.** If no rule places a file, put the file in the folder whose name describes the file's role. If no folder name describes the file's role, put the file at the root of its layer folder.

## Folder limits

- Never make a folder more than one level below a layer folder, except directly inside `ui/dialogs/`.
- Unless the project owner asks for a new folder, never make a new folder outside `adapters/`.
- Never silence clippy's `module_inception` lint with `#[allow]` or `#[expect]`.

## `adapters/`

1. A null adapter goes in the folder of the outside system it stands in for.
2. A file that talks to an outside system goes in the folder named after that outside system. If that folder does not exist, make it.
3. A file that talks to no outside system goes in a folder that holds one of the file's importers. If no folder holds an importer, ask the project owner.

## `application/`

**The rules below place every file in `application/` except a port.**

**A use-case folder is a folder under `application/` other than `checks/` and `ports/`.**

**A use case's entry file is the file whose primary type runs the use case.**

**A shared check is a file that meets two conditions.** Its primary type decides whether an action may proceed. Its importers are not all in one use-case folder.

1. A file whose primary type is `SessionService`, or a type that `SessionService::new` takes, goes at the root.
2. If a use case fits in one file, the use case's entry file goes at the root.
3. Any other entry file goes in its use case's folder.
4. A shared check goes in `checks/`.
5. A file with importers in two or more use-case folders goes at the root.
6. A file with importers in exactly one use-case folder goes in that use-case folder.
7. Any other file goes at the root.

## `domain/`

**A config type is a type that holds or checks a part of the config.** A file whose primary type is a config type goes at the `domain/` root, never in a subfolder.
