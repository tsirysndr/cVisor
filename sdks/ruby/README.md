# cVisor — Ruby SDK

A `fiddle` FFI wrapper over the `libcvisor` C ABI. Linux-only.

## Usage

```ruby
require "cvisor"

sb = Cvisor::Sandbox.new
out = sb.run("echo hello")
puts out.stdout    # "hello\n"
puts out.stderr    # ""
puts out.exit_code # 0
```

`Sandbox#run(cmd, timeout_ms: nil)` blocks until the sandboxed command exits and
returns an `Output` with `#stdout` / `#stderr` (String), `#stdout_bytes` /
`#stderr_bytes`, and `#exit_code` (Integer, shell convention: status, or
128+signo when killed by a signal).

Pass a positive `timeout_ms:` to SIGKILL the guest after that many
milliseconds; a timed-out run reports exit code 137:

```ruby
sb.run("sleep 30", timeout_ms: 300).exit_code  # 137
```

`Sandbox#set_allow_network(allow)` toggles outbound INET/INET6 networking for
the sandbox (allowed by default):

```ruby
sb.set_allow_network(false)  # deny outbound networking
```

## Interactive console

Launch an IRB session with a live sandbox preloaded:

```bash
bin/console
```

```
cVisor interactive console
  sb          -> a Sandbox instance
  sh("cmd")   -> run a shell command in the sandbox, printing stdout/stderr
  Cvisor::Sandbox.new -> create your own

irb(main):001> sh("echo hello; uname -n")
hello
cvisor
```

## Development

The SDK loads `libcvisor.so`. Build it from the repo root (`cargo xtask ffi`),
which drops a copy into `native/`, or point the SDK at one via the `CVISOR_LIB`
environment variable.
