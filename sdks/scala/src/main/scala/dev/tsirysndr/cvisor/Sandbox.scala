package dev.tsirysndr.cvisor

import java.io.File
import java.lang.foreign.*
import java.lang.invoke.MethodHandle
import java.nio.charset.StandardCharsets
import java.nio.file.{Files, StandardCopyOption}

/** The in-process native sandbox, backed by `libcvisor` over Java FFM
  * (`java.lang.foreign`). **Linux-only** at run time and requires **JDK 22+**.
  *
  * Constructing a [[Sandbox]] on a non-Linux host throws — use
  * [[RemoteSandbox]] / [[GraphQLClient]] to talk to a daemon instead. The native
  * library is loaded lazily on first construction, so the GraphQL path never
  * touches `libcvisor`.
  */
final class Sandbox private (private var ptr: MemorySegment) extends AutoCloseable:
  import Native.*

  private def live: MemorySegment =
    if ptr == null then throw new IllegalStateException("sandbox is closed") else ptr

  /** Enable (`true`) or disable native debug logging. */
  def setLogDebug(enabled: Boolean): this.type =
    invoke(setLogLevelH, live, Integer.valueOf(if enabled then 1 else 0)); this

  /** Enable/disable outbound INET/INET6 networking for the guest (default on). */
  def setAllowNetwork(allow: Boolean): this.type =
    invoke(setAllowNetworkH, live, Integer.valueOf(if allow then 1 else 0)); this

  /** Enable/disable inbound TCP servers (bind a fixed port, listen). Off by default. */
  def setAllowListen(allow: Boolean): this.type =
    invoke(setAllowListenH, live, Integer.valueOf(if allow then 1 else 0)); this

  /** Set a guest environment variable (layered over PATH/HOME). */
  def setEnv(key: String, value: String): this.type =
    val arena = Arena.ofConfined()
    try invoke(setEnvH, live, arena.allocateFrom(key), arena.allocateFrom(value))
    finally arena.close()
    this

  /** Cap guest resources via cgroup v2 (any zero value = no limit). */
  def setLimits(memoryMax: Long = 0, pidsMax: Long = 0, cpuPercent: Int = 0): this.type =
    invoke(setLimitsH, live, java.lang.Long.valueOf(memoryMax), java.lang.Long.valueOf(pidsMax), Integer.valueOf(cpuPercent))
    this

  /** Write bytes to `path` inside the sandbox overlay (visible to later runs). */
  def writeFile(path: String, data: Array[Byte]): Unit =
    val arena = Arena.ofConfined()
    try
      val cpath = arena.allocateFrom(path)
      val n = data.length
      val seg = arena.allocate(math.max(n, 1).toLong)
      if n > 0 then MemorySegment.copy(data, 0, seg, ValueLayout.JAVA_BYTE, 0L, n)
      val rc = invoke(writeFileH, live, cpath, seg, java.lang.Long.valueOf(n.toLong)).asInstanceOf[Integer].intValue
      if rc != 0 then throw new RuntimeException(s"write-file failed (errno ${-rc})")
    finally arena.close()

  /** Write a UTF-8 string to `path`. */
  def writeFile(path: String, data: String): Unit =
    writeFile(path, data.getBytes(StandardCharsets.UTF_8))

  /** Read `path` from the sandbox overlay and return its bytes. */
  def readFile(path: String): Array[Byte] =
    val arena = Arena.ofConfined()
    try
      val cpath = arena.allocateFrom(path)
      val lenSeg = arena.allocate(ValueLayout.JAVA_LONG)
      val out = invoke(readFileH, live, cpath, lenSeg).asInstanceOf[MemorySegment]
      val n = lenSeg.get(ValueLayout.JAVA_LONG, 0L)
      if isNull(out) || n == 0 then Array.emptyByteArray
      else
        try out.reinterpret(n).toArray(ValueLayout.JAVA_BYTE)
        finally invoke(bytesFreeH, out, java.lang.Long.valueOf(n))
    finally arena.close()

  /** Copy a host file or directory tree into the sandbox at `guestPath`. */
  def copyInto(hostPath: String, guestPath: String): Unit =
    checkRc("copy-into", copyIntoH, hostPath, guestPath)

  /** Copy a sandbox file or directory tree out to `hostPath`. */
  def copyOut(guestPath: String, hostPath: String): Unit =
    checkRc("copy-out", copyOutH, guestPath, hostPath)

  private def checkRc(op: String, h: MethodHandle, a: String, b: String): Unit =
    val arena = Arena.ofConfined()
    try
      val rc = invoke(h, live, arena.allocateFrom(a), arena.allocateFrom(b)).asInstanceOf[Integer].intValue
      if rc != 0 then throw new RuntimeException(s"$op failed (errno ${-rc})")
    finally arena.close()

  /** Archive the sandbox directory `path` under `key`. */
  def cacheSave(path: String, key: String, backend: String = "", format: String = "gzip"): Unit =
    cacheOp("cache-save", cacheSaveH, path, key, backend, format)

  /** Restore the archive `key` into the sandbox directory `path`. */
  def cacheRestore(path: String, key: String, backend: String = "", format: String = "gzip"): Unit =
    cacheOp("cache-restore", cacheRestoreH, path, key, backend, format)

  private def cacheOp(op: String, h: MethodHandle, path: String, key: String, backend: String, format: String): Unit =
    val arena = Arena.ofConfined()
    try
      val rc = invoke(
        h,
        live,
        arena.allocateFrom(path),
        arena.allocateFrom(key),
        arena.allocateFrom(backend),
        arena.allocateFrom(format),
      ).asInstanceOf[Integer].intValue
      if rc != 0 then throw new RuntimeException(s"$op failed (errno ${-rc})")
    finally arena.close()

  /** Run a shell command, blocking until it exits. */
  def run(command: String): Output = runImpl(command, None)

  /** Run a shell command, SIGKILLing the guest after `timeoutMs` (exit code 137). */
  def run(command: String, timeoutMs: Long): Output = runImpl(command, Some(timeoutMs))

  private def runImpl(command: String, timeoutMs: Option[Long]): Output =
    val arena = Arena.ofConfined()
    try
      val cmd = arena.allocateFrom(command)
      val out = timeoutMs match
        case Some(t) => invoke(runTimeoutH, live, cmd, java.lang.Long.valueOf(t)).asInstanceOf[MemorySegment]
        case None    => invoke(runH, live, cmd).asInstanceOf[MemorySegment]
      if isNull(out) then throw new RuntimeException("sandbox run failed")
      try
        val so = readOutput(arena, out, outputStdoutH)
        val se = readOutput(arena, out, outputStderrH)
        val code = invoke(outputExitCodeH, out).asInstanceOf[Integer].intValue
        Output(new String(so, StandardCharsets.UTF_8), new String(se, StandardCharsets.UTF_8), code)
      finally invoke(outputFreeH, out)
    finally arena.close()

  /** Free the sandbox. Idempotent; a `Sandbox` is an `AutoCloseable`. */
  override def close(): Unit =
    if ptr != null then
      val p = ptr
      ptr = null
      invoke(sandboxFreeH, p)

