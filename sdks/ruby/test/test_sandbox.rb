# e2e test for the cVisor Ruby SDK. Run in a musl ruby container with
# CVISOR_LIB pointing at libcvisor.so, under seccomp=unconfined:
#   ruby test/test_sandbox.rb
$LOAD_PATH.unshift File.expand_path("../lib", __dir__)
require "cvisor"

def assert_eq(actual, expected, msg)
  raise "#{msg}: expected #{expected.inspect}, got #{actual.inspect}" unless actual == expected
end

sb = Cvisor::Sandbox.new
assert_eq(sb.run("echo hello from ruby").stdout, "hello from ruby\n", "echo")
assert_eq(sb.run("printf 'a\nb\nc\n' | grep b").stdout, "b\n", "pipeline")
assert_eq(sb.run("echo x > /tmp/f && grep x /tmp/f").stdout, "x\n", "tmp redirect")
assert_eq(sb.run("uname -n").stdout, "cvisor\n", "uname virtualized")
assert_eq(sb.run("grep Name /proc/self/status").stdout, "Name:\tcvisor-guest\n", "proc virtualized")
assert_eq(sb.run("exit 7").exit_code, 7, "exit code")
assert_eq(sb.run("false").exit_code, 1, "exit code false")
assert_eq(sb.run("true").exit_code, 0, "exit code true")
assert_eq(sb.run("echo hi > /tmp/a.part && mv /tmp/a.part /tmp/a && grep hi /tmp/a").stdout, "hi\n", "atomic rename")
assert_eq(sb.run("sleep 30", timeout_ms: 300).exit_code, 137, "timeout kill")

sb.write_file("/tmp/data.txt", "seeded\n")
assert_eq(sb.run("grep seeded /tmp/data.txt").stdout, "seeded\n", "write_file seeds overlay")
sb.run("echo from-run > /tmp/out.txt")
assert_eq(sb.read_file("/tmp/out.txt"), "from-run\n", "read_file sees run output")
sb.set_allow_listen(true) # just confirm it doesn't raise

# Cache round-trip: save a sandbox directory under a key, then restore it into
# a fresh sandbox's overlay and confirm the files are visible to a run.
sb.write_file("/tmp/proj/a.txt", "alpha\n")
sb.write_file("/tmp/proj/b.txt", "beta\n")
sb.cache_save("/tmp/proj", "k1")
sb2 = Cvisor::Sandbox.new
sb2.cache_restore("/tmp/proj", "k1")
# grep (read-based) rather than cat (zero-copy sendfile bypasses capture).
assert_eq(sb2.run("grep -h . /tmp/proj/a.txt /tmp/proj/b.txt").stdout, "alpha\nbeta\n", "cache round-trip")

chunks = []
code = sb.run_streaming("for i in 1 2 3; do echo line$i; sleep 0.1; done",
                        on_stdout: ->(s) { chunks << s })
assert_eq(code, 0, "streaming exit code")
assert_eq(chunks.join, "line1\nline2\nline3\n", "streaming stdout")

buf = []
sh = sb.shell(on_output: ->(s) { buf << s })
sh.write_stdin("echo SHELL_OK\n")
sh.write_stdin("test -t 1 && echo IS_TTY\n")
sh.write_stdin("exit 4\n")
assert_eq(sh.wait, 4, "shell exit code")
sleep 0.1
merged = buf.join
raise "shell missing SHELL_OK: #{merged.inspect}" unless merged.include?("SHELL_OK")
raise "shell missing IS_TTY: #{merged.inspect}" unless merged.include?("IS_TTY")
sh.close


sbe = Cvisor::Sandbox.new
sbe.set_env("FOO", "bar")
sbe.set_env("GREETING", "hi there")
assert_eq(sbe.run("echo $FOO-$GREETING").stdout, "bar-hi there\n", "set_env")

puts "RUBY_SDK_OK"
