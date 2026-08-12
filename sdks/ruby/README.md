# cVisor — Ruby SDK

A `fiddle` FFI wrapper over the `libcvisor` C ABI. Linux-only.

## Usage

```ruby
require "cvisor"

sb = Cvisor::Sandbox.new
out = sb.run("echo hello")
puts out.stdout   # "hello\n"
puts out.stderr   # ""
```

`Sandbox#run(cmd)` blocks until the sandboxed command exits and returns an
`Output` with `#stdout` / `#stderr` (String) and `#stdout_bytes` /
`#stderr_bytes`.

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
