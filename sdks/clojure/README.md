# cVisor — Clojure SDK
[![Clojars Project](https://img.shields.io/clojars/v/io.github.tsirysndr/cvisor.svg)](https://clojars.org/io.github.tsirysndr/cvisor)


Java FFM (`java.lang.foreign`) bindings over the `libcvisor` C ABI. Linux-only
at runtime; requires **JDK 22+** (the FFM API is final as of JDK 22). The
pinned toolchain is in `mise.toml` — run `mise install` to get it.

Published on Clojars as `io.github.tsirysndr/cvisor`.

## Usage

```clojure
(require '[cvisor.core :as cvisor])

(with-open [sb (cvisor/sandbox)]
  (let [out (cvisor/run sb "echo hello")]
    (print (:stdout out))     ; "hello\n"
    (print (:stderr out))))   ; ""
```

`run` blocks until the sandboxed command exits and returns a map with
`:stdout` / `:stderr` (String), `:exit-code` (long — shell convention: status,
or 128 + signal), and `:stdout-bytes` / `:stderr-bytes`. `sandbox` returns a
`Closeable`, so `with-open` frees it; `close` is also exposed directly and is
idempotent.

```clojure
;; Exit codes:
(:exit-code (cvisor/run sb "exit 3"))          ; => 3

;; Timeout — SIGKILL the guest after N ms; a timed-out run reports 137:
(:exit-code (cvisor/run sb "sleep 60" {:timeout-ms 500}))   ; => 137

;; Egress kill switch — deny outbound INET/INET6 sockets (default allowed):
(cvisor/set-allow-network! sb false)

;; Inbound TCP servers (bind fixed port, listen) — off by default:
(cvisor/set-allow-listen! sb true)
```

## Files

Transfer files in and out of the sandbox overlay without running a command; a
file written this way is visible to later `run`s of the same sandbox:

```clojure
(cvisor/write-file sb "/app/config.json" "{\"k\":1}")
(cvisor/run sb "cat /app/config.json")
(String. (cvisor/read-file sb "/tmp/result.txt") "UTF-8")

;; Recursive directory copy (respects .gitignore / .dockerignore):
(cvisor/copy-into sb "./src" "/app")
(cvisor/copy-out sb "/app/dist" "./dist")
```

## Cache

Back up and restore a sandbox directory, keyed — for build caches, deps, etc.
The bundled library uses the host disk with gzip/estargz/none; the S3 backend
and zstd need a library built with those features.

```clojure
(cvisor/cache-save sb "/app/node_modules" "deps-v1")
;; ...later, in another sandbox — exact key or newest matching prefix:
(cvisor/cache-restore sb "/app/node_modules" "deps-v1")
;; options: {:backend "s3://bucket/prefix" :format "estargz"}
```

Add `--enable-native-access=ALL-UNNAMED` to your JVM options to silence the
FFM restricted-method warning (the `:test` and `:console` aliases already do).

## Streaming output

`run-streaming` runs the command in the background and calls `:on-stdout` /
`:on-stderr` with String chunks as output arrives, then returns the exit code:

```clojure
(cvisor/run-streaming
  sb "for i in 1 2 3; do echo $i; sleep 0.2; done"
  {:on-stdout (fn [s] (print s) (flush))})   ; => 0
```

## Interactive PTY shell

`shell` starts an interactive `/bin/sh` on a pseudo-terminal in the background.
Feed it with `write!`, observe the merged terminal output via `:on-output`,
resize with `resize!`, and free it with `close`:

```clojure
(let [sh (cvisor/shell sb {:on-output (fn [s] (print s) (flush))})]
  (cvisor/write! sh "echo hello; uname -n\n")
  (cvisor/resize! sh 40 120)
  (cvisor/write! sh "exit\n")
  (cvisor/wait sh)       ; block -> exit code
  (.close sh))
```

`exit-code` returns the code once finished (nil while running), `wait` blocks
for it, and `kill!` SIGKILLs the guest.

## Interactive console

Launch a rebel-readline REPL with a live sandbox preloaded:

```bash
clojure -M:console
```

```
cVisor interactive console
  sb                    -> a live Sandbox
  (sh "cmd")            -> run a shell command in the sandbox, printing stdout/stderr
  (cvisor.core/sandbox) -> create your own

user=> (sh "echo hello; uname -n")
hello
cvisor
```

### Quick try under Docker

The sandbox only runs on Linux; from any host, build the native library once
(`cargo xtask ffi` from the repo root) and drop into the console in a musl
JDK 22 container (multi-arch; installs the Clojure CLI on first run):

```bash
docker run -it --rm --security-opt seccomp=unconfined \
  -v "$PWD":/sdk -w /sdk bellsoft/liberica-openjdk-alpine-musl:22 \
  sh -c 'apk add --no-cache bash curl >/dev/null &&
         curl -sLO https://github.com/clojure/brew-install/releases/latest/download/linux-install.sh &&
         bash linux-install.sh >/dev/null && clojure -M:console'
```

## Development

The SDK loads `libcvisor.so`. Build it from the repo root (`cargo xtask ffi`),
which drops a copy into `resources/cvisor/native/`, or point the SDK at one
via the `CVISOR_LIB` environment variable.

Run the e2e test in a musl JDK container (Linux syscalls + seccomp required):

```bash
docker run --rm --security-opt seccomp=unconfined \
  -v "$PWD":/sdk -w /sdk bellsoft/liberica-openjdk-alpine-musl:22 \
  sh -c 'apk add --no-cache bash curl >/dev/null &&
         curl -sLO https://github.com/clojure/brew-install/releases/latest/download/linux-install.sh &&
         bash linux-install.sh >/dev/null && clojure -M:test'
```

## Publishing

```bash
cargo xtask ffi && cargo xtask ffi --arch x86_64   # from the repo root: both arches
clojure -T:build jar                               # bundles both .so files
CLOJARS_USERNAME=... CLOJARS_PASSWORD=... clojure -T:build deploy
```
