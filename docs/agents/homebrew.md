# Homebrew

**Every rule in this file binds each change to `pinnedTools` or to the package's wrapper in `flake.nix`, and each release.** The `KarlHarnois/homebrew-tap` repository holds the Homebrew formula. The formula bundles its own build of each tool in `pinnedTools`, at the version the flake pins.

## Pinned tools

- Pin each tool that the package's wrapper names through `pinnedTools`. Never write a `--set-default` into `postInstall` by hand: the formula bundles only the tools in `pinnedTools`.
- Keep the name `pinnedTools` and its shape, a wrapper variable mapped to a package. The tap's CI compares `packages.<system>.default.pinnedTools` at each release tag with the formula's bundled tools.

## Releases

**A release is a tag `v<version>` on a commit of `master`.** The formula installs only a release.

1. Bump the version in `Cargo.toml` and in the README version badge.
2. After the project owner merges that change, ask the project owner to push the tag `v<version>` on the merge commit.
3. In the tap, open a PR that sets the formula's URL and sha256 to the new tag. In the same PR, set each bundled tool to the version that `pinnedTools` pins at the tag.
4. If the tap's CI fails, fix the formula and push again. If the same check fails three times, stop and show its output to the project owner.
5. Once the tap's CI passes, ask the project owner to add the `pr-pull` label. The label publishes the bottles.
