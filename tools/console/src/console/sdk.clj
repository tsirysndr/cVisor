(ns console.sdk
  "Unified per-language commands for the `sdks/*` packages — the same commands
  `.github/workflows/ci.yml` (FFI e2e) and `sdk-graphql.yml` (GraphQL smokes)
  run in CI.

  Every command takes a language as a plain keyword (an *atom*, in the Lisp
  sense — a bare scalar, not a call per language): `(test :clojure)`,
  `(build :ruby)`, `(publish :scala)`. A string works too (`(test \"ruby\")`) —
  handy from `bb`, where CLI args always arrive as strings.

  Two test flavors:
    `(test  <lang>)` — the native FFI-backed suite. Needs `libcvisor` (run
                       `(console.build/ffi)` first) and a Linux kernel with
                       seccomp=unconfined, so it's a Linux/CI command.
    `(smoke <lang>)` — the pure-HTTP GraphQL client smoke against a running
                       daemon (env CVISOR_GRAPHQL_URL / CVISOR_TOKEN). Runs on
                       any host, macOS included.

  `:exclude [test]` — shadows the rarely-used `clojure.core/test`."
  (:refer-clojure :exclude [test])
  (:require [babashka.fs :as fs]
            [clojure.string :as str]
            [console.shell :as sh]))

(def ^:private sdk-dir
  "Where each language's SDK lives, relative to the repo root. `:node` is the
  one npm package covering Node (napi), Bun and Deno."
  {:node    "sdks/node"
   :python  "sdks/python"
   :ruby    "sdks/ruby"
   :erlang  "sdks/erlang"
   :elixir  "sdks/elixir"
   :gleam   "sdks/gleam"
   :clojure "sdks/clojure"
   :go      "sdks/go"
   :rust    "sdks/rust"
   :scala   "sdks/scala"})

(defn- ->lang
  "Coerce a language argument to a keyword and validate it against `sdk-dir`,
  so both `:ruby` and `\"ruby\"` work."
  [lang]
  (let [k (keyword (name lang))]
    (when-not (contains? sdk-dir k)
      (throw (ex-info (str "unknown lang " k " (expected one of "
                           (str/join " " (sort (keys sdk-dir))) ")")
                      {:lang k})))
    k))

(defn dir
  "The repo-root-relative directory for `lang`. Usage: (dir :ruby)"
  [lang]
  (get sdk-dir (->lang lang)))

(defn- abs-dir [lang] (str (fs/path (sh/repo-root) (dir lang))))

(defn deps
  "Fetch dependencies where a language needs a separate step. lang ∈ :node
  :elixir :gleam (the rest resolve deps inside their own test/build step).
  Usage: (deps :node)"
  [lang]
  (case (->lang lang)
    :node   (sh/sh ["bun" "install"] {:dir (dir :node)})
    :elixir (sh/sh ["mix" "deps.get"] {:dir (dir :elixir)})
    :gleam  (sh/sh ["gleam" "deps" "download"] {:dir (dir :gleam)})
    (throw (ex-info (str "deps: no separate deps step for " (name lang)) {:lang lang}))))

(defn test
  "Run one SDK's native (FFI-backed) unit suite — the command ci.yml runs.
  Needs libcvisor + Linux (see the ns docstring). lang ∈ :node :python :ruby
  :erlang :elixir :gleam :clojure :go :rust :scala. Usage: (test :ruby)"
  [lang]
  (case (->lang lang)
    :node    (sh/sh ["cargo" "xtask" "run-node"])
    :python  (sh/sh ["uv" "run" "--group" "dev" "pytest" "-q"] {:dir (dir :python)})
    :ruby    (sh/sh ["ruby" "test/test_sandbox.rb"] {:dir (dir :ruby)})
    :erlang  (sh/run-steps (dir :erlang)
                           ["make" "all"]
                           ["sh" "-c" "erlc -o ebin test/cvisor_test.erl && erl -noshell -pa ebin -eval 'cvisor_test:run()' -s init stop"])
    :elixir  (sh/run-steps (dir :elixir)
                           ["mix" "deps.get"]
                           ["mix" "test"])
    :gleam   (sh/sh ["gleam" "test"] {:dir (dir :gleam)})
    :clojure (sh/sh ["clojure" "-M:test"] {:dir (dir :clojure)})
    :go      (sh/run-steps (dir :go)
                           ["go" "vet" "./..."]
                           ["go" "test" "./..."])
    :rust    (sh/sh ["cargo" "test"] {:dir (dir :rust)})
    :scala   (sh/run-steps (dir :scala)
                           ["sbt" "-batch" "compile" "Test/compile"]
                           ["sbt" "-batch" "test"])))

