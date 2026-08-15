(ns build
  "Build/publish the cVisor Clojure SDK: clojure -T:build jar|install|deploy|clean"
  (:require [clojure.java.io :as io]
            [clojure.tools.build.api :as b]
            [deps-deploy.deps-deploy :as dd]))

(def lib 'io.github.tsirysndr/cvisor)
(def version "0.2.0")
(def class-dir "target/classes")
(def jar-file (format "target/cvisor-%s.jar" version))

(def ^:private natives
  ["resources/cvisor/native/libcvisor-aarch64.so"
   "resources/cvisor/native/libcvisor-x86_64.so"])

(defn- check-natives []
  (doseq [path natives]
    (when-not (.exists (io/file path))
      (throw (ex-info (str path " is missing; build it from the repo root with "
                           "`cargo xtask ffi [--arch x86_64]`")
                      {:path path})))))

(defn clean [_]
  (b/delete {:path "target"}))

(defn jar [_]
  (check-natives)
  (b/write-pom {:class-dir class-dir
                :lib       lib
                :version   version
                :basis     (b/create-basis {:project "deps.edn"})
                :src-dirs  ["src"]
                :scm       {:url        "https://github.com/tsirysndr/cVisor"
                            :connection "scm:git:https://github.com/tsirysndr/cVisor.git"
                            :tag        (str "clojure-sdk-v" version)}
                :pom-data  [[:description
                             "In-process Linux sandbox — Clojure SDK (Java FFM over the libcvisor C ABI)"]
                            [:url "https://github.com/tsirysndr/cVisor"]
                            [:licenses
                             [:license
                              [:name "MIT"]
                              [:url "https://opensource.org/license/mit"]]]]})
  (b/copy-dir {:src-dirs ["src" "resources"] :target-dir class-dir})
  (b/jar {:class-dir class-dir :jar-file jar-file}))

(defn install [_]
  (jar nil)
  (b/install {:basis     (b/create-basis {:project "deps.edn"})
              :lib       lib
              :version   version
              :jar-file  jar-file
              :class-dir class-dir}))

;; Needs CLOJARS_USERNAME and CLOJARS_PASSWORD (a Clojars deploy token).
(defn deploy [_]
  (jar nil)
  (dd/deploy {:installer :remote
              :artifact  (b/resolve-path jar-file)
              :pom-file  (b/pom-path {:lib lib :class-dir class-dir})}))
