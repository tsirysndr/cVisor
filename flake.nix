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
          ];
        };
      });
}