(defn smoke
  "Run one SDK's GraphQL remote smoke against a running daemon (env
  CVISOR_GRAPHQL_URL / CVISOR_TOKEN). Pure HTTP — no libcvisor — so it runs on
  any host. lang ∈ every SDK. Usage: (smoke :python)"
  [lang]
  (case (->lang lang)
    :node    (sh/sh ["bun" "test-remote.ts"] {:dir (dir :node)})
    :python  (sh/sh ["python" "remote_smoke.py"] {:dir (dir :python)})
    :ruby    (sh/sh ["ruby" "test/remote_smoke.rb"] {:dir (dir :ruby)})
    :erlang  (sh/run-steps (dir :erlang)
                           ["sh" "-c" "mkdir -p ebin && erlc -o ebin src/cvisor_graphql.erl src/cvisor_remote.erl test/cvisor_remote_smoke.erl && erl -noshell -pa ebin -eval 'cvisor_remote_smoke:run()' -s init stop"])
    :elixir  (sh/sh ["elixir" "remote_smoke.exs"] {:dir (dir :elixir)})
    :gleam   (sh/sh ["gleam" "run" "-m" "remote_smoke"] {:dir (dir :gleam)})
    :clojure (sh/sh ["clojure" "-M:remote"] {:dir (dir :clojure)})
    :go      (sh/sh ["go" "run" "./cmd/remote_smoke"] {:dir (dir :go) :extra-env {"CGO_ENABLED" "0"}})
    :rust    (sh/sh ["cargo" "run" "--example" "remote_smoke"] {:dir (dir :rust)})
    :scala   (sh/sh ["sbt" "-batch" "runMain dev.tsirysndr.cvisor.RemoteSmoke"] {:dir (dir :scala)})))

(defn lint
  "Lint one SDK. lang ∈ :rust (fmt --check + clippy -D warnings) :go (gofmt +
  vet). Usage: (lint :rust)"
  [lang]
  (case (->lang lang)
    :rust (sh/run-steps (dir :rust)
                        ["cargo" "fmt" "--check"]
                        ["cargo" "clippy" "--" "-D" "warnings"])
    :go   (sh/run-steps (dir :go)
                        ["gofmt" "-l" "."]
                        ["go" "vet" "./..."])
    (throw (ex-info (str "lint: no lint step wired for " (name lang)) {:lang lang}))))

(defn build
  "Build one SDK's distributable artifact. lang ∈ :node (tsc) :python (wheel +
  sdist) :ruby (.gem) :clojure (jar) :rust (release) :scala (jar + sources +
  javadoc) :erlang/:elixir/:gleam/:go (compile). Usage: (build :clojure)"
  [lang]
  (case (->lang lang)
    :node    (sh/sh ["bun" "run" "build"] {:dir (dir :node)})
    :python  (sh/sh ["uv" "build"] {:dir (dir :python)})
    :ruby    (sh/sh ["gem" "build" "cvisor.gemspec"] {:dir (dir :ruby)})
    :clojure (sh/sh ["clojure" "-T:build" "jar"] {:dir (dir :clojure)})
    :rust    (sh/sh ["cargo" "build" "--release"] {:dir (dir :rust)})
    ;; Maven Central needs the -sources and -javadoc jars, so `package` alone
    ;; wouldn't be a publishable build.
    :scala   (sh/sh ["sbt" "-batch" "package" "packageSrc" "packageDoc"] {:dir (dir :scala)})
    :erlang  (sh/sh ["rebar3" "compile"] {:dir (dir :erlang)})
    :elixir  (sh/sh ["mix" "compile"] {:dir (dir :elixir)})
    :gleam   (sh/sh ["gleam" "build"] {:dir (dir :gleam)})
    :go      (sh/sh ["go" "build" "./..."] {:dir (dir :go)})))

