{
  description = "Ringleader, a tmux session manager for Claude and opencode";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";
    dryer = {
      url = "github:unclebob/dryer";
      flake = false;
    };
    crapper = {
      url = "github:unclebob/crapper/9f1bead298b5a9d576bdd6319289fcf426e5b18a";
      flake = false;
    };
    mutator = {
      url = "github:unclebob/mutator/c57f03879a08d2afe8c7e044e86c80bb164afd30";
      flake = false;
    };
    uml-viewer = {
      url = "github:unclebob/uml-viewer";
      flake = false;
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      dryer,
      crapper,
      mutator,
      uml-viewer,
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
      ];

      metricsTools =
        pkgs:
        let
          python = pkgs.python3;
          treeSitterDeps = [
            python.pkgs.tree-sitter
            python.pkgs.tree-sitter-language-pack
          ];
          pyprojectVersion =
            source: (builtins.fromTOML (builtins.readFile "${source}/pyproject.toml")).project.version;
          buildTool =
            name: source: extraDependencies:
            python.pkgs.buildPythonApplication {
              pname = name;
              version = pyprojectVersion source;
              pyproject = true;
              src = source;
              doCheck = false;
              build-system = [ python.pkgs.setuptools ];
              dependencies = treeSitterDeps ++ extraDependencies;
            };
          dryerTool = buildTool "dryer" dryer [ ];
          crapperTool = buildTool "crapper" crapper [ ];
          mutatorTool = (buildTool "mutator" mutator [ crapperTool ]).overridePythonAttrs {
            patches = [ ./patches/mutator-copy-debug-build.patch ];
            patchFlags = [
              "-p1"
              "--fuzz=0"
            ];
          };
          llvm = pkgs.rustc.llvmPackages.llvm;
          llvmCoverageEnv = {
            LLVM_COV = "${llvm}/bin/llvm-cov";
            LLVM_PROFDATA = "${llvm}/bin/llvm-profdata";
          };
          gitRuntime = {
            runtimeInputs = [ pkgs.git ];
          };
          coverageRuntime = {
            runtimeInputs =
              developmentPackages pkgs
              ++ pkgs.stdenv.initialPath
              ++ [
                pkgs.stdenv.cc
                pkgs.cargo-llvm-cov
              ];
            runtimeEnv = llvmCoverageEnv;
          };
          pidNamespace = pkgs.lib.optionalString pkgs.stdenv.isLinux "${pkgs.util-linux}/bin/unshare --map-current-user --pid --fork --kill-child --mount-proc ";
          wrapTool = wrapToolBehind "";
          wrapToolBehind =
            prefix: name: tool: runtime:
            pkgs.writeShellApplication (
              runtime
              // {
                inherit name;
                text = ''exec ${prefix}${tool}/bin/${name} "$@"'';
              }
            );
        in
        {
          inherit
            dryerTool
            crapperTool
            mutatorTool
            llvmCoverageEnv
            ;
          dryerApp = wrapTool "dryer" dryerTool gitRuntime;
          crapperApp = wrapTool "crapper" crapperTool coverageRuntime;
          mutatorApp = wrapToolBehind pidNamespace "mutator" mutatorTool coverageRuntime;
        };

      forEachSystemWithMetricsTools = build: forEachSystem (pkgs: build pkgs (metricsTools pkgs));

      crateVersion = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).package.version;

      pinnedTools = pkgs: {
        RINGLEADER_TMUX = pkgs.tmux;
        RINGLEADER_GIT = pkgs.gitMinimal;
      };

      pinnedToolDefaults =
        tools:
        nixpkgs.lib.concatMapAttrsStringSep " " (
          variable: tool: "--set-default ${variable} ${nixpkgs.lib.getExe tool}"
        ) tools;

      readmeVersionBadge =
        pkgs:
        pkgs.runCommand "readme-version-badge" { } ''
          grep --fixed-strings --quiet 'badge/version-${crateVersion}-' ${./README.md} || {
            echo "The README version badge does not show ${crateVersion} from Cargo.toml" >&2
            exit 1
          }
          touch $out
        '';

      pinnedToolsWrapper =
        pkgs: package:
        pkgs.runCommand "pinned-tools-wrapper" { } ''
          wrapped=$(grep --only-matching '^export RINGLEADER_[A-Z_]*' ${nixpkgs.lib.getExe package} | cut --delimiter=' ' --fields=2 | sort)
          pinned=$(printf '%s\n' ${nixpkgs.lib.escapeShellArgs (builtins.attrNames package.pinnedTools)} | sort)
          [[ $wrapped == "$pinned" ]] || {
            echo "The wrapper sets" $wrapped "but pinnedTools lists" $pinned". Pin each tool the wrapper names through pinnedTools: the Homebrew tap bundles only those." >&2
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
      packages = forEachSystem (
        pkgs:
        let
          tools = pinnedTools pkgs;
        in
        {
          default = pkgs.rustPlatform.buildRustPackage {
            pname = "ringleader";
            version = crateVersion;
            src = self;
            cargoLock.lockFile = ./Cargo.lock;
            nativeBuildInputs = [ pkgs.makeWrapper ];
            doCheck = true;
            __darwinAllowLocalNetworking = true;
            nativeCheckInputs = builtins.attrValues tools ++ [ pkgs.direnv ];
            postInstall = ''
              wrapProgram $out/bin/ringleader ${pinnedToolDefaults tools}
            '';
            passthru.pinnedTools = tools;
            meta = {
              description = "A tmux session manager for Claude and opencode";
              license = pkgs.lib.licenses.mit;
              mainProgram = "ringleader";
              platforms = supportedSystems;
            };
          };
        }
      );

      apps = forEachSystemWithMetricsTools (
        pkgs: tools: {
          dryer = {
            type = "app";
            program = "${tools.dryerApp}/bin/dryer";
            meta.description = "Find candidate duplicate code";
          };
          crapper = {
            type = "app";
            program = "${tools.crapperApp}/bin/crapper";
            meta.description = "Score complexity, coverage, and CRAP";
          };
          mutator = {
            type = "app";
            program = "${tools.mutatorApp}/bin/mutator";
            meta.description = "Run mutation testing";
          };
        }
      );

      devShells = forEachSystemWithMetricsTools (
        pkgs: tools: rec {
          default = pkgs.mkShell {
            packages = developmentPackages pkgs ++ [ pkgs.rust-analyzer ];
          };
          metrics = pkgs.mkShell {
            inputsFrom = [ default ];
            packages = [
              pkgs.cargo-llvm-cov
              pkgs.clojure
              tools.dryerTool
              tools.crapperTool
              tools.mutatorApp
            ];
            env = tools.llvmCoverageEnv;
            UML_VIEWER_DEPS = ''
              {:deps {uml-viewer/uml-viewer {:local/root "${uml-viewer}"} quil/quil {:mvn/version "4.3.1563"}}
               :aliases {:ir {:main-opts ["-m" "uml-viewer.main.ir-generator"]}
                         :run {:main-opts ["-m" "uml-viewer.main.uml-viewer"]}}}'';
          };
        }
      );

      formatter = forEachSystem (pkgs: pkgs.nixfmt-tree);

      checks = nixpkgs.lib.genAttrs supportedSystems (system: {
        default = self.packages.${system}.default;
        readme-version-badge = readmeVersionBadge nixpkgs.legacyPackages.${system};
        agent-docs = agentDocs nixpkgs.legacyPackages.${system};
        pinned-tools-wrapper =
          pinnedToolsWrapper nixpkgs.legacyPackages.${system}
            self.packages.${system}.default;
        dev-shell-tools = devShellTools nixpkgs.legacyPackages.${system} self.devShells.${system}.default;
      });
    };
}
