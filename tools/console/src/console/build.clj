(ns console.build
  "The Rust workspace + `cargo xtask` build/test cycle — the cVisor CLI
  (`cvisor`), the daemon (`cvisord`), the FFI cdylib (libcvisor.so) and the
  napi node module. Thin pass-throughs over `cargo`; see `xtask/` and the
  workspace `Cargo.toml` for what each actually does.

  `:exclude [test]` — the name shadows the rarely-used `clojure.core/test`;
  the CI-mirroring one is worth keeping."
  (:refer-clojure :exclude [test])
  (:require [console.shell :as sh]))

;; -- workspace basics --------------------------------------------------------

(defn fmt
  "`cargo fmt --all` — format the whole workspace."
  [] (sh/sh ["cargo" "fmt" "--all"]))

(defn fmt-check
  "`cargo fmt --all -- --check` — CI's formatting gate."
  [] (sh/sh ["cargo" "fmt" "--all" "--" "--check"]))

(defn clippy
  "`cargo clippy --tests -D warnings` over the workspace."
  [] (sh/sh ["cargo" "clippy" "--tests" "--" "-D" "warnings"]))

(defn test
  "`cargo test -p cvisor-core` — the pure-logic unit tests (run on any host,
  macOS included). For the full seccomp e2e suite use `(xtask-test)`."
  [] (sh/sh ["cargo" "test" "-p" "cvisor-core"]))

(defn clean
  "`cargo clean`."
  [] (sh/sh ["cargo" "clean"]))

;; -- binaries ----------------------------------------------------------------

(defn cli
  "Release build of the `cvisor` CLI (embeds ui/dist — run `(ui/build)` first
  for the real UI)."
  [] (sh/sh ["cargo" "build" "-p" "cvisor-cli" "--bin" "cvisor" "--release"]))

(defn daemon
  "Release build of the `cvisord` daemon (gRPC + GraphQL)."
  [] (sh/sh ["cargo" "build" "-p" "cvisor-daemon" "--bin" "cvisord" "--release"]))

(defn run
  "`cargo run -p cvisor-cli --bin cvisor -- <args>` — build then run the CLI.
  E.g. `(run \"--\" \"uname\" \"-a\")` or `(run \"doctor\")`."
  [& args]
  (sh/sh (into ["cargo" "run" "-p" "cvisor-cli" "--bin" "cvisor" "--"] (map str args))))

;; -- cargo xtask (Alpine/musl e2e + FFI distribution) ------------------------

(defn xtask-test
  "`cargo xtask test [--arch <arch>]` — the full unit + e2e suite in Alpine
  under seccomp (Docker, musl). Optional arch: :x86_64 or :aarch64."
  ([] (sh/sh ["cargo" "xtask" "test"]))
  ([arch] (sh/sh ["cargo" "xtask" "test" "--arch" (name arch)])))

(defn xtask-run
  "`cargo xtask run` — run the sandbox binary in Alpine."
  [] (sh/sh ["cargo" "xtask" "run"]))

(defn ffi
  "`cargo xtask ffi [--arch <arch>]` — build libcvisor.so, patchelf its musl
  soname, and distribute it into every FFI SDK's native dir. Run this before
  the FFI-backed SDK tests. Optional arch: :x86_64 or :aarch64."
  ([] (sh/sh ["cargo" "xtask" "ffi"]))
  ([arch] (sh/sh ["cargo" "xtask" "ffi" "--arch" (name arch)])))

(defn run-node
  "`cargo xtask run-node` — build libcvisor.node (napi) and run the Node SDK
  test.ts in bun."
  [] (sh/sh ["cargo" "xtask" "run-node"]))

(defn node-artifacts
  "`cargo xtask node-artifacts` — build libcvisor.node for all four platforms."
  [] (sh/sh ["cargo" "xtask" "node-artifacts"]))
