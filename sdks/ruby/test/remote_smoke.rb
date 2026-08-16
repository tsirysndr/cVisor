# Remote-GraphQL e2e smoke for the Ruby SDK's pure-stdlib client
# (Cvisor::RemoteSandbox over net/http — no libcvisor). Point it at a running
# cvisord:
#   CVISOR_GRAPHQL_URL=http://127.0.0.1:8080/graphql CVISOR_TOKEN=... ruby test/remote_smoke.rb
$LOAD_PATH.unshift File.expand_path("../lib", __dir__)
require "cvisor/remote"

def assert(cond, msg)
  raise "assertion failed: #{msg}" unless cond
end

url = ENV.fetch("CVISOR_GRAPHQL_URL", "http://127.0.0.1:8080/graphql")
token = ENV.fetch("CVISOR_TOKEN", "")

remote = Cvisor::RemoteSandbox.new(url, token)

health = remote.health
assert(health["ok"] == true, "health not ok: #{health.inspect}")

out = remote.run("echo hello")
assert(out["stdout"] == "hello\n", "run stdout: #{out.inspect}")
assert(out["exitCode"] == 0, "run exit code: #{out.inspect}")

sb = remote.create_sandbox
assert(!sb["id"].to_s.empty?, "create_sandbox returned no id: #{sb.inspect}")
remote.write_file(sb["id"], "/tmp/data.txt", "round-trip\n")
data = remote.read_file(sb["id"], "/tmp/data.txt")
assert(data == "round-trip\n", "read_file round-trip: #{data.inspect}")
remote.free_sandbox(sb["id"])

puts "RUBY_GRAPHQL_OK"
