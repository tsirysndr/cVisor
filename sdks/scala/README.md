# cVisor — Scala SDK

A Scala 3 client for the [cVisor](../../README.md) sandbox runtime.

Two layers, same philosophy as the Go and Rust SDKs:

| Layer                             | Transport                                  | Platforms               |
| --------------------------------- | ------------------------------------------ | ----------------------- |
| `GraphQLClient` / `RemoteSandbox` | daemon GraphQL over HTTP (`java.net.http`) | any (macOS, Linux, ...) |
| `Sandbox`                         | in-process `libcvisor` via Java FFM        | Linux only              |

The GraphQL path is **daemon-first** and portable: it uses only the JDK's
`java.net.http.HttpClient` plus [uPickle/ujson](https://github.com/com-lihaoyi/upickle)
and works on macOS with no native library. The native `Sandbox` uses the
Foreign Function & Memory API (`java.lang.foreign`), so it needs **JDK 22+** and
runs **only on Linux**; on any other OS its constructor throws.

- Package: `dev.tsirysndr.cvisor`
- Build: **sbt**, Scala 3.3 LTS, targeting the JVM
- JDK: **22+** (pinned in `mise.toml` — run `mise install`) for the FFM API

Add `--enable-native-access=ALL-UNNAMED` to your JVM options to silence the FFM
restricted-method warning (the `run`/`test` tasks already do via `build.sbt`).

## Remote: talk to a daemon (any OS, incl. macOS)

```scala
import dev.tsirysndr.cvisor.{RemoteSandbox, RunOptions}

val sb = RemoteSandbox("http://127.0.0.1:8080/graphql", token)

val health = sb.health()
println(s"daemon ${health.version} ok=${health.ok}")

// One-shot run in an ephemeral sandbox.
val out = sb.run("echo hi")
print(out.stdout)                 // "hi\n"
println(out.exitCode)

// Persistent sandbox + file seed (base64 handled for you).
val info = sb.createSandbox()     // random name; binds this handle to it
sb.writeFile("/work/hello.txt", "hi\n")
print(sb.run("cat /work/hello.txt").stdout)
val data = sb.readFile("/work/hello.txt")   // Array[Byte]

// Per-run overrides.
sb.run("sleep 60", RunOptions(timeoutMs = 500))  // exit 137 (SIGKILL)
```

`RemoteSandbox` mirrors the daemon operations: `health`, `createSandbox`,
`listSandboxes`, `freeSandbox`, `configure`, `run`, `snapshot`, `rollback`,
`branch`, `fork`, `snapshots`, `deleteSnapshot`, `writeFile`, `readFile`,
`cacheSave`, `cacheRestore`, `cacheList`. Optionals use case classes with
defaults (`RunOptions`, `ConfigureOptions`, `Limits`). Non-empty GraphQL
`errors` raise `GraphQLError`. For raw access, drop to the client:

```scala
import dev.tsirysndr.cvisor.GraphQLClient

val c = new GraphQLClient("http://127.0.0.1:8080/graphql", token)
c.query("{ sandboxes { id name } }")
c.mutate("mutation($cmd:String!){ run(command:$cmd){ stdout } }", ujson.Obj("cmd" -> "uname -a"))
```

The daemon prints its bearer `token` on startup (or set `CVISOR_TOKEN`).

## Native: in-process sandbox (Linux only)

On Linux you can run the sandbox in-process, with no daemon:

```scala
import dev.tsirysndr.cvisor.Sandbox

val sb = Sandbox()                       // AutoCloseable
try
  sb.setAllowNetwork(false).setEnv("FOO", "bar")
  sb.writeFile("/work/x", "hello")
  print(sb.run("cat /work/x").stdout)    // "hello"
finally sb.close()
```

`Sandbox` (`setLogDebug`, `setAllowNetwork`, `setAllowListen`, `setEnv`,
`setLimits`, `writeFile`, `readFile`, `copyInto`, `copyOut`, `cacheSave`,
`cacheRestore`, `run`, `run(cmd, timeoutMs)`) is **Linux-only**. Its constructor
throws `UnsupportedOperationException` on macOS/Windows — use `RemoteSandbox`
there. The native library is loaded **lazily** on first construction, so
importing or using the GraphQL path never touches `libcvisor`.

It loads `libcvisor` from `CVISOR_LIB`, or a bundled
`libcvisor-<arch>.so` under `src/main/resources/cvisor/native/`. Build it from
the repo root:

```bash
cargo xtask ffi                 # aarch64
cargo xtask ffi --arch x86_64   # x86_64
```

which drops the `.so` into each SDK's resources.

## Build & run

```bash
sbt compile
sbt "runMain dev.tsirysndr.cvisor.Smoke"   # smoke-tests the GraphQL path
```

`Smoke` reads `CVISOR_URL` / `CVISOR_TOKEN` from the environment.

## Quick try of the native path under Docker

The sandbox only runs on Linux; from any host, build the native library once
(`cargo xtask ffi` from the repo root) and run inside a musl JDK 22 container:

```bash
docker run --rm --security-opt seccomp=unconfined \
  -v "$PWD":/sdk -w /sdk bellsoft/liberica-openjdk-alpine-musl:22 \
  sh -c 'apk add --no-cache bash >/dev/null && sbt compile'
```
