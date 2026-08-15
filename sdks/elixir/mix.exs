defmodule Cvisor.MixProject do
  use Mix.Project

  @version "0.1.0"
  @source_url "https://github.com/tsirysndr/cVisor"

  def project do
    [
      app: :cvisor_ex,
      version: @version,
      elixir: "~> 1.15",
      start_permanent: Mix.env() == :prod,
      description:
        "Elixir SDK for cVisor, an in-process Linux sandbox. Pipe-friendly " <>
          "builder over the NIF-backed cvisor runtime.",
      package: package(),
      deps: deps(),
      docs: docs(),
      name: "cvisor_ex",
      source_url: @source_url
    ]
  end

  def application do
    [extra_applications: [:logger]]
  end

  defp deps do
    [
      cvisor_dep(),
      {:ex_doc, "~> 0.34", only: :dev, runtime: false}
    ]
  end

  # The NIF-backed Erlang runtime (`cvisor` on Hex). A published package can't
  # carry a path dep, so default to the Hex release; in the monorepo, set
  # CVISOR_PATH=../erlang to build/test against the local Erlang source.
  defp cvisor_dep do
    case System.get_env("CVISOR_PATH") do
      nil -> {:cvisor, "~> 0.3"}
      path -> {:cvisor, path: path}
    end
  end

  defp package do
    [
      licenses: ["MIT"],
      links: %{"GitHub" => @source_url},
      files: ~w(lib mix.exs README.md)
    ]
  end

  defp docs do
    [
      main: "readme",
      extras: ["README.md"],
      source_ref: "elixir-sdk-v#{@version}"
    ]
  end
end
