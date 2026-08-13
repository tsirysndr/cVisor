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

(defn -main [& _]
  (let [{:keys [fail error]} (run-tests 'cvisor.sandbox-test)]
    (if (zero? (+ fail error))
      (do (println "CLOJURE_SDK_OK") (System/exit 0))
      (System/exit 1))))
