# Platform code

**Every rule in this file binds each change to a platform file, a seam, or a `target_os` cfg attribute.** Ringleader builds for Linux and Apple Silicon macOS.

**A platform file is a file whose `mod` line limits it to one target OS through a `target_os` cfg attribute.** A platform-free file is any other file under `src/`. A seam is a capability whose implementation the target OS picks, through one re-export in `adapters/mod.rs`.

- Write a `target_os` cfg attribute only on one of these:
  - a `mod` line under `adapters/`
  - a seam's re-export in `adapters/mod.rs`
  - the `compile_error!` in `adapters/mod.rs`
- Name a seam by its re-export, never by the platform type behind it.
- If the production code of only one target OS calls a platform-free file, compile the file under `#[cfg(any(target_os = "<that OS>", test))]`. Without that attribute, the other target OS builds the file as dead code.
- A change that edits a platform file is done only after the darwin cross-check passes.

## Darwin cross-check

**The darwin cross-check compiles every target, tests included, for macOS on Linux.** It runs no test.

**Never add the darwin cross-check toolchain to `flake.nix`, to a dev shell, or to CI.**

**If `Cargo.lock` gains a crate that compiles C, stop and ask the project owner.** The check has no macOS SDK for that crate.

To run the darwin cross-check:

1. Run `nix develop -c rustc --version`. The second word of its output is `<version>`.
2. Read the locked nixpkgs revision from `flake.lock`. That revision is `<revision>`.
3. Run this command, with `<version>` and `<revision>` filled in:

   ```sh
   nix develop -c nix shell --impure --expr '
     let
       nixpkgs = builtins.getFlake "github:NixOS/nixpkgs/<revision>";
       rustOverlay = builtins.getFlake "github:oxalica/rust-overlay";
       pkgs = import nixpkgs {
         system = builtins.currentSystem;
         overlays = [ rustOverlay.overlays.default ];
       };
     in
     pkgs.rust-bin.stable."<version>".minimal.override {
       extensions = [ "clippy" ];
       targets = [ "aarch64-apple-darwin" ];
     }' -c cargo clippy --all-targets --target aarch64-apple-darwin -- -D warnings
   ```

   The outer `nix develop` supplies the C compiler that build scripts link with. Keep the toolchain's version equal to the dev shell's: clippy then reports the same lints for both targets.

4. The darwin cross-check passes when the command exits 0.

If the toolchain fails to build, stop and show the output to the project owner. If the command reports an error, fix the error and run the command again. If the same error survives three fixes, stop and show the output to the project owner.