object Sandbox:
  /** Create a native sandbox. Throws on non-Linux hosts. */
  def apply(): Sandbox =
    if !Native.isLinux then
      throw new UnsupportedOperationException(
        "the local FFI sandbox is Linux-only; use GraphQLClient / RemoteSandbox to talk to a daemon",
      )
    val p = Native.invoke(Native.sandboxNewH).asInstanceOf[MemorySegment]
    if Native.isNull(p) then throw new RuntimeException("failed to create sandbox")
    new Sandbox(p)

/** Lazy FFM bindings over the `libcvisor` C ABI. Nothing here loads the native
  * library until a [[Sandbox]] is actually constructed.
  */
private object Native:
  def isLinux: Boolean = System.getProperty("os.name", "").toLowerCase.contains("linux")

  private def hostArch: String =
    val a = System.getProperty("os.arch", "")
    if a.matches("(?i).*(aarch64|arm64).*") then "aarch64" else "x86_64"

  private def extractResource(path: String): Option[String] =
    Option(getClass.getResourceAsStream("/" + path)).map { in =>
      try
        val tmp = File.createTempFile("libcvisor-", ".so")
        tmp.deleteOnExit()
        Files.copy(in, tmp.toPath, StandardCopyOption.REPLACE_EXISTING)
        tmp.getAbsolutePath
      finally in.close()
    }

  private def libraryPath: String =
    Option(System.getenv("CVISOR_LIB"))
      .orElse(extractResource(s"cvisor/native/libcvisor-$hostArch.so"))
      .getOrElse(
        throw new RuntimeException(
          s"libcvisor-$hostArch.so not found on the classpath; set CVISOR_LIB or build it with `cargo xtask ffi`",
        ),
      )

  private lazy val linker: Linker = Linker.nativeLinker()
  private lazy val lookup: SymbolLookup = SymbolLookup.libraryLookup(libraryPath, Arena.global())

  private val PTR: MemoryLayout = ValueLayout.ADDRESS
  private val I32: MemoryLayout = ValueLayout.JAVA_INT
  private val I64: MemoryLayout = ValueLayout.JAVA_LONG

  private def fd(ret: MemoryLayout, args: MemoryLayout*): FunctionDescriptor =
    FunctionDescriptor.of(ret, args*)
  private def fdVoid(args: MemoryLayout*): FunctionDescriptor =
    FunctionDescriptor.ofVoid(args*)

  private def bind(name: String, desc: FunctionDescriptor): MethodHandle =
    linker.downcallHandle(lookup.find(name).orElseThrow(), desc)

  /** Invoke a bound downcall handle; numeric args must be boxed by the caller. */
  def invoke(h: MethodHandle, args: AnyRef*): AnyRef = h.invokeWithArguments(args*)

  def isNull(seg: MemorySegment): Boolean = seg == null || seg.address() == 0L

  /** Drain a byte buffer returned by an `*_stdout`/`*_stderr` accessor. */
  def readOutput(arena: Arena, out: MemorySegment, accessor: MethodHandle): Array[Byte] =
    val lenSeg = arena.allocate(ValueLayout.JAVA_LONG)
    val ptr = invoke(accessor, out, lenSeg).asInstanceOf[MemorySegment]
    val n = lenSeg.get(ValueLayout.JAVA_LONG, 0L)
    if isNull(ptr) || n == 0 then Array.emptyByteArray
    else
      try ptr.reinterpret(n).toArray(ValueLayout.JAVA_BYTE)
      finally invoke(bytesFreeH, ptr, java.lang.Long.valueOf(n))

  lazy val sandboxNewH: MethodHandle       = bind("cvisor_sandbox_new", fd(PTR))
  lazy val sandboxFreeH: MethodHandle      = bind("cvisor_sandbox_free", fdVoid(PTR))
  lazy val setLogLevelH: MethodHandle      = bind("cvisor_sandbox_set_log_level", fdVoid(PTR, I32))
  lazy val setAllowNetworkH: MethodHandle  = bind("cvisor_sandbox_set_allow_network", fdVoid(PTR, I32))
  lazy val setAllowListenH: MethodHandle   = bind("cvisor_sandbox_set_allow_listen", fdVoid(PTR, I32))
  lazy val setEnvH: MethodHandle           = bind("cvisor_sandbox_set_env", fdVoid(PTR, PTR, PTR))
  lazy val setLimitsH: MethodHandle        = bind("cvisor_sandbox_set_limits", fdVoid(PTR, I64, I64, I32))
  lazy val writeFileH: MethodHandle        = bind("cvisor_sandbox_write_file", fd(I32, PTR, PTR, PTR, I64))
  lazy val readFileH: MethodHandle         = bind("cvisor_sandbox_read_file", fd(PTR, PTR, PTR, PTR))
  lazy val copyIntoH: MethodHandle         = bind("cvisor_sandbox_copy_into", fd(I32, PTR, PTR, PTR))
  lazy val copyOutH: MethodHandle          = bind("cvisor_sandbox_copy_out", fd(I32, PTR, PTR, PTR))
  lazy val cacheSaveH: MethodHandle        = bind("cvisor_cache_save", fd(I32, PTR, PTR, PTR, PTR, PTR))
  lazy val cacheRestoreH: MethodHandle     = bind("cvisor_cache_restore", fd(I32, PTR, PTR, PTR, PTR, PTR))
  lazy val runH: MethodHandle              = bind("cvisor_run", fd(PTR, PTR, PTR))
  lazy val runTimeoutH: MethodHandle       = bind("cvisor_run_timeout", fd(PTR, PTR, PTR, I64))
  lazy val outputFreeH: MethodHandle       = bind("cvisor_output_free", fdVoid(PTR))
  lazy val outputExitCodeH: MethodHandle   = bind("cvisor_output_exit_code", fd(I32, PTR))
  lazy val outputStdoutH: MethodHandle     = bind("cvisor_output_stdout", fd(PTR, PTR, PTR))
  lazy val outputStderrH: MethodHandle     = bind("cvisor_output_stderr", fd(PTR, PTR, PTR))
  lazy val bytesFreeH: MethodHandle        = bind("cvisor_bytes_free", fdVoid(PTR, I64))
