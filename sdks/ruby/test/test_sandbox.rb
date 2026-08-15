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

puts "RUBY_SDK_OK"
