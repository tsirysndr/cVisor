import { atom } from "jotai";
import { atomWithStorage } from "jotai/utils";
import type { ThemeName } from "../theme";

// Currently selected sandbox (id) shown in the main pane and terminal dock.
export const selectedSandboxAtom = atom<string | null>(null);

// Which section the sidebar navigates to / the main content renders.
export type ViewKey = "sandboxes" | "snapshots" | "caches";
export const viewAtom = atom<ViewKey>("sandboxes");

// Keyboard cursor (row index) for the active view's list. Reset when the view
// changes. In the Sandboxes view it is kept in sync with the selected sandbox.
export const cursorAtom = atom(0);

// Raycast-style command palette visibility.
export const paletteOpenAtom = atom(false);

// Bumped by the `f` shortcut; the Sandboxes view focuses its filter input on
// change.
export const sandboxFilterFocusAtom = atom(0);

// Keyboard-shortcut help modal visibility.
export const helpOpenAtom = atom(false);

// Modal visibilities.
export const runModalOpenAtom = atom(false);
export const createModalOpenAtom = atom(false);
export const settingsOpenAtom = atom(false);

// Per-sandbox settings modal (network flags + resource limits): the id of the
// sandbox being edited, or null when closed.
export const sandboxSettingsAtom = atom<string | null>(null);

// Snapshot picker modal: branch a new sandbox from a snapshot, or roll the
// selected sandbox back to one. `null` when closed.
export type SnapshotPickerMode = "branch" | "rollback";
export const snapshotPickerAtom = atom<SnapshotPickerMode | null>(null);

// Terminal dock tabs. Each tab is its own PTY session; a sandbox can have any
// number of tabs. Terminals for every tab stay mounted so their sessions
// survive tab switches and panel show/hide.
export interface TerminalTab {
  id: string;
  sandboxId: string;
}
export const terminalTabsAtom = atom<TerminalTab[]>([]);
export const activeTerminalTabAtom = atom<string | null>(null);

// Right-docked AI agent panel: a PTY running one of the pre-installed agent
// CLIs inside the selected sandbox, or (desktop only) on the host machine —
// where the CLI can use local cvisor skills to drive sandboxes. The chosen CLI
// and target persist.
export const agentPanelVisibleAtom = atomWithStorage("cvisor.agentPanel", false);
export const agentCliAtom = atomWithStorage("cvisor.agentCli", "claude");
export const agentTargetAtom = atomWithStorage<"sandbox" | "host">(
  "cvisor.agentTarget",
  "host",
);
export const agentFullscreenAtom = atom(false);

// Default cache store + archive format for cache operations. Empty backend =
// the daemon's disk store; e.g. "s3://bucket/prefix" targets S3 (credentials
// come from the daemon's AWS_* environment). Empty format = gzip.
export const cacheBackendAtom = atomWithStorage("cvisor.cacheBackend", "");
export const cacheFormatAtom = atomWithStorage("cvisor.cacheFormat", "");

// Layout: left sidebar + bottom-docked terminal panel. Visibility + panel
// height persist across reloads; fullscreen is transient.
export const sidebarVisibleAtom = atomWithStorage("cvisor.sidebar", true);
export const terminalPanelVisibleAtom = atomWithStorage("cvisor.terminal", true);
export const terminalHeightAtom = atomWithStorage("cvisor.terminalHeight", 280);
export const terminalFullscreenAtom = atom(false);

// App theme; persisted so the choice survives reloads. Defaults to dark.
export const themeAtom = atomWithStorage<ThemeName>("cvisor.theme", "dark");
