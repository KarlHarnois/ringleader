# Homebrew

**Every rule in this file binds each change to `pinnedTools` in `flake.nix`, to the release workflow, and to the version.** The `KarlHarnois/homebrew-tap` repository holds the Homebrew formula. The formula bundles its own build of each tool in `pinnedTools`, at the version the flake pins.

## Pinned tools

**Keep the name `pinnedTools` and its shape, a wrapper variable mapped to a package.** The tap's CI compares `packages.<system>.default.pinnedTools` at each release tag with the formula's bundled tools.

## Releases

**A release is a tag `v<version>` on a commit of `master`.** The release workflow tags each `master` commit that passes CI, and then tells the tap about the tag. The tap publishes the formula for the tag.

- NEVER change the version in `Cargo.toml`: the tap writes each tag's version into `Cargo.toml` and `Cargo.lock` before it builds.
- A build outside Homebrew reports the version in `Cargo.toml`, which stays `0.1.0`.
- NEVER push a release tag by hand: the release workflow computes each tag from the latest one.
- If the release workflow fails, show its log to the project owner.
