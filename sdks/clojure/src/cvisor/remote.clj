(ns cvisor.remote
  "A GraphQL-backed high-level API mirroring the FFM-backed `cvisor.core`
  sandbox, but driven over HTTP against a running cVisor daemon. Pure JDK HTTP +
  JSON (via `cvisor.graphql`) — no native library — so it runs on any platform
  (macOS included). This is what makes the SDK usable off Linux.

    (require '[cvisor.remote :as remote])
    (let [c (remote/client \"http://127.0.0.1:8080/graphql\" token)]
      (get (remote/run c \"echo hello\") \"stdout\"))   ; => \"hello\\n\""
  (:require [cvisor.graphql :as graphql])
  (:import [java.nio.charset StandardCharsets]
           [java.util Base64]))

(def ^:private sandbox-fields
  (str "{ id name allowNetwork allowListen env { key value } "
       "limits { memoryMax pidsMax cpuPercent } }"))

(defn client
  "Create a remote-sandbox client for the daemon at `url` with bearer `token`."
  [url token]
  (graphql/client url token))

(defn health
  "Daemon liveness and build info."
  [client]
  (get (graphql/query client "{ health { version ok } }") "health"))

(defn run
  "Run a command in a fresh ephemeral sandbox. Returns a map with \"stdout\",
  \"stderr\" (strings) and \"exitCode\" (long). Options: :timeout-ms."
  ([client command] (run client command {}))
  ([client command {:keys [timeout-ms]}]
   (-> (graphql/mutate
        client
        "mutation($command: String!, $timeoutMs: Int) {
           run(command: $command, timeoutMs: $timeoutMs) { stdout stderr exitCode }
         }"
        {"command" command "timeoutMs" (or timeout-ms 0)})
       (get "run"))))

(defn create-sandbox
  "Create a sandbox; a nil/empty name gets a random docker-style name."
  ([client] (create-sandbox client nil))
  ([client name]
   (-> (graphql/mutate client
                       (str "mutation($name: String) { createSandbox(name: $name) " sandbox-fields " }")
                       {"name" name})
       (get "createSandbox"))))

(defn list-sandboxes
  "All live sandboxes."
  [client]
  (get (graphql/query client (str "{ sandboxes " sandbox-fields " }") {}) "sandboxes"))

(defn free-sandbox
  "Free a sandbox and clean up its overlay."
  [client id]
  (-> (graphql/mutate client "mutation($id: String!) { freeSandbox(id: $id) }" {"id" id})
      (get "freeSandbox")))

(defn configure
  "Update a sandbox's network/listen flags, limits, and env. `opts` keys:
  :allow-network, :allow-listen, :limits (map), :env (vector of {key,value} maps)."
  [client id {:keys [allow-network allow-listen limits env]}]
  (-> (graphql/mutate
       client
       (str "mutation($id: String!, $allowNetwork: Boolean, $allowListen: Boolean,"
            " $limits: LimitsInput, $env: [EnvVarInput!]) {"
            " configure(id: $id, allowNetwork: $allowNetwork, allowListen: $allowListen,"
            " limits: $limits, env: $env) " sandbox-fields " }")
       {"id" id "allowNetwork" allow-network "allowListen" allow-listen
        "limits" limits "env" env})
      (get "configure")))

(defn write-file
  "Write bytes (or a String) to a file inside a sandbox (base64 over the wire)."
  [client id path data]
  (let [bytes (if (bytes? data) data (.getBytes ^String data StandardCharsets/UTF_8))
        encoded (.encodeToString (Base64/getEncoder) bytes)]
    (-> (graphql/mutate
         client
         "mutation($id: String!, $path: String!, $data: String!) {
            writeFile(id: $id, path: $path, dataBase64: $data)
          }"
         {"id" id "path" path "data" encoded})
        (get "writeFile"))))

(defn read-file
  "Read a file from a sandbox (base64-decoded from the wire); returns bytes."
  ^bytes [client id path]
  (let [encoded (-> (graphql/query
                     client
                     "query($id: String!, $path: String!) { readFile(id: $id, path: $path) }"
                     {"id" id "path" path})
                    (get "readFile"))]
    (.decode (Base64/getDecoder) ^String encoded)))

(defn cache-save
  "Archive a sandbox path into the cache under `key`. Options: :backend, :format."
  ([client id path key] (cache-save client id path key {}))
  ([client id path key {:keys [backend format] :or {backend "" format "gzip"}}]
   (-> (graphql/mutate
        client
        "mutation($id: String!, $path: String!, $key: String!, $backend: String, $format: String) {
           cacheSave(id: $id, path: $path, key: $key, backend: $backend, format: $format)
         }"
        {"id" id "path" path "key" key "backend" backend "format" format})
       (get "cacheSave"))))

(defn cache-restore
  "Restore a cached entry into a sandbox path. Options: :backend, :format."
  ([client id path key] (cache-restore client id path key {}))
  ([client id path key {:keys [backend format] :or {backend "" format "gzip"}}]
   (-> (graphql/mutate
        client
        "mutation($id: String!, $path: String!, $key: String!, $backend: String, $format: String) {
           cacheRestore(id: $id, path: $path, key: $key, backend: $backend, format: $format)
         }"
        {"id" id "path" path "key" key "backend" backend "format" format})
       (get "cacheRestore"))))

(defn cache-list
  "List entries in a cache backend (default backend when nil)."
  ([client] (cache-list client nil))
  ([client backend]
   (-> (graphql/query
        client
        "query($backend: String) { cacheList(backend: $backend) { name size } }"
        {"backend" backend})
       (get "cacheList"))))

(defn snapshot
  "Snapshot a sandbox's overlay; returns the snapshot id."
  ([client id] (snapshot client id nil))
  ([client id snapshot-id]
   (-> (graphql/mutate
        client
        "mutation($id: String!, $snapshotId: String) { snapshot(id: $id, snapshotId: $snapshotId) }"
        {"id" id "snapshotId" snapshot-id})
       (get "snapshot"))))

(defn rollback
  "Replace a sandbox's overlay with a snapshot (discard changes since)."
  [client id snapshot-id]
  (-> (graphql/mutate
       client
       "mutation($id: String!, $snapshotId: String!) { rollback(id: $id, snapshotId: $snapshotId) }"
       {"id" id "snapshotId" snapshot-id})
      (get "rollback")))

(defn branch
  "Create a new sandbox from a snapshot."
  ([client snapshot-id] (branch client snapshot-id nil))
  ([client snapshot-id name]
   (-> (graphql/mutate
        client
        (str "mutation($snapshotId: String!, $name: String) { branch(snapshotId: $snapshotId, name: $name) "
             sandbox-fields " }")
        {"snapshotId" snapshot-id "name" name})
       (get "branch"))))

(defn fork
  "Fork a live sandbox's overlay into a new sandbox."
  ([client id] (fork client id nil))
  ([client id name]
   (-> (graphql/mutate
        client
        (str "mutation($id: String!, $name: String) { fork(id: $id, name: $name) " sandbox-fields " }")
        {"id" id "name" name})
       (get "fork"))))

(defn snapshots
  "List saved overlay snapshots."
  [client]
  (get (graphql/query client "{ snapshots { name size } }" {}) "snapshots"))

(defn delete-snapshot
  "Delete a snapshot; returns whether it existed."
  [client id]
  (-> (graphql/mutate client "mutation($id: String!) { deleteSnapshot(id: $id) }" {"id" id})
      (get "deleteSnapshot")))
