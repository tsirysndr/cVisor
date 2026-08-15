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
    extern "void cvisor_sandbox_set_allow_network(void*, int)"
    extern "void cvisor_sandbox_set_allow_listen(void*, int)"
    extern "void cvisor_sandbox_set_env(void*, const char*, const char*)"
    extern "int cvisor_sandbox_write_file(void*, const char*, const char*, size_t)"
    extern "void* cvisor_sandbox_read_file(void*, const char*, void*)"
    extern "void* cvisor_run(void*, const char*)"
    extern "void* cvisor_run_timeout(void*, const char*, unsigned long long)"
    extern "void cvisor_output_free(void*)"
    extern "int cvisor_output_exit_code(void*)"
    extern "void* cvisor_output_stdout(void*, void*)"
    extern "void* cvisor_output_stderr(void*, void*)"
    extern "void cvisor_bytes_free(void*, size_t)"

    extern "int cvisor_sandbox_copy_into(void*, const char*, const char*)"
    extern "int cvisor_sandbox_copy_out(void*, const char*, const char*)"
    extern "int cvisor_cache_save(void*, const char*, const char*, const char*, const char*)"
    extern "int cvisor_cache_restore(void*, const char*, const char*, const char*, const char*)"

    extern "void* cvisor_session_start(void*, const char*, int)"
    extern "void* cvisor_session_read_stdout(void*, void*)"
    extern "void* cvisor_session_read_stderr(void*, void*)"
    extern "long cvisor_session_write_stdin(void*, const char*, size_t)"
    extern "void cvisor_session_resize(void*, unsigned short, unsigned short)"
    extern "int cvisor_session_try_wait(void*, void*)"
    extern "int cvisor_session_wait(void*)"
    extern "void cvisor_session_kill(void*)"
    extern "void cvisor_session_free(void*)"
  end

  # Drain a length-prefixed byte buffer from a native accessor and free it.
  # `accessor` is a Native method taking (handle, len_ptr) and returning a
  # Fiddle::Pointer; the count is written into `len_ptr`.
  def self.read_bytes(handle, accessor)
    len = Fiddle::Pointer.malloc(Fiddle::SIZEOF_SIZE_T, Fiddle::RUBY_FREE)
    ptr = Native.public_send(accessor, handle, len)
    n = len[0, Fiddle::SIZEOF_SIZE_T].unpack1(Fiddle::SIZEOF_SIZE_T == 8 ? "Q" : "L")
    return "".b if ptr.null? || n.zero?

    begin
      ptr[0, n].b
    ensure
      Native.cvisor_bytes_free(ptr, n)
    end
  end

  # Captured output of one run.
  class Output
    attr_reader :stdout_bytes, :stderr_bytes, :exit_code

    def initialize(stdout_bytes, stderr_bytes, exit_code)
      @stdout_bytes = stdout_bytes
      @stderr_bytes = stderr_bytes
      @exit_code = exit_code
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

    def set_allow_network(allow)
      Native.cvisor_sandbox_set_allow_network(@ptr, allow ? 1 : 0)
    end

    # Allow (or deny) inbound TCP servers inside the sandbox (denied by default).
    def set_allow_listen(allow)
      Native.cvisor_sandbox_set_allow_listen(@ptr, allow ? 1 : 0)
    end

    # Set an environment variable for the guest (applies to subsequent runs).
    def set_env(key, value)
      Native.cvisor_sandbox_set_env(@ptr, key.to_s, value.to_s)
    end

    # Seed a file into the sandbox's persistent overlay at an absolute `path`;
    # visible to later #run calls of this same Sandbox. Raises on error.
    def write_file(path, data)
      bytes = data.b
      rc = Native.cvisor_sandbox_write_file(@ptr, path, bytes, bytes.bytesize)
      raise "write_file failed (errno #{-rc})" if rc != 0
    end

    # Read the guest's view of an absolute `path` as a binary String
    # (overlay copy if present, else the real host file for cow paths).
    def read_file(path)
      len = Fiddle::Pointer.malloc(Fiddle::SIZEOF_SIZE_T, Fiddle::RUBY_FREE)
      ptr = Native.cvisor_sandbox_read_file(@ptr, path, len)
      n = len[0, Fiddle::SIZEOF_SIZE_T].unpack1(Fiddle::SIZEOF_SIZE_T == 8 ? "Q" : "L")
      return "".b if ptr.null? || n.zero?

      begin
        ptr[0, n].b
      ensure
        Native.cvisor_bytes_free(ptr, n)
      end
    end

    # Copy a host file or directory tree into the sandbox overlay at
    # `guest_path`; visible to later #run calls. Directory copies are recursive
    # and honor .gitignore/.dockerignore. Raises on error.
    def copy_into(host_path, guest_path)
      rc = Native.cvisor_sandbox_copy_into(@ptr, host_path, guest_path)
      raise "copy_into failed (errno #{-rc})" if rc != 0
    end

    # Copy the guest's view of `guest_path` (file or directory) out to
    # `host_path` on the host filesystem. Raises on error.
    def copy_out(guest_path, host_path)
      rc = Native.cvisor_sandbox_copy_out(@ptr, guest_path, host_path)
      raise "copy_out failed (errno #{-rc})" if rc != 0
    end

    # Archive the sandbox directory `sandbox_path` under `key` in a cache
    # backend. `backend`: "" or "disk" (default), "disk:/path", or an
    # "s3://bucket/prefix?..." URL (S3 requires the lib built with the s3
    # feature). `format`: "gzip" (default), "estargz", "none", or "zstd"
    # (zstd requires the lib built with that feature). Respects
    # .gitignore/.dockerignore. Raises on error.
    def cache_save(sandbox_path, key, backend: "", format: "gzip")
      rc = Native.cvisor_cache_save(@ptr, sandbox_path, key, backend, format)
      raise "cache_save failed (errno #{-rc})" if rc != 0
    end

    # Restore a cached archive stored under `key` into the sandbox overlay at
    # `sandbox_path`. See #cache_save for `backend`/`format`. Raises on error.
    def cache_restore(sandbox_path, key, backend: "", format: "gzip")
      rc = Native.cvisor_cache_restore(@ptr, sandbox_path, key, backend, format)
      raise "cache_restore failed (errno #{-rc})" if rc != 0
    end

    def run(command, timeout_ms: nil)
      out = if timeout_ms.is_a?(Integer) && timeout_ms.positive?
              Native.cvisor_run_timeout(@ptr, command, timeout_ms)
            else
              Native.cvisor_run(@ptr, command)
            end
      raise "sandbox run failed" if out.null?
      begin
        Output.new(read_output(out, :cvisor_output_stdout),
                   read_output(out, :cvisor_output_stderr),
                   Native.cvisor_output_exit_code(out))
      ensure
        Native.cvisor_output_free(out)
      end
    end

    def close
      return unless @ptr && !@ptr.null?
      Native.cvisor_sandbox_free(@ptr)
      @ptr = nil
    end

    # Start a non-PTY streaming session, draining stdout/stderr into the
    # callbacks (UTF-8 Strings) until the command exits. Returns the exit code.
    def run_streaming(command, on_stdout: nil, on_stderr: nil, poll_ms: 15)
      ptr = Native.cvisor_session_start(@ptr, command, 0)
      raise "sandbox session failed" if ptr.null?
      session = Session.new(ptr)
      begin
        loop do
          emit(session.read_stdout, on_stdout)
          emit(session.read_stderr, on_stderr)
          code = session.exit_code
          if code
            emit(session.read_stdout, on_stdout)
            emit(session.read_stderr, on_stderr)
            return code
          end
          sleep(poll_ms / 1000.0)
        end
      ensure
        session.close
      end
    end

    # Start an interactive PTY shell (/bin/sh -i). If `on_output` is given, a
    # background thread drains merged stdout and yields UTF-8 Strings to it
    # until the shell exits. Returns the Session (caller must #close it).
    def shell(on_output: nil, poll_ms: 15)
      ptr = Native.cvisor_session_start(@ptr, nil, 1)
      raise "sandbox shell failed" if ptr.null?
      session = Session.new(ptr)
      if on_output
        Thread.new do
          loop do
            emit(session.read_stdout, on_output)
            if session.exit_code
              emit(session.read_stdout, on_output)
              break
            end
            sleep(poll_ms / 1000.0)
          end
        end
      end
      session
    end

    private

    def emit(str, callback)
      callback.call(str.force_encoding("UTF-8")) if callback && !str.empty?
    end

    def read_output(out, accessor)
      Cvisor.read_bytes(out, accessor)
    end
  end

  # A streaming session over a sandboxed process. Wraps the opaque
  # CvisorSession* pointer. PTY sessions merge stderr into stdout.
  class Session
    def initialize(ptr)
      @ptr = ptr
    end

    def read_stdout = Cvisor.read_bytes(@ptr, :cvisor_session_read_stdout)
    def read_stderr = Cvisor.read_bytes(@ptr, :cvisor_session_read_stderr)

    def write_stdin(data)
      bytes = data.b
      Native.cvisor_session_write_stdin(@ptr, bytes, bytes.bytesize)
    end

    def resize(rows, cols)
      Native.cvisor_session_resize(@ptr, rows, cols)
    end

    # Exit code if the process has finished, else nil.
    def exit_code
      done = Fiddle::Pointer.malloc(Fiddle::SIZEOF_INT, Fiddle::RUBY_FREE)
      code = Native.cvisor_session_try_wait(@ptr, done)
      finished = done[0, Fiddle::SIZEOF_INT].unpack1("l")
      finished.zero? ? nil : code
    end

    def wait = Native.cvisor_session_wait(@ptr)

    def kill
      Native.cvisor_session_kill(@ptr)
    end

    def close
      return unless @ptr && !@ptr.null?
      Native.cvisor_session_free(@ptr)
      @ptr = nil
    end
  end
end
