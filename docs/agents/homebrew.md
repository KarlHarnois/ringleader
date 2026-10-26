# Homebrew

**Every rule in this file binds each change to `pinnedTools` in `flake.nix`, and each release.** The `KarlHarnois/homebrew-tap` repository holds the Homebrew formula. The formula bundles its own build of each tool in `pinnedTools`, at the version the flake pins.

## Pinned tools

**Keep the name `pinnedTools` and its shape, a wrapper variable mapped to a package.** The tap's CI compares `packages.<system>.default.pinnedTools` at each release tag with the formula's bundled tools.

## Releases

**A release is a tag `v<version>` on a commit of `master`.** The formula installs only a release.

1. Bump the version in `Cargo.toml`, in `Cargo.lock`, and in the README version badge. The formula builds with `cargo install --locked`, which rejects a `Cargo.lock` that names another version. If no tag names the version in `Cargo.toml` yet, skip this step: the release ships that version.
2. Ask the project owner to push the tag `v<version>` on the commit of `master` that names the version.
3. In the tap, open a PR that sets the formula's URL and sha256 to the new tag. In the same PR, set each bundled tool to the version that `pinnedTools` pins at the tag.
4. If the tap's CI fails, fix the formula and push again. If the same check fails three times, stop and show its output to the project owner.
5. Once the tap's CI passes, ask the project owner to add the `pr-pull` label. The label publishes the bottles.
