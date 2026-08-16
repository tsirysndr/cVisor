# Optional GraphQL client for the cVisor daemon's HTTP API.
#
# Pure stdlib (net/http, uri, json) — no native library — so it runs on any
# platform (macOS included), unlike the Fiddle-backed Cvisor::Sandbox.
#
#   gql = Cvisor::GraphQL.new("http://127.0.0.1:8080/graphql", token)
#   data = gql.query("{ health { version ok } }")
#   data["health"]   # => {"version"=>"0.1.0", "ok"=>true}

require "json"
require "net/http"
require "uri"

module Cvisor
  # A thin client for the cVisor daemon's GraphQL endpoint.
  class GraphQL
    # Raised when the daemon returns a non-empty `errors` array.
    class Error < StandardError; end

    def initialize(url, token)
      @uri = URI(url)
      @token = token
    end

    # Run a GraphQL query document; returns the `data` Hash.
    def query(document, variables = {})
      execute(document, variables)
    end

    # Run a GraphQL mutation document; returns the `data` Hash.
    def mutate(document, variables = {})
      execute(document, variables)
    end

    private

    def execute(document, variables)
      req = Net::HTTP::Post.new(@uri)
      req["Content-Type"] = "application/json"
      req["Authorization"] = "Bearer #{@token}"
      req.body = JSON.generate(query: document, variables: variables || {})

      res = Net::HTTP.start(@uri.hostname, @uri.port, use_ssl: @uri.scheme == "https") do |http|
        http.request(req)
      end
      raise Error, "GraphQL request failed: HTTP #{res.code} #{res.message}" unless res.is_a?(Net::HTTPSuccess)

      body = JSON.parse(res.body)
      errors = body["errors"]
      raise Error, errors.map { |e| e["message"] }.join("; ") if errors && !errors.empty?

      body["data"] || {}
    end
  end
end
