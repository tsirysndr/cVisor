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

puts "RUBY_SDK_OK"
