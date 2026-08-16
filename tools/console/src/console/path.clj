(ns console.path
  "Repo-root discovery. Lives in its own namespace so every other `console.*`
  namespace can depend on it without forming a cycle."
  (:require [babashka.fs :as fs]))

(defn repo-root
  "Walk up from cwd until we find the cVisor monorepo root, identified by the
  workspace `Cargo.toml` sitting next to the `sdks/` directory — the one place
  in the tree both exist side by side (every other `Cargo.toml` here, under
  crates/* and xtask/, has no sibling sdks/). Honors `CVISOR_ROOT` if set."
  []
  (or (System/getenv "CVISOR_ROOT")
      (loop [dir (fs/absolutize (fs/cwd))]
        (cond
          (nil? dir)
          (throw (ex-info "Could not locate cVisor repo root" {:cwd (str (fs/cwd))}))

          (and (fs/exists? (fs/path dir "Cargo.toml"))
               (fs/directory? (fs/path dir "sdks")))
          (str dir)

          :else (recur (fs/parent dir))))))
