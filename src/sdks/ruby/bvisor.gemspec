Gem::Specification.new do |spec|
  spec.name        = "bvisor"
  spec.version     = "0.0.6"
  spec.summary     = "In-process Linux sandbox — Ruby SDK (Fiddle FFI over libbvisor)"
  spec.description = "A thin Fiddle FFI wrapper over the libbvisor C ABI. Linux-only."
  spec.authors     = ["butter.dev"]
  spec.homepage    = "https://github.com/butter-dot-dev/bVisor"
  spec.license     = "MIT"

  spec.required_ruby_version = ">= 3.0"
  spec.files = Dir["lib/**/*.rb", "native/*.so", "README.md"]
  spec.require_paths = ["lib"]
  # fiddle is part of the standard library.
  spec.platform = Gem::Platform.new("aarch64-linux-musl")
end
