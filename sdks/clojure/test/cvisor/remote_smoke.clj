(ns cvisor.remote-smoke
  "Remote-GraphQL e2e smoke for the Clojure SDK's pure-JDK client
  (cvisor.remote over java.net.http — no FFM/libcvisor). Run against a running
  cvisord:

    CVISOR_GRAPHQL_URL=http://127.0.0.1:8080/graphql CVISOR_TOKEN=... clojure -M:remote"
  (:require [cvisor.remote :as remote])
  (:import [java.nio.charset StandardCharsets]))

(defn -main [& _]
  (let [url (or (System/getenv "CVISOR_GRAPHQL_URL") "http://127.0.0.1:8080/graphql")
        token (or (System/getenv "CVISOR_TOKEN") "")
        client (remote/client url token)]
    (assert (true? (get (remote/health client) "ok")) "health not ok")

    (let [out (remote/run client "echo hello")]
      (assert (= "hello\n" (get out "stdout")) (str "run stdout: " out))
      (assert (= 0 (get out "exitCode")) (str "run exit code: " out)))

    (let [sb (remote/create-sandbox client)
          id (get sb "id")]
      (assert (not (empty? id)) (str "create-sandbox returned no id: " sb))
      (remote/write-file client id "/tmp/data.txt" "round-trip\n")
      (let [data (String. ^bytes (remote/read-file client id "/tmp/data.txt")
                          StandardCharsets/UTF_8)]
        (assert (= "round-trip\n" data) (str "read-file round-trip: " data)))
      (remote/free-sandbox client id))

    (println "CLOJURE_GRAPHQL_OK")
    (flush)
    (System/exit 0)))
