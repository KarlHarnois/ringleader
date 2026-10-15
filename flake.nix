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
          wrapTool =
            name: tool: runtime:
            pkgs.writeShellApplication (
              runtime
              // {
                inherit name;
                text = ''exec ${tool}/bin/${name} "$@"'';
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
          mutatorApp = wrapTool "mutator" mutatorTool coverageRuntime;
        };

      forEachSystemWithMetricsTools = build: forEachSystem (pkgs: build pkgs (metricsTools pkgs));

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
            description = "A tmux session manager for Claude and opencode";
            license = pkgs.lib.licenses.mit;
            mainProgram = "ringleader";
            platforms = supportedSystems;
          };
        };
      });

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
              tools.mutatorTool
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
        dev-shell-tools = devShellTools nixpkgs.legacyPackages.${system} self.devShells.${system}.default;
      });
    };
}
