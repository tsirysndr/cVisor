(ns cvisor.graphql
  "Optional GraphQL client for the cVisor daemon's HTTP API.

  Pure JDK (`java.net.http.HttpClient`) + `clojure.data.json` — no native
  library — so it runs on any platform (macOS included), unlike the FFM-backed
  `cvisor.core/sandbox`.

    (require '[cvisor.graphql :as graphql])
    (let [c (graphql/client \"http://127.0.0.1:8080/graphql\" token)]
      (graphql/query c \"{ health { version ok } }\" {}))
    ;; => {\"health\" {\"version\" \"0.1.0\", \"ok\" true}}"
  (:require [clojure.data.json :as json]
            [clojure.string :as str])
  (:import [java.net URI]
           [java.net.http HttpClient HttpRequest HttpRequest$BodyPublishers
            HttpResponse$BodyHandlers]
           [java.time Duration]))

(set! *warn-on-reflection* true)

(defrecord Client [^HttpClient http ^String url ^String token])

(defn client
  "Create a GraphQL client for `url` authenticating with bearer `token`."
  [^String url ^String token]
  (->Client (-> (HttpClient/newBuilder)
                (.connectTimeout (Duration/ofSeconds 30))
                (.build))
            url
            token))

(defn- execute [^Client c ^String document variables]
  (let [body (json/write-str {:query document :variables (or variables {})})
        req  (-> (HttpRequest/newBuilder (URI/create (:url c)))
                 (.header "Content-Type" "application/json")
                 (.header "Authorization" (str "Bearer " (:token c)))
                 (.POST (HttpRequest$BodyPublishers/ofString body))
                 (.build))
        resp (.send ^HttpClient (:http c) req (HttpResponse$BodyHandlers/ofString))
        code (.statusCode resp)]
    (when (>= code 400)
      (throw (ex-info (str "GraphQL request failed: HTTP " code) {:status code :body (.body resp)})))
    (let [parsed (json/read-str (.body resp))
          errors (get parsed "errors")]
      (when (seq errors)
        (throw (ex-info (str/join "; " (map #(get % "message") errors))
                        {:errors errors})))
      (get parsed "data"))))

(defn query
  "Run a GraphQL query document; returns the parsed `data` map."
  ([^Client c ^String document] (query c document {}))
  ([^Client c ^String document variables] (execute c document variables)))

(defn mutate
  "Run a GraphQL mutation document; returns the parsed `data` map."
  ([^Client c ^String document] (mutate c document {}))
  ([^Client c ^String document variables] (execute c document variables)))
