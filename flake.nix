{
  description = "tt: task + time tracker (CLI, daemon, server, web)";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/1f607aa79b6356ed45d3cd3c8079c196e79efa2c"; # nixos-26.05 (26 oct 2026)
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs { inherit system; };
        lib = pkgs.lib;
        version = "0.1.0";

        pnpmPkg = pkgs.pnpm_11;
        nodejsPkg = pkgs.nodejs_24;

        # Rust binaries: `tt` (CLI + daemon) and `tt-server`.
        tt = pkgs.rustPlatform.buildRustPackage {
          pname = "tt";
          inherit version;

          src = lib.fileset.toSource {
            root = ./.;
            fileset = lib.fileset.unions [
              ./Cargo.toml
              ./Cargo.lock
              ./crates
              ./contrib
              (lib.fileset.maybeMissing ./fixtures)
            ];
          };

          cargoLock.lockFile = ./Cargo.lock;
          nativeBuildInputs = [ pkgs.installShellFiles ];
          cargoBuildFlags = [
            "-p"
            "tt-cli"
            "-p"
            "tt-server"
          ];

          # Interop tests start a node sync server; keep the package build hermetic.
          doCheck = false;

          postInstall = ''
            install -Dm644 contrib/tt.service -t $out/share/systemd/user
            install -Dm644 contrib/tt-server.service -t $out/share/systemd/system
          ''
          + lib.optionalString (pkgs.stdenv.buildPlatform.canExecute pkgs.stdenv.hostPlatform) ''
            installShellCompletion --zsh --name _tt <($out/bin/tt completions zsh)
            installShellCompletion --zsh --name _tt-server <($out/bin/tt-server completions zsh)
          '';

          meta = {
            description = "Task and time tracker: scriptable CLI, hookable daemon, sync server";
            license = lib.licenses.mit;
            mainProgram = "tt";
            platforms = lib.platforms.linux;
          };
        };

        # Web app, built to a static directory for `tt-server serve --web-dir`.
        tt-web = pkgs.stdenv.mkDerivation (finalAttrs: {
          pname = "tt-web";
          inherit version;

          src = lib.fileset.toSource {
            root = ./.;
            fileset = lib.fileset.unions [
              ./package.json
              ./pnpm-lock.yaml
              ./pnpm-workspace.yaml
              ./packages
              (lib.fileset.difference ./web (
                lib.fileset.unions [
                  (lib.fileset.maybeMissing ./web/node_modules)
                  (lib.fileset.maybeMissing ./web/dist)
                  (lib.fileset.maybeMissing ./web/dev-dist)
                  (lib.fileset.maybeMissing ./web/test-results)
                  (lib.fileset.maybeMissing ./web/playwright-report)
                ]
              ))
              (lib.fileset.maybeMissing ./fixtures)
            ];
          };

          nativeBuildInputs = [
            nodejsPkg
            pnpmPkg
            (pkgs.pnpmConfigHook.override { pnpm = pnpmPkg; })
          ];

          pnpmDeps = pkgs.fetchPnpmDeps {
            pnpm = pnpmPkg;
            inherit (finalAttrs) pname version src;
            fetcherVersion = 3;
            hash = "sha256-qjP6qnDm6HP8ofJNBxGQtDwzMSHkXvXzEzxneZ/Lg1Y=";
          };

          buildPhase = ''
            runHook preBuild
            pnpm --filter web build
            runHook postBuild
          '';

          installPhase = ''
            runHook preInstall
            mkdir -p $out
            cp -r web/dist/. $out/
            runHook postInstall
          '';

          meta = {
            description = "tt web app (static bundle)";
            license = lib.licenses.mit;
          };
        });
      in
      {
        packages = {
          inherit tt tt-web;
          default = tt;
        };

        apps.default = {
          type = "app";
          program = "${tt}/bin/tt";
        };

        devShells.default = pkgs.mkShell {
          inputsFrom = [ tt ];

          packages =
            with pkgs;
            [
              cargo
              rustc
              clippy
              rustfmt
              rust-analyzer
              just
              sqlite
            ]
            ++ [
              nodejsPkg
              pnpmPkg
            ];

          RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
        };

        formatter = pkgs.nixfmt-rfc-style;
      }
    );
}
