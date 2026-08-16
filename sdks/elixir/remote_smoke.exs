# Remote-GraphQL e2e smoke for the Elixir SDK's pure-OTP client
# (Cvisor.Remote over :httpc — no NIF). Loads just the two client modules (no
# mix project, no NIF build) and runs against a running cvisord:
#   CVISOR_GRAPHQL_URL=http://127.0.0.1:8080/graphql CVISOR_TOKEN=... elixir remote_smoke.exs
Code.require_file("lib/cvisor/graphql.ex", __DIR__)
Code.require_file("lib/cvisor/remote.ex", __DIR__)

url = System.get_env("CVISOR_GRAPHQL_URL") || "http://127.0.0.1:8080/graphql"
token = System.get_env("CVISOR_TOKEN") || ""

remote = Cvisor.Remote.connect(url, token)

{:ok, %{"ok" => true}} = Cvisor.Remote.health(remote)

{:ok, %{"stdout" => "hello\n", "exitCode" => 0}} = Cvisor.Remote.run(remote, "echo hello")

{:ok, %{"id" => id}} = Cvisor.Remote.create_sandbox(remote)
{:ok, true} = Cvisor.Remote.write_file(remote, id, "/tmp/data.txt", "round-trip\n")
{:ok, "round-trip\n"} = Cvisor.Remote.read_file(remote, id, "/tmp/data.txt")
{:ok, true} = Cvisor.Remote.free_sandbox(remote, id)

IO.puts("ELIXIR_GRAPHQL_OK")
