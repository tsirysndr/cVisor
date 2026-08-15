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
          int16  ValueLayout/JAVA_SHORT
          size-t ValueLayout/JAVA_LONG]
      {:sandbox-new         (bind "cvisor_sandbox_new" (fd ptr))
       :sandbox-free        (bind "cvisor_sandbox_free" (fd nil ptr))
       :set-log-level       (bind "cvisor_sandbox_set_log_level" (fd nil ptr int32))
       :set-allow-network   (bind "cvisor_sandbox_set_allow_network" (fd nil ptr int32))
       :run                 (bind "cvisor_run" (fd ptr ptr ptr))
       :run-timeout         (bind "cvisor_run_timeout" (fd ptr ptr ptr size-t))
       :output-free         (bind "cvisor_output_free" (fd nil ptr))
       :output-exit-code    (bind "cvisor_output_exit_code" (fd int32 ptr))
       :output-stdout       (bind "cvisor_output_stdout" (fd ptr ptr ptr))
       :output-stderr       (bind "cvisor_output_stderr" (fd ptr ptr ptr))
       :bytes-free          (bind "cvisor_bytes_free" (fd nil ptr size-t))
       :session-start       (bind "cvisor_session_start" (fd ptr ptr ptr int32))
       :session-read-stdout (bind "cvisor_session_read_stdout" (fd ptr ptr ptr))
       :session-read-stderr (bind "cvisor_session_read_stderr" (fd ptr ptr ptr))
       :session-write-stdin (bind "cvisor_session_write_stdin" (fd size-t ptr ptr size-t))
       :session-resize      (bind "cvisor_session_resize" (fd nil ptr int16 int16))
       :session-try-wait    (bind "cvisor_session_try_wait" (fd int32 ptr ptr))
       :session-wait        (bind "cvisor_session_wait" (fd int32 ptr))
       :session-kill        (bind "cvisor_session_kill" (fd nil ptr))
       :session-free        (bind "cvisor_session_free" (fd nil ptr))})))

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

;; ---------------------------------------------------------------------------
;; Streaming sessions: run the guest in the background, receive output via
;; callbacks as it arrives, and (for a PTY shell) feed stdin.

(defrecord Session [ptr]
  Closeable
  (close [_]
    (when-let [p @ptr]
      (reset! ptr nil)
      (call :session-free p))))

(defn- live-sess ^MemorySegment [^Session s]
  (or @(:ptr s) (throw (ex-info "session is closed" {}))))

(defn- drain ^bytes [^Session s accessor]
  (with-open [arena (Arena/ofConfined)]
    (read-output arena (live-sess s) accessor)))

(defn exit-code
  "The guest's exit code if it has finished, else nil."
  [^Session s]
  (with-open [arena (Arena/ofConfined)]
    (let [done (.allocate arena ^MemoryLayout ValueLayout/JAVA_INT)
          code (call :session-try-wait (live-sess s) done)]
      (when-not (zero? (.get done ^java.lang.foreign.ValueLayout$OfInt ValueLayout/JAVA_INT 0))
        code))))

(defn wait
  "Block until the guest exits and return its exit code."
  [^Session s]
  (call :session-wait (live-sess s)))

(defn kill!
  "SIGKILL the guest process group."
  [^Session s]
  (call :session-kill (live-sess s)))

(defn write!
  "Write a String (or bytes) to the guest's stdin (PTY shells only)."
  [^Session s data]
  (let [bytes (if (bytes? data) data (.getBytes ^String data StandardCharsets/UTF_8))
        n     (alength ^bytes bytes)]
    (with-open [arena (Arena/ofConfined)]
      (let [seg (.allocate arena (long (max n 1)))]
        (when (pos? n)
          (MemorySegment/copy ^bytes bytes 0 seg ^ValueLayout$OfByte ValueLayout/JAVA_BYTE 0 n))
        (call :session-write-stdin (live-sess s) seg (long n))))))

(defn resize!
  "Set the PTY window size (no-op for a non-PTY session)."
  [^Session s rows cols]
  (call :session-resize (live-sess s) (short rows) (short cols)))

(defn- start-session ^Session [^Sandbox sb ^String cmd pty?]
  (with-open [arena (Arena/ofConfined)]
    (let [c   (if cmd (.allocateFrom arena cmd) (MemorySegment/NULL))
          ptr (call :session-start (live-ptr sb) c (int (if pty? 1 0)))]
      (when (null-seg? ptr)
        (throw (ex-info "failed to start session" {:command cmd})))
      (->Session (atom ptr)))))

(defn- pump-loop [^Session s poll-ms on-stdout on-stderr]
  (let [emit (fn [accessor cb]
               (when cb
                 (let [b (drain s accessor)]
                   (when (pos? (alength ^bytes b))
                     (cb (String. ^bytes b StandardCharsets/UTF_8))))))]
    (loop []
      (emit :session-read-stdout on-stdout)
      (emit :session-read-stderr on-stderr)
      (if-let [code (exit-code s)]
        (do ;; final drain after exit
          (emit :session-read-stdout on-stdout)
          (emit :session-read-stderr on-stderr)
          code)
        (do (Thread/sleep (long poll-ms)) (recur))))))

(defn run-streaming
  "Run a shell command in the background, invoking :on-stdout / :on-stderr with
  String chunks as output arrives. Blocks until the guest exits; returns the
  exit code. Options: :on-stdout fn, :on-stderr fn, :poll-ms (default 15)."
  ([^Sandbox sb ^String command] (run-streaming sb command {}))
  ([^Sandbox sb ^String command {:keys [on-stdout on-stderr poll-ms] :or {poll-ms 15}}]
   (let [s (start-session sb command false)]
     (try
       (pump-loop s poll-ms on-stdout on-stderr)
       (finally
         (.close s))))))

(defn shell
  "Start an interactive /bin/sh on a PTY in the background. Returns a Session
  (Closeable). Options: :on-output fn called with String chunks of the merged
  terminal output, :poll-ms (default 15). Feed it with `write!`, resize with
  `resize!`, observe completion with `exit-code`/`wait`, and free with `close`."
  ([^Sandbox sb] (shell sb {}))
  ([^Sandbox sb {:keys [on-output poll-ms] :or {poll-ms 15}}]
   (let [s (start-session sb nil true)]
     (when on-output
       (let [emit (fn []
                    (let [b (drain s :session-read-stdout)]
                      (when (pos? (alength ^bytes b))
                        (on-output (String. ^bytes b StandardCharsets/UTF_8)))))]
         (doto (Thread.
                ^Runnable
                (fn []
                  (loop []
                    (emit)
                    (when (nil? (exit-code s))
                      (Thread/sleep (long poll-ms))
                      (recur)))
                  (emit))) ;; final drain after exit
           (.setDaemon true)
           (.start))))
     s)))
