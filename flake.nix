{
  description = "Ringleader, a tmux session manager for AI coding agents";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";
  };

  outputs =
    {
      self,
      nixpkgs,
    }:
    let
      supportedSystems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
      ];

      forEachSystem =
        build: nixpkgs.lib.genAttrs supportedSystems (system: build nixpkgs.legacyPackages.${system});

      developmentPackages = pkgs: [
        pkgs.rustc
        pkgs.cargo
        pkgs.clippy
        pkgs.rustfmt
        pkgs.tmux
        pkgs.git
        pkgs.nixfmt
        pkgs.util-linux
        pkgs.direnv
        pkgs.cargo-llvm-cov
      ];

      llvmCoverageEnv = pkgs: {
        LLVM_COV = "${pkgs.rustc.llvmPackages.llvm}/bin/llvm-cov";
        LLVM_PROFDATA = "${pkgs.rustc.llvmPackages.llvm}/bin/llvm-profdata";
      };

      crateVersion = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).package.version;

      readmeVersionBadge =
        pkgs:
        pkgs.runCommand "readme-version-badge" { } ''
          grep --fixed-strings --quiet 'badge/version-${crateVersion}-' ${./README.md} || {
            echo "The README version badge does not show ${crateVersion} from Cargo.toml" >&2
            exit 1
          }
          touch $out
        '';

      agentDocs =
        pkgs:
        pkgs.runCommand "agent-docs" { } ''
          fail() {
            echo "$1" >&2
            exit 1
          }
          guide=${./AGENTS.md}
          references=${./docs/agents}
          budget=25000
          size=$(wc -c < "$guide")
          (( size <= budget )) ||
            fail "AGENTS.md is $size bytes, over its budget of $budget. Move a rule that only one kind of task needs into a reference file under docs/agents/."
          if grep --quiet '^@' "$guide"; then
            fail "AGENTS.md imports a file with @, which loads the file in every session. Name the file in the Reference files table instead."
          fi
          for file in $(cd "$references" && find . -name '*.md' | sed 's#^\./##'); do
            grep --fixed-strings --quiet "| \`docs/agents/$file\` |" "$guide" ||
              fail "The Reference files table in AGENTS.md has no row for docs/agents/$file."
          done
          for file in $(grep --only-matching '^| `docs/agents/[^`]*`' "$guide" | sed 's#^| `docs/agents/##; s#`$##'); do
            [[ -f $references/$file ]] ||
              fail "The Reference files table in AGENTS.md names docs/agents/$file, which does not exist."
          done
          touch $out
        '';

      devShellTools =
        pkgs: defaultShell:
        pkgs.runCommand "dev-shell-tools" { } ''
          fail() {
            echo "$1" >&2
            exit 1
          }
          check_shell() {
            local analyzer pattern="RUST_SRC_PATH-'([^']*)'"
            analyzer=$(PATH=$1 && command -v rust-analyzer) || fail "a dev shell has no rust-analyzer"
            [[ $analyzer == /nix/store/* ]] || fail "a dev shell takes rust-analyzer from $analyzer"
            [[ $(< "$analyzer") =~ $pattern ]] || fail "rust-analyzer sets no RUST_SRC_PATH"
            [[ -f ''${BASH_REMATCH[1]}/core/src/lib.rs ]] || fail "RUST_SRC_PATH holds no core sources"
            ! (PATH=$1 && command -v rustup) || fail "a dev shell provides rustup"
          }
          check_shell ${pkgs.lib.makeBinPath defaultShell.nativeBuildInputs}
          touch $out
        '';
    in
    {
      packages = forEachSystem (pkgs: {
        default = pkgs.rustPlatform.buildRustPackage {
          pname = "ringleader";
          version = crateVersion;
          src = self;
          cargoLock.lockFile = ./Cargo.lock;
          nativeBuildInputs = [ pkgs.makeWrapper ];
          doCheck = true;
          __darwinAllowLocalNetworking = true;
          nativeCheckInputs = [
            pkgs.gitMinimal
            pkgs.tmux
            pkgs.direnv
          ];
          postInstall = ''
            wrapProgram $out/bin/ringleader \
              --set-default RINGLEADER_TMUX ${pkgs.tmux}/bin/tmux \
              --set-default RINGLEADER_GIT ${pkgs.gitMinimal}/bin/git
          '';
          meta = {
            description = "A tmux session manager for AI coding agents";
            license = pkgs.lib.licenses.mit;
            mainProgram = "ringleader";
            platforms = supportedSystems;
          };
        };
      });

      devShells = forEachSystem (pkgs: {
        default = pkgs.mkShell {
          packages = developmentPackages pkgs ++ [ pkgs.rust-analyzer ];
          env = llvmCoverageEnv pkgs;
          shellHook = ''
            if [[ -z ''${CI:-} ]]; then
              export CARGO_BUILD_JOBS=''${CARGO_BUILD_JOBS:-2}
            fi
          '';
        };
      });

      formatter = forEachSystem (pkgs: pkgs.nixfmt-tree);

      checks = nixpkgs.lib.genAttrs supportedSystems (system: {
        default = self.packages.${system}.default;
        readme-version-badge = readmeVersionBadge nixpkgs.legacyPackages.${system};
        agent-docs = agentDocs nixpkgs.legacyPackages.${system};
        dev-shell-tools = devShellTools nixpkgs.legacyPackages.${system} self.devShells.${system}.default;
      });
    };
}
