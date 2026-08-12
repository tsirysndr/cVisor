# cVisor Ruby SDK — Fiddle FFI wrapper over the libcvisor C ABI. Linux-only.
#
#   require "cvisor"
#   out = Cvisor::Sandbox.new.run("echo hello")
#   puts out.stdout   # "hello\n"

require "fiddle"
require "fiddle/import"

module Cvisor
  # Low-level binding to libcvisor via Fiddle.
  module Native
    extend Fiddle::Importer

    def self.library_path
      return ENV["CVISOR_LIB"] if ENV["CVISOR_LIB"]
      arch = RbConfig::CONFIG["host_cpu"] =~ /(aarch64|arm64)/ ? "aarch64" : "x86_64"
      here = File.expand_path("../native", __dir__)
      [
        File.join(here, "libcvisor-#{arch}.so"),
        File.join(here, "libcvisor.so"),
      ].find { |p| File.exist?(p) } || File.join(here, "libcvisor-#{arch}.so")
    end

    dlload library_path

    extern "void* cvisor_sandbox_new(void)"
    extern "void cvisor_sandbox_free(void*)"
    extern "void cvisor_sandbox_set_log_level(void*, int)"
    extern "void* cvisor_run(void*, const char*)"
    extern "void cvisor_output_free(void*)"
    extern "void* cvisor_output_stdout(void*, void*)"
    extern "void* cvisor_output_stderr(void*, void*)"
    extern "void cvisor_bytes_free(void*, size_t)"
  end

  # Captured output of one run.
  class Output
    attr_reader :stdout_bytes, :stderr_bytes

    def initialize(stdout_bytes, stderr_bytes)
      @stdout_bytes = stdout_bytes
      @stderr_bytes = stderr_bytes
    end

    def stdout = @stdout_bytes.force_encoding("UTF-8")
    def stderr = @stderr_bytes.force_encoding("UTF-8")
  end

  class Sandbox
    def initialize
      @ptr = Native.cvisor_sandbox_new
      raise "failed to create sandbox" if @ptr.null?
    end

    def set_log_level(level)
      Native.cvisor_sandbox_set_log_level(@ptr, level == "DEBUG" ? 1 : 0)
    end

    def run(command)
      out = Native.cvisor_run(@ptr, command)
      raise "sandbox run failed" if out.null?
      begin
        Output.new(read_output(out, :cvisor_output_stdout),
                   read_output(out, :cvisor_output_stderr))
      ensure
        Native.cvisor_output_free(out)
      end
    end

    def close
      return unless @ptr && !@ptr.null?
      Native.cvisor_sandbox_free(@ptr)
      @ptr = nil
    end

    private

    def read_output(out, accessor)
      len = Fiddle::Pointer.malloc(Fiddle::SIZEOF_SIZE_T, Fiddle::RUBY_FREE)
      ptr = Native.public_send(accessor, out, len)
      n = len[0, Fiddle::SIZEOF_SIZE_T].unpack1(Fiddle::SIZEOF_SIZE_T == 8 ? "Q" : "L")
      return "".b if ptr.null? || n.zero?

      begin
        ptr[0, n].b
      ensure
        Native.cvisor_bytes_free(ptr, n)
      end
    end
  end
end
