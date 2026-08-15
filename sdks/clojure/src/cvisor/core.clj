(ns cvisor.core
  "cVisor Clojure SDK — java.lang.foreign (FFM) bindings over the libcvisor C ABI.
  Linux-only at runtime; requires JDK 22+.

    (require '[cvisor.core :as cvisor])
    (with-open [sb (cvisor/sandbox)]
      (:stdout (cvisor/run sb \"echo hello\")))   ; => \"hello\\n\""
  (:require [clojure.java.io :as io])
  (:import [java.io Closeable File]
           [java.lang.foreign Arena FunctionDescriptor Linker Linker$Option
            MemoryLayout MemorySegment SymbolLookup ValueLayout ValueLayout$OfByte
            ValueLayout$OfLong]
           [java.lang.invoke MethodHandle]
           [java.nio.charset StandardCharsets]))

(set! *warn-on-reflection* true)

(defn- host-arch ^String []
  (if (re-find #"aarch64|arm64" (System/getProperty "os.arch")) "aarch64" "x86_64"))

(defn- extract-resource ^String [^String path]
  (when-let [res (io/resource path)]
    (let [tmp (File/createTempFile "libcvisor-" ".so")]
      (.deleteOnExit tmp)
      (with-open [in (io/input-stream res)]
        (io/copy in tmp))
      (.getAbsolutePath tmp))))

(defn- library-path ^String []
  (or (System/getenv "CVISOR_LIB")
      (extract-resource (str "cvisor/native/libcvisor-" (host-arch) ".so"))
      (throw (ex-info (str "libcvisor-" (host-arch) ".so not found on the classpath; "
                           "set CVISOR_LIB or build it with `cargo xtask ffi`")
                      {:arch (host-arch)}))))

(defn- fd ^FunctionDescriptor [ret & args]
  (let [layouts (into-array MemoryLayout args)]
    (if ret
      (FunctionDescriptor/of ret layouts)
      (FunctionDescriptor/ofVoid layouts))))

(def ^:private handles
  (delay
    (let [linker (Linker/nativeLinker)
          lookup (SymbolLookup/libraryLookup (library-path) (Arena/global))
          bind   (fn ^MethodHandle [^String name desc]
                   (.downcallHandle linker (.orElseThrow (.find lookup name)) desc
                                    (into-array Linker$Option [])))
          ptr    ValueLayout/ADDRESS
          int32  ValueLayout/JAVA_INT
          size-t ValueLayout/JAVA_LONG]
      {:sandbox-new       (bind "cvisor_sandbox_new" (fd ptr))
       :sandbox-free      (bind "cvisor_sandbox_free" (fd nil ptr))
       :set-log-level     (bind "cvisor_sandbox_set_log_level" (fd nil ptr int32))
       :set-allow-network (bind "cvisor_sandbox_set_allow_network" (fd nil ptr int32))
       :run               (bind "cvisor_run" (fd ptr ptr ptr))
       :run-timeout       (bind "cvisor_run_timeout" (fd ptr ptr ptr size-t))
       :output-free       (bind "cvisor_output_free" (fd nil ptr))
       :output-exit-code  (bind "cvisor_output_exit_code" (fd int32 ptr))
       :output-stdout     (bind "cvisor_output_stdout" (fd ptr ptr ptr))
       :output-stderr     (bind "cvisor_output_stderr" (fd ptr ptr ptr))
       :bytes-free        (bind "cvisor_bytes_free" (fd nil ptr size-t))})))

(defn- call [k & args]
  (.invokeWithArguments ^MethodHandle (get @handles k) ^java.util.List (vec args)))

(defn- null-seg? [seg]
  (or (nil? seg) (zero? (.address ^MemorySegment seg))))

(defn- read-output ^bytes [^Arena arena out accessor]
  (let [len (.allocate arena ^MemoryLayout ValueLayout/JAVA_LONG)
        ptr (call accessor out len)
        n   (.get len ^ValueLayout$OfLong ValueLayout/JAVA_LONG 0)]
    (if (or (null-seg? ptr) (zero? n))
      (byte-array 0)
      (try
        (.toArray (.reinterpret ^MemorySegment ptr n) ^ValueLayout$OfByte ValueLayout/JAVA_BYTE)
        (finally
          (call :bytes-free ptr n))))))

(defrecord Sandbox [ptr]
  Closeable
  (close [_]
    (when-let [p @ptr]
      (reset! ptr nil)
      (call :sandbox-free p))))

(defn sandbox
  "Create a sandbox. Free it with `close` (Closeable, so with-open works)."
  ^Sandbox []
  (let [p (call :sandbox-new)]
    (when (null-seg? p)
      (throw (ex-info "failed to create sandbox" {})))
    (->Sandbox (atom p))))

(defn- live-ptr [^Sandbox sb]
  (or @(:ptr sb) (throw (ex-info "sandbox is closed" {}))))

(defn set-log-level!
  "Set the sandbox log level: :debug (or \"DEBUG\") enables logging, anything else disables it."
  [^Sandbox sb level]
  (call :set-log-level (live-ptr sb) (if (#{:debug "DEBUG"} level) (int 1) (int 0))))

(defn set-allow-network!
  "Enable or disable outbound INET/INET6 networking for the sandbox (default on).
  When off, the guest's attempts to create internet sockets are denied."
  [^Sandbox sb allow?]
  (call :set-allow-network (live-ptr sb) (if allow? (int 1) (int 0))))

(defn run
  "Run a shell command in the sandbox, blocking until it exits. Returns
  {:stdout String, :stderr String, :exit-code long, :stdout-bytes bytes,
  :stderr-bytes bytes}. Options:
    :timeout-ms — SIGKILL the guest after this many milliseconds (exit code 137)."
  ([^Sandbox sb ^String command] (run sb command {}))
  ([^Sandbox sb ^String command {:keys [timeout-ms]}]
   (with-open [arena (Arena/ofConfined)]
     (let [cmd (.allocateFrom arena command)
           out (if timeout-ms
                 (call :run-timeout (live-ptr sb) cmd (long timeout-ms))
                 (call :run (live-ptr sb) cmd))]
       (when (null-seg? out)
         (throw (ex-info "sandbox run failed" {:command command})))
       (try
         (let [stdout-bytes (read-output arena out :output-stdout)
               stderr-bytes (read-output arena out :output-stderr)]
           {:stdout       (String. ^bytes stdout-bytes StandardCharsets/UTF_8)
            :stderr       (String. ^bytes stderr-bytes StandardCharsets/UTF_8)
            :exit-code    (call :output-exit-code out)
            :stdout-bytes stdout-bytes
            :stderr-bytes stderr-bytes})
         (finally
           (call :output-free out)))))))

(defn close
  "Free the sandbox. Idempotent."
  [^Closeable sb]
  (.close sb))
