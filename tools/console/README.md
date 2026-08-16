# cVisor Console

A **single Clojure REPL (and `bb` task set) for every operational command in the
monorepo.** Instead of remembering whether a piece of work is a `cargo xtask`
invocation, a `bun run` script in `ui/`, a `docker build`, or one of ten
different SDK test runners under `sdks/*`, you sit in one REPL and call
functions:

```clojure
user=> (build/daemon)          ;; release-build cvisord
user=> (sdk/smoke :scala)      ;; GraphQL remote smoke against a running daemon
user=> (sdk/publish :scala)    ;; publishSigned + publish-central.sh
user=> (ui/dev)                ;; the web UI dev server
user=> (docker/build :ubuntu)  ;; one container image
```

Every command is a thin Clojure wrapper around the existing tool (`cargo`,
`bun`, `docker`, `uv`, `gem`, `mix`, `gleam`, `clojure`, `sbt`, ...). Nothing in
the underlying scripts changes — the console is a **discoverable, composable
front door**.

**Every `sdk/*` command takes the language as a plain keyword** — an *atom*, not
a call per language — so `(sdk/test :ruby)`, `(sdk/build :clojure)`,
`(sdk/smoke :python)` are all the same shape. A string works too
(`(sdk/test "ruby")`), which is how `bb`'s CLI args arrive.

## Run it

Toolchain is pinned in `.mise.toml` (babashka, clojure, a JDK ≥ 22):

```bash
cd tools/console && mise install
```

Two front doors:

```bash
./console          # from the repo root: the rebel REPL (clojure -M:rebel)
bb <task>          # one-shot: bb help, bb daemon, bb sdk:smoke scala
bb tasks           # list every bb task
```

In the REPL, `(help)` / `(ls)` list every command; the namespaces are in scope as
`build`, `sdk`, `ui`, `docker` (and `sh` for ad-hoc shell-outs).

## Command groups

| Group     | Namespace        | What it wraps                                              |
| --------- | ---------------- | --------------------------------------------------------- |
| `build`   | `console.build`  | `cargo` + `cargo xtask` (fmt/clippy/test, cli, daemon, ffi, run-node, …) |
| `sdk`     | `console.sdk`    | the ten SDKs: `deps`/`test`/`smoke`/`lint`/`build`/`install`/`publish` |
| `ui`      | `console.ui`     | the web SPA + Tauri desktop shell (`bun run …`)           |
| `docker`  | `console.docker` | the alpine/trixie/ubuntu images (`docker build`)          |

## SDK commands: two test flavors

- `(sdk/test <lang>)` — the **native FFI** suite ci.yml runs. Needs `libcvisor`
  (run `(build/ffi)` first) and a **Linux** kernel with `seccomp=unconfined`.
- `(sdk/smoke <lang>)` — the **pure-HTTP GraphQL** client smoke against a running
  daemon (`CVISOR_GRAPHQL_URL` / `CVISOR_TOKEN`). Runs on **any host**, macOS
  included. `(sdk/smoke-all)` runs them all.

## Publishing

`(sdk/publish <lang>)` pushes to the language's registry — npm, PyPI, RubyGems,
Clojars, crates.io, Hex, and **Maven Central** for Scala (`sbt publishSigned`
then `sdks/scala/publish-central.sh`, which needs a Central Portal user token in
`CENTRAL_TOKEN_USERNAME`/`CENTRAL_TOKEN_PASSWORD`). Each publisher expects its
own credentials already configured — the console manages none of them. Go has no
registry push: a module is published by tagging the repo.
