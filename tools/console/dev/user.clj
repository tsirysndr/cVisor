(ns user
  "Auto-loaded REPL helpers. Drops every console namespace into scope under
  short aliases so you can poke around immediately.

      user=> (help)
      user=> (build/daemon)
      user=> (sdk/smoke :scala)     ;; lang is always a keyword (atom)
      user=> (ui/dev)
      user=> (docker/build :ubuntu)"
  (:require [console.core   :as c]
            [console.shell  :as sh]
            [console.build  :as build]
            [console.sdk    :as sdk]
            [console.ui     :as ui]
            [console.docker :as docker]))

(def help c/help)
(def ls   c/ls)

(println)
(println "cVisor Console — REPL loaded. Try (help) or (ls).")
(println "Aliases in scope: c, sh, build, sdk, ui, docker")
(println)
