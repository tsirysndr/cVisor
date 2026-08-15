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
      # The NIF-backed Erlang runtime. Published on Hex as `cvisor`; a path dep
      # in the monorepo. Compiles c_src/cvisor_nif.c and ships libcvisor.so.
      {:cvisor, path: "../erlang"},
      {:ex_doc, "~> 0.34", only: :dev, runtime: false}
    ]
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