(defn install
  "Install a built artifact into the local package cache. lang ∈ :clojure
  (~/.m2). Usage: (install :clojure)"
  [lang]
  (case (->lang lang)
    :clojure (sh/sh ["clojure" "-T:build" "install"] {:dir (dir :clojure)})
    (throw (ex-info (str "install: no install step wired for " (name lang)) {:lang lang}))))

(defn- publish-ruby
  "gem build, then gem push whichever cvisor-*.gem it produced (sorted, so a
  stale gem from a previous build can't be pushed by mistake)."
  [args]
  (let [code (build :ruby)]
    (if-not (zero? code)
      code
      (let [gemfile (->> (fs/glob (abs-dir :ruby) "cvisor-*.gem")
                         (sort-by str)
                         last)]
        (when-not gemfile
          (throw (ex-info "gem build did not produce a .gem file" {:dir (abs-dir :ruby)})))
        (sh/sh (into ["gem" "push" (str (fs/file-name gemfile))] (map str args))
               {:dir (dir :ruby)})))))

(defn publish
  "Publish one SDK to its registry. lang ∈ :node (npm) :python (PyPI) :ruby
  (RubyGems) :clojure (Clojars) :rust (crates.io) :scala (Maven Central)
  :erlang/:elixir (Hex) :gleam (Hex). Extra args pass through to the underlying
  publisher.

  ⚠️ NOT sandboxed and NOT dry-run — this really pushes to the public registry.
  Each publisher expects its own credentials already configured (npm/uv/gem
  login, CLOJARS_USERNAME/PASSWORD, `cargo login`, a Hex key, and for Scala a
  published GPG key + Sonatype token). The console manages none of them.

  Usage: (publish :scala) | (publish :gleam \"--dry-run\")"
  [lang & args]
  (case (->lang lang)
    ;; The node package ships per-platform napi artifacts, so its publisher is
    ;; a script (build:native + publish all packages), not a bare `npm publish`.
    :node    (sh/sh (into ["bun" "scripts/publish.ts"] (map str args)) {:dir (dir :node)})
    :python  (sh/run-steps (dir :python)
                           ["uv" "build"]
                           (into ["uv" "publish"] (map str args)))
    :ruby    (publish-ruby args)
    :clojure (sh/sh (into ["clojure" "-T:build" "deploy"] (map str args)) {:dir (dir :clojure)})
    :rust    (sh/sh (into ["cargo" "publish"] (map str args)) {:dir (dir :rust)})
    ;; Two steps: publishSigned stages a signed bundle, then publish-central.sh
    ;; zips it and POSTs it to the Central Portal's publisher API (sbt-sonatype's
    ;; release path can't talk to the Portal). Needs a Central Portal user token
    ;; in CENTRAL_TOKEN_USERNAME/CENTRAL_TOKEN_PASSWORD.
    :scala   (sh/run-steps (dir :scala)
                           ["sbt" "-batch" "publishSigned"]
                           (into ["./publish-central.sh"] (map str args)))
    :erlang  (sh/sh (into ["rebar3" "hex" "publish"] (map str args)) {:dir (dir :erlang)})
    :elixir  (sh/sh (into ["mix" "hex.publish"] (map str args)) {:dir (dir :elixir)})
    :gleam   (sh/sh (into ["gleam" "publish"] (map str args)) {:dir (dir :gleam)})
    ;; Go has no registry push — a module is "published" by tagging the repo
    ;; (a `sdks/go` module path / vX.Y.Z tag) and letting the proxy fetch it.
    (throw (ex-info (str "publish: no publish step wired for " (name lang)
                         (when (= :go (->lang lang)) " (Go publishes by git tag)"))
                    {:lang lang}))))

(defn smoke-all
  "Run every SDK's GraphQL remote smoke in turn against a running daemon
  (env CVISOR_GRAPHQL_URL / CVISOR_TOKEN), stopping at the first failure.
  Cross-platform — the FFI runtime is never loaded. Returns whether all passed."
  []
  (every? zero?
          (map smoke [:python :ruby :node :erlang :elixir :gleam :clojure :go :rust :scala])))
