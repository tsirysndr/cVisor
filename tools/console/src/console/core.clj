(ns console.core
  "cVisor console — a centralized REPL for every operational command in the
  monorepo (the Rust CLI/daemon + `cargo xtask` build cycle, the ten SDKs, the
  web UI + desktop shell, and the container images).

  Quick start (REPL):
      (require '[console.core :as c])
      (c/help)   ;; or (c/ls)

  Or as a one-shot with babashka:
      $ bb help
      $ bb daemon
      $ bb sdk:smoke scala"
  (:require [console.shell :as sh]))

(def ^:private registry
  "Hand-written index of every command grouped by namespace. Keeps `(help)`
  cheap and discoverable — namespaces are still loaded lazily."
  [{:group "build" :ns 'console.build
    :cmds [[:fmt            "cargo fmt --all"]
           [:fmt-check      "cargo fmt --all -- --check (CI gate)"]
           [:clippy         "cargo clippy --tests -D warnings"]
           [:test           "cargo test -p cvisor-core (pure-logic, any host)"]
           [:cli            "release build of the cvisor CLI"]
           [:daemon         "release build of the cvisord daemon"]
           [:run            "cargo run cvisor -- <args>. Args: forwarded to the CLI"]
           [:xtask-test     "cargo xtask test [arch] — full seccomp e2e in Alpine"]
           [:xtask-run      "cargo xtask run — run the sandbox in Alpine"]
           [:ffi            "cargo xtask ffi [arch] — build+distribute libcvisor.so"]
           [:run-node       "cargo xtask run-node — build napi + Node test"]
           [:node-artifacts "cargo xtask node-artifacts — libcvisor.node, all platforms"]
           [:clean          "cargo clean"]]}

   {:group "sdk" :ns 'console.sdk
    :cmds [[:deps      "Fetch deps. lang ∈ :node :elixir :gleam. Usage: (deps :node)"]
           [:test      "Native FFI unit suite (Linux + libcvisor). lang ∈ any. Usage: (test :ruby)"]
           [:smoke     "GraphQL remote smoke vs a daemon (any host). lang ∈ any. Usage: (smoke :python)"]
           [:lint      "Lint. lang ∈ :rust :go. Usage: (lint :rust)"]
           [:build     "Build the distributable artifact. lang ∈ any. Usage: (build :clojure)"]
           [:install   "Install locally. lang ∈ :clojure (~/.m2). Usage: (install :clojure)"]
           [:publish   "Publish to the registry. lang ∈ all but :go. Usage: (publish :scala)"]
           [:smoke-all "Every SDK's GraphQL smoke in turn (needs a running daemon)"]
           [:dir       "The repo-root-relative dir for lang. Usage: (dir :ruby)"]]}

   {:group "ui" :ns 'console.ui
    :cmds [[:install  "bun install"]
           [:dev      "bun run dev — Vite dev server"]
           [:build    "bun run build — tsc + vite build into ui/dist"]
           [:preview  "bun run preview"]
           [:tauri    "bun run tauri <sub> — desktop shell. Args: subcommand [args...]"]]}

   {:group "docker" :ns 'console.docker
    :cmds [[:build     "Build one image variant. Usage: (build :ubuntu)"]
           [:build-all "Build all three variants (alpine/trixie/ubuntu)"]]}])

(defn- pad [s n] (let [s (str s)] (str s (apply str (repeat (max 0 (- n (count s))) " ")))))

(defn ls
  "Print every registered command, grouped by namespace, with a one-liner."
  []
  (doseq [{:keys [group ns cmds]} registry]
    (println)
    (println (str "── " group "  (" ns ") ──"))
    (doseq [[sym desc] cmds]
      (println " " (pad sym 15) "  " desc)))
  :ok)

(defn help
  "Pretty banner + ls. Use this from the REPL for a quick tour."
  []
  (println)
  (println "cVisor Console — REPL-driven ops for the whole monorepo")
  (println "    (require '[console.build :as build])")
  (println "    (build/daemon)")
  (println "    (require '[console.sdk :as sdk])")
  (println "    (sdk/smoke :scala)   ;; lang is always a keyword (atom)")
  (println)
  (println "Commands:")
  (ls)
  (println)
  (println "From shell:   bb <task>     (see `bb tasks`)")
  (println "Repo root:   " (sh/repo-root))
  :ok)

(defn dispatch
  "Entry point for `clj -X console.core/dispatch :cmd :sdk/smoke :args [:scala]`."
  [{:keys [cmd args] :or {args []}}]
  (let [[grp sym] ((juxt namespace name) cmd)
        ns-sym    (symbol (str "console." grp))]
    (require ns-sym)
    (let [f (ns-resolve ns-sym (symbol sym))]
      (when-not f
        (throw (ex-info (str "Unknown command: " cmd) {:cmd cmd})))
      (apply f args))))
