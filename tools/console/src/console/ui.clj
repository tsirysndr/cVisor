(ns console.ui
  "The web UI (ui/) — a Docker-Desktop-style SPA over the daemon's GraphQL API,
  plus its Tauri desktop shell. The `cvisor` CLI embeds ui/dist via rust-embed
  (`cvisor ui`), so run `(build)` before a release CLI build. See
  `ui/package.json` for the scripts these wrap."
  (:refer-clojure :exclude [build])
  (:require [console.shell :as sh]))

(def ^:private in-ui {:dir "ui"})

(defn install
  "`bun install` — fetch the UI's node deps."
  [] (sh/sh ["bun" "install"] in-ui))

(defn dev
  "`bun run dev` — Vite dev server."
  [] (sh/sh ["bun" "run" "dev"] in-ui))

(defn build
  "`bun run build` — `tsc --noEmit && vite build` into ui/dist."
  [] (sh/sh ["bun" "run" "build"] in-ui))

(defn preview
  "`bun run preview` — serve the built ui/dist."
  [] (sh/sh ["bun" "run" "preview"] in-ui))

(defn tauri
  "Escape hatch: any `tauri` subcommand via `bun run tauri <args>`, e.g.
  `(tauri \"dev\")` or `(tauri \"build\")` for the native desktop app."
  [& args]
  (sh/sh (into ["bun" "run" "tauri"] (map str args)) in-ui))
