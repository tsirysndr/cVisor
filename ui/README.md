# cVisor UI

A single Vite + React + TypeScript app that talks to the `cvisord` GraphQL API.
One codebase, two builds: **web** (served statically) and **desktop** (Tauri).

## Stack

Vite · React 18 · Tailwind + HeroUI (Charmbracelet-style dark theme) · Jotai ·
TanStack Query + graphql-request · graphql-ws (terminal stream) · xterm.js ·
cmdk command palette · react-hook-form + zod · react-content-loader skeletons.

## Develop (bun)

```bash
bun install
bun run dev              # web dev server on :5173
bun run build            # tsc typecheck + vite build -> dist/
bun run preview
bun run tauri dev        # desktop shell (needs the Rust toolchain)
```

## Connecting

On first launch the app shows a **Setup** screen for the GraphQL URL, WS URL
(auto-derived, editable) and bearer token; it validates via the `health` query
and persists to `localStorage` (`cvisor.config`). The CLI can instead inject
`window.__CVISOR_CONFIG__ = { graphqlUrl, wsUrl, token }` to skip setup, or set
`VITE_CVISOR_GRAPHQL_URL` / `VITE_CVISOR_WS_URL` / `VITE_CVISOR_TOKEN`. Change or
clear the connection from the top-bar Settings / command palette.

Press `/` (or Cmd/Ctrl-K) for the command palette.
