(ns cvisor.console
  "Interactive console: a rebel-readline REPL with a live sandbox (`sb`) and an
  `sh` helper preloaded. Launch with `clojure -M:console`."
  (:require [clojure.main]
            [cvisor.core :as cvisor]
            [rebel-readline.clojure.main :as rebel]))

(defn -main [& _]
  (let [sb (cvisor/sandbox)]
    (create-ns 'user)
    (intern 'user 'sb sb)
    (intern 'user 'sh
            (fn [cmd]
              (let [{:keys [stdout stderr]} (cvisor/run sb cmd)]
                (print stdout)
                (print stderr)
                (flush))))
    (println "cVisor interactive console")
    (println "  sb                    -> a live Sandbox")
    (println "  (sh \"cmd\")            -> run a shell command in the sandbox, printing stdout/stderr")
    (println "  (cvisor.core/sandbox) -> create your own")
    (try
      (rebel/-main)
      (catch clojure.lang.ExceptionInfo e
        (if (= :rebel-readline.jline-api/bad-terminal (:type (ex-data e)))
          (do (println (.getMessage e))
              (println "Falling back to the plain Clojure REPL.")
              (clojure.main/main))
          (throw e))))))
