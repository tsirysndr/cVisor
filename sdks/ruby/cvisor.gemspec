Gem::Specification.new do |spec|
  spec.name        = "cvisor"
  spec.version     = "0.1.0"
  spec.summary     = "In-process Linux sandbox — Ruby SDK (Fiddle FFI over libcvisor)"
  spec.description = "A thin Fiddle FFI wrapper over the libcvisor C ABI. Linux-only."
  spec.authors     = ["butter.dev"]
  spec.homepage    = "https://github.com/tsirysndr/cVisor"
  spec.license     = "MIT"
  spec.metadata    = {
    "homepage_uri"    => "https://github.com/tsirysndr/cVisor",
    "source_code_uri" => "https://github.com/tsirysndr/cVisor",
  }

  spec.required_ruby_version = ">= 3.0"
  spec.files = Dir["lib/**/*.rb", "bin/*", "native/*.so", "README.md"]
  spec.require_paths = ["lib"]
  # The interactive console: `cvisor-console`.
  spec.bindir = "bin"
  spec.executables = ["console"]
  # fiddle and irb are part of the standard library.
  spec.platform = Gem::Platform.new("aarch64-linux-musl")
end
