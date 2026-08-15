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
    extern "void* cvisor_run(void*, const char*)"
    extern "void* cvisor_run_timeout(void*, const char*, unsigned long long)"
    extern "void cvisor_output_free(void*)"
    extern "int cvisor_output_exit_code(void*)"
    extern "void* cvisor_output_stdout(void*, void*)"
    extern "void* cvisor_output_stderr(void*, void*)"
    extern "void cvisor_bytes_free(void*, size_t)"

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
