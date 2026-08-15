(ns cvisor.sandbox-test
  "e2e test for the cVisor Clojure SDK. Run in a musl JDK 22 container under
  seccomp=unconfined (see README.md):
    clojure -M:test"
  (:require [clojure.test :refer [deftest is run-tests]]
            [cvisor.core :as cvisor]))

(deftest sandbox-e2e
  (with-open [sb (cvisor/sandbox)]
    (is (= "hello from clojure\n" (:stdout (cvisor/run sb "echo hello from clojure"))))
    (is (= "b\n" (:stdout (cvisor/run sb "printf 'a\nb\nc\n' | grep b"))))
    (is (= "x\n" (:stdout (cvisor/run sb "echo x > /tmp/f && grep x /tmp/f"))))
    (is (= "cvisor\n" (:stdout (cvisor/run sb "uname -n"))))
    (is (= "Name:\tcvisor-guest\n" (:stdout (cvisor/run sb "grep Name /proc/self/status"))))))

(deftest exit-codes
  (with-open [sb (cvisor/sandbox)]
    (is (= 0 (:exit-code (cvisor/run sb "true"))))
    (is (= 7 (:exit-code (cvisor/run sb "exit 7"))))
    (is (= 1 (:exit-code (cvisor/run sb "false"))))))

(deftest atomic-rename
  (with-open [sb (cvisor/sandbox)]
    (is (= "hi\n" (:stdout (cvisor/run sb "echo hi > /tmp/a.part && mv /tmp/a.part /tmp/a && grep hi /tmp/a"))))))

(deftest timeout
  (with-open [sb (cvisor/sandbox)]
    (is (= 137 (:exit-code (cvisor/run sb "sleep 30" {:timeout-ms 300}))))))

(deftest network-toggle
  (with-open [sb (cvisor/sandbox)]
    (cvisor/set-allow-network! sb false)
    ;; The egress kill switch must not crash the shell; the follow-on echo runs.
    (is (= "ok\n" (:stdout (cvisor/run sb "(nc -w1 127.0.0.1 9 </dev/null 2>/dev/null || true); echo ok"))))))

(deftest streaming-callbacks
  (with-open [sb (cvisor/sandbox)]
    (let [chunks (atom [])
          code   (cvisor/run-streaming
                  sb "for i in 1 2 3; do echo line$i; sleep 0.1; done"
                  {:on-stdout (fn [s] (swap! chunks conj s))})]
      (is (= 0 code))
      (is (= "line1\nline2\nline3\n" (apply str @chunks))))))

(deftest interactive-pty-shell
  (with-open [sb (cvisor/sandbox)]
    (let [out (atom "")
          sh  (cvisor/shell sb {:on-output (fn [s] (swap! out str s))})]
      (cvisor/write! sh "echo SHELL_OK\n")
      (cvisor/write! sh "test -t 1 && echo IS_TTY\n")
      (cvisor/write! sh "exit 4\n")
      (is (= 4 (cvisor/wait sh)))
      (Thread/sleep 100) ;; let the output thread drain
      (is (re-find #"SHELL_OK" @out))
      (is (re-find #"IS_TTY" @out))
      (.close sh))))

(deftest file-io
  (with-open [sb (cvisor/sandbox)]
    (cvisor/write-file sb "/tmp/data.txt" "seeded\n")
    (is (= "seeded\n" (:stdout (cvisor/run sb "grep seeded /tmp/data.txt"))))
    (cvisor/run sb "echo from-run > /tmp/out.txt")
    (is (= "from-run\n" (String. (cvisor/read-file sb "/tmp/out.txt") "UTF-8")))
    (cvisor/set-allow-listen! sb true)))

(deftest cache-and-copy
  ;; Seed a dir in one sandbox, cache it, restore into another, run sees it.
  (let [key (str "k-" (System/nanoTime))]
    (with-open [a (cvisor/sandbox)]
      (cvisor/write-file a "/tmp/proj/a.txt" "alpha\n")
      (cvisor/write-file a "/tmp/proj/sub/b.txt" "beta\n")
      (cvisor/cache-save a "/tmp/proj" key))
    (with-open [b (cvisor/sandbox)]
      (cvisor/cache-restore b "/tmp/proj" key)
      (is (= "alpha\nbeta\n"
             (:stdout (cvisor/run b "grep alpha /tmp/proj/a.txt && grep beta /tmp/proj/sub/b.txt")))))))

(defn -main [& _]
  (let [{:keys [fail error]} (run-tests 'cvisor.sandbox-test)]
    (if (zero? (+ fail error))
      (do (println "CLOJURE_SDK_OK") (System/exit 0))
      (System/exit 1))))
