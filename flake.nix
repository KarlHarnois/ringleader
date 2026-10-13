{
  description = "Ringleader, a tmux session manager for Claude and opencode";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";
    dryer = {
      url = "github:unclebob/dryer";
      flake = false;
    };
    crapper = {
      url = "github:unclebob/crapper";
      flake = false;
    };
    mutator = {
      url = "github:unclebob/mutator";
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
          mutatorTool = buildTool "mutator" mutator [ crapperTool ];
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
            pkgs.gitMinimal
            pkgs.tmux
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
            packages = developmentPackages pkgs;
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
      });
    };
}
