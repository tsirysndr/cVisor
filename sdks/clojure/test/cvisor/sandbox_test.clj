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

(defn -main [& _]
  (let [{:keys [fail error]} (run-tests 'cvisor.sandbox-test)]
    (if (zero? (+ fail error))
      (do (println "CLOJURE_SDK_OK") (System/exit 0))
      (System/exit 1))))
