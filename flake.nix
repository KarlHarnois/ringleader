{
  description = "Ringleader, a tmux session manager for AI coding agents";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";
    crane.url = "github:ipetkov/crane";
  };

  outputs =
    {
      self,
      nixpkgs,
      crane,
    }:
    let
      supportedSystems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
      ];

      forEachSystem =
        build: nixpkgs.lib.genAttrs supportedSystems (system: build nixpkgs.legacyPackages.${system});

      ciPackages = pkgs: [
        pkgs.rustc
        pkgs.cargo
        pkgs.clippy
        pkgs.rustfmt
        pkgs.tmux
        pkgs.git
        pkgs.direnv
        pkgs.cargo-llvm-cov
      ];

      developmentPackages =
        pkgs:
        ciPackages pkgs
        ++ [
          pkgs.nixfmt
          pkgs.util-linux
          pkgs.rust-analyzer
        ];

      llvmCoverageEnv = pkgs: {
        LLVM_COV = "${pkgs.rustc.llvmPackages.llvm}/bin/llvm-cov";
        LLVM_PROFDATA = "${pkgs.rustc.llvmPackages.llvm}/bin/llvm-profdata";
      };

      crateVersion = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).package.version;

      revisionSuffix =
        nixpkgs.lib.optionalString (self ? shortRev || self ? dirtyShortRev)
          "-g${self.shortRev or self.dirtyShortRev}";

      pinnedTools = pkgs: {
        RINGLEADER_TMUX = pkgs.tmux;
        RINGLEADER_GIT = pkgs.gitMinimal;
      };

      pinnedToolDefaults =
        tools:
        nixpkgs.lib.concatMapAttrsStringSep " " (
          variable: tool: "--set-default ${variable} ${nixpkgs.lib.getExe tool}"
        ) tools;

      pinnedToolsWrapper =
        pkgs: package:
        pkgs.runCommand "pinned-tools-wrapper"
          { wrapperCommand = builtins.unsafeDiscardStringContext package.postInstall; }
          ''
            wrapped=$({ grep --only-matching --regexp='--set-default RINGLEADER_[A-Z_]*' <<< "$wrapperCommand" || true; } | cut --delimiter=' ' --fields=2 | sort)
            pinned=$(printf '%s\n' ${nixpkgs.lib.escapeShellArgs (builtins.attrNames package.pinnedTools)} | sort)
            [[ $wrapped == "$pinned" ]] || {
              echo "The wrapper sets" ''${wrapped:-nothing} "but pinnedTools lists" ''${pinned:-nothing}". Pin each tool the wrapper names through pinnedTools: the Homebrew tap bundles only those." >&2
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
        default =
          let
            craneLib = crane.mkLib pkgs;
            tools = pinnedTools pkgs;
            buildArgs = {
              pname = "ringleader";
              version = crateVersion;
              src = self;
              cargoExtraArgs = "--locked --package ringleader";
              doCheck = false;
            };
          in
          craneLib.buildPackage (
            buildArgs
            // {
              cargoArtifacts = craneLib.buildDepsOnly (buildArgs // { cargoCheckCommand = "true"; });
              nativeBuildInputs = [ pkgs.makeWrapper ];
              RINGLEADER_VERSION = crateVersion + revisionSuffix;
              postInstall = ''
                wrapProgram $out/bin/ringleader ${pinnedToolDefaults tools}
              '';
              passthru.pinnedTools = tools;
              meta = {
                description = "A tmux session manager for AI coding agents";
                license = pkgs.lib.licenses.mit;
                mainProgram = "ringleader";
                platforms = supportedSystems;
              };
            }
          );
      });

      devShells = forEachSystem (pkgs: {
        default = pkgs.mkShell {
          packages = developmentPackages pkgs;
          env = llvmCoverageEnv pkgs;
          shellHook = ''
            if [[ -z ''${CI:-} ]]; then
              export CARGO_BUILD_JOBS=''${CARGO_BUILD_JOBS:-2}
            fi
          '';
        };
        ci = pkgs.mkShell {
          packages = ciPackages pkgs;
          env = llvmCoverageEnv pkgs;
        };
      });

      formatter = forEachSystem (pkgs: pkgs.nixfmt-tree);

      checks = nixpkgs.lib.genAttrs supportedSystems (system: {
        default = self.packages.${system}.default;
        agent-docs = agentDocs nixpkgs.legacyPackages.${system};
        pinned-tools-wrapper =
          pinnedToolsWrapper nixpkgs.legacyPackages.${system}
            self.packages.${system}.default;
        dev-shell-tools = devShellTools nixpkgs.legacyPackages.${system} self.devShells.${system}.default;
      });
    };
}
