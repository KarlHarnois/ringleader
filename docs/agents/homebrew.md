# Homebrew

**Every rule in this file binds all work on `pinnedTools` in `flake.nix` and all work on releases, including `.github/workflows/release.yml`.** The `KarlHarnois/homebrew-tap` repository holds the Homebrew formula. The formula bundles its own build of each tool in `pinnedTools`, at the version the flake pins.

## Pinned tools

**Keep the name `pinnedTools` and its shape, a wrapper variable mapped to a package.** The tap's CI compares `packages.<system>.default.pinnedTools` at each release tag with the formula's bundled tools.

## Releases

**A release is a release commit on `master` that carries a tag `v<version>`.** A release commit is a commit whose title is `Release v<version>`. The release workflow adds a release commit on top of each `master` commit that passes CI. The release commit sets the version in `Cargo.toml` and `Cargo.lock`. The workflow pushes the release commit and its tag with the `RELEASE_DEPLOY_KEY` deploy key. The `master` ruleset lets deploy keys bypass its rules. When CI passes on the release commit, the workflow tells the tap about the tag.

- NEVER change the version in `Cargo.toml` or `Cargo.lock` by hand: only a release commit sets it.
- A `nix build` has no `.git`, so it reports the version in `Cargo.toml`.
- NEVER push a release tag by hand: the release workflow computes each tag from the latest one.
- If the release workflow fails, show its log to the project owner.
