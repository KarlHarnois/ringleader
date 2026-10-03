{
  description = "Ringleader, a tmux session manager for Claude and opencode";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";

  outputs =
    { self, nixpkgs }:
    let
      supportedSystems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forEachSystem =
        build: nixpkgs.lib.genAttrs supportedSystems (system: build nixpkgs.legacyPackages.${system});
    in
    {
      packages = forEachSystem (pkgs: {
        default = pkgs.rustPlatform.buildRustPackage {
          pname = "ringleader";
          version = "0.1.0";
          src = self;
          cargoLock.lockFile = ./Cargo.lock;
          nativeBuildInputs = [ pkgs.makeWrapper ];
          doCheck = true;
          nativeCheckInputs = [
            pkgs.git
            pkgs.tmux
          ];
          postInstall = ''
            wrapProgram $out/bin/ringleader \
              --set-default RINGLEADER_TMUX ${pkgs.tmux}/bin/tmux \
              --set-default RINGLEADER_GIT ${pkgs.git}/bin/git
          '';
          meta = {
            description = "A tmux session manager for Claude and opencode";
            license = pkgs.lib.licenses.mit;
            mainProgram = "ringleader";
            platforms = supportedSystems;
          };
        };
      });

      devShells = forEachSystem (pkgs: {
        default = pkgs.mkShell {
          packages = [
            pkgs.rustc
            pkgs.cargo
            pkgs.clippy
            pkgs.rustfmt
            pkgs.tmux
            pkgs.git
            pkgs.nixfmt
            pkgs.util-linux
          ];
        };
      });

      formatter = forEachSystem (pkgs: pkgs.nixfmt-tree);

      checks = nixpkgs.lib.genAttrs supportedSystems (system: {
        default = self.packages.${system}.default;
      });
    };
}
