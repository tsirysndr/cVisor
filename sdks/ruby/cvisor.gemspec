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

  # One platform gem per CPU, each bundling only its own libcvisor build
  # (a musl .so from `cargo xtask ffi --arch <cpu>`). Select the target with
  # CVISOR_ARCH=aarch64|x86_64 (defaults to the host CPU):
  #   CVISOR_ARCH=x86_64 gem build cvisor.gemspec
  cpu = ENV.fetch("CVISOR_ARCH") do
    RbConfig::CONFIG["host_cpu"] =~ /(aarch64|arm64)/ ? "aarch64" : "x86_64"
  end
  spec.platform = Gem::Platform.new("#{cpu}-linux-musl")
  spec.files = Dir["lib/**/*.rb", "bin/*", "README.md"] + ["native/libcvisor-#{cpu}.so"]
  spec.require_paths = ["lib"]
  # The interactive console: `cvisor-console`.
  spec.bindir = "bin"
  spec.executables = ["console"]
  # fiddle and irb are part of the standard library.
end
