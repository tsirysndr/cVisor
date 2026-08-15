{
  description = "cvisor — in-process Linux sandbox CLI";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    crane.url = "github:ipetkov/crane";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, crane, flake-utils, rust-overlay }:
    # Linux only — the sandbox runtime (seccomp user-notifier, pidfd) is Linux.
    flake-utils.lib.eachSystem [ "x86_64-linux" "aarch64-linux" ] (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ (import rust-overlay) ];
        };

        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "clippy" "rustfmt" ];
          targets = [ "x86_64-unknown-linux-musl" "aarch64-unknown-linux-musl" ];
        };

        craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;

        src = craneLib.cleanCargoSource ./.;

        # The web UI (ui/) is built with bun and embedded into the CLI via
        # rust-embed. Nix builds have no network, so dependency installation is a
        # fixed-output derivation (its output is content-addressed by hash).
        webDepsSrc = pkgs.lib.fileset.toSource {
          root = ./ui;
          fileset = pkgs.lib.fileset.unions (
            [ ./ui/package.json ]
            ++ pkgs.lib.optional (builtins.pathExists ./ui/bun.lockb) ./ui/bun.lockb
            ++ pkgs.lib.optional (builtins.pathExists ./ui/bun.lock) ./ui/bun.lock
          );
        };

        webNodeModules = pkgs.stdenv.mkDerivation {
          pname = "cvisor-web-node-modules";
          version = "0.1.0";
          src = webDepsSrc;
          nativeBuildInputs = [ pkgs.bun ];
          dontConfigure = true;
          buildPhase = ''
            runHook preBuild
            export HOME=$(mktemp -d)
            bun install --frozen-lockfile --no-progress
            runHook postBuild
          '';
          installPhase = ''
            runHook preInstall
            mv node_modules $out
            runHook postInstall
          '';
          dontFixup = true;
          # Fixed-output: pin the installed node_modules by hash. The tree is
          # NOT platform-independent (native optional deps like
          # @tailwindcss/oxide-* install per-OS/CPU), so each system has its own.
          # To (re)compute after a dependency change: set the entry to
          # pkgs.lib.fakeHash, run `nix build .#web-node-modules` on that system
          # in CI, and copy the hash the mismatch error reports. (Never nix build
          # locally.)
          outputHashMode = "recursive";
          outputHashAlgo = "sha256";
          outputHash = {
            x86_64-linux = pkgs.lib.fakeHash;
            aarch64-linux = pkgs.lib.fakeHash;
          }.${system};
        };

        web = pkgs.stdenv.mkDerivation {
          pname = "cvisor-web";
          version = "0.1.0";
          src = ./ui;
          nativeBuildInputs = [ pkgs.bun pkgs.nodejs ];
          configurePhase = ''
            runHook preConfigure
            cp -r ${webNodeModules} node_modules
            chmod -R u+w node_modules
            patchShebangs node_modules
            export HOME=$(mktemp -d)
            runHook postConfigure
          '';
          buildPhase = ''
            runHook preBuild
            bun run build
            runHook postBuild
          '';
          installPhase = "cp -r dist $out";
        };

        commonArgs = {
          inherit src;
          strictDeps = true;
          pname = "cvisor";
          # Only the CLI, with all features (zstd archive + S3 cache backend).
          cargoExtraArgs = "--package cvisor-cli --bin cvisor --features zstd,s3";
          # zstd-sys and ring (S3 TLS) build their C with the stdenv compiler;
          # ring's build needs perl.
          nativeBuildInputs = [ pkgs.perl pkgs.pkg-config ];
          # Sandbox tests need seccomp/fork; skip them in the pure build.
          doCheck = false;
        };

        cargoArtifacts = craneLib.buildDepsOnly commonArgs;

        cvisor = craneLib.buildPackage (commonArgs // {
          inherit cargoArtifacts;
          # Drop the built web UI where rust-embed (crates/cvisor-cli,
          # #[folder = "../../ui/dist"]) expects it, before compiling the CLI.
          preBuild = ''
            mkdir -p ui
            cp -R ${web} ui/dist
            chmod -R u+w ui/dist
          '';
          meta = {
            description = "In-process Linux sandbox CLI";
            mainProgram = "cvisor";
            platforms = [ "x86_64-linux" "aarch64-linux" ];
          };
        });
      in
      {
        packages = {
          default = cvisor;
          cvisor = cvisor;
          # Exposed so CI can build them to (re)compute the node_modules hash.
          web = web;
          web-node-modules = webNodeModules;
        };

        apps.default = flake-utils.lib.mkApp {
          drv = cvisor;
          name = "cvisor";
        };

        checks.cvisor = cvisor;

        devShells.default = pkgs.mkShell {
          inputsFrom = [ cvisor ];
          packages = [
            rustToolchain
            pkgs.cargo-zigbuild
            pkgs.zig
            pkgs.perl
            pkgs.pkg-config
            pkgs.bun
            pkgs.nodejs
          ];
        };
      });
}
