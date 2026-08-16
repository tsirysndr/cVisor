(ns console.docker
  "The three published container images — `ghcr.io/tsirysndr/cvisor` in its
  alpine (default), debian-trixie, and ubuntu variants — each shipping the
  `cvisor` CLI and `cvisord` daemon. Wraps `docker build` over the matching
  Dockerfile (see .github/workflows/docker.yml for the CI matrix)."
  (:refer-clojure :exclude [build])
  (:require [console.shell :as sh]))

(def ^:private variants
  "variant -> {dockerfile, tag-suffix}. The alpine image is the default
  (`cvisor:latest`); the others carry a prefix, matching docker.yml."
  {:alpine {:file "Dockerfile"        :suffix "latest"}
   :trixie {:file "Dockerfile.debian" :suffix "trixie-latest"}
   :ubuntu {:file "Dockerfile.ubuntu" :suffix "ubuntu-latest"}})

(defn- ->variant [v]
  (let [k (keyword (name v))]
    (when-not (contains? variants k)
      (throw (ex-info (str "unknown image variant " k " (expected :alpine :trixie :ubuntu)")
                      {:variant k})))
    k))

(defn build
  "Build one image variant locally. variant ∈ :alpine (default) :trixie
  :ubuntu. Extra args pass through to `docker build`. Usage:
  `(build :ubuntu)` or `(build :alpine \"--no-cache\")`."
  [variant & args]
  (let [{:keys [file suffix]} (get variants (->variant variant))]
    (sh/sh (into ["docker" "build" "-f" file "-t" (str "ghcr.io/tsirysndr/cvisor:" suffix)]
                 (concat (map str args) ["."])))))

(defn build-all
  "Build all three image variants in turn, stopping at the first failure."
  []
  (apply sh/run-steps "."
         (for [v [:alpine :trixie :ubuntu]
               :let [{:keys [file suffix]} (get variants v)]]
           ["docker" "build" "-f" file "-t" (str "ghcr.io/tsirysndr/cvisor:" suffix) "."])))
