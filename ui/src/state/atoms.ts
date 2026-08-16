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

// Keyboard-shortcut help modal visibility.
export const helpOpenAtom = atom(false);

// Modal visibilities.
export const runModalOpenAtom = atom(false);
export const createModalOpenAtom = atom(false);
export const settingsOpenAtom = atom(false);

// Snapshot picker modal: branch a new sandbox from a snapshot, or roll the
// selected sandbox back to one. `null` when closed.
export type SnapshotPickerMode = "branch" | "rollback";
export const snapshotPickerAtom = atom<SnapshotPickerMode | null>(null);

// Layout: left sidebar + bottom-docked terminal panel. Visibility + panel
// height persist across reloads; fullscreen is transient.
export const sidebarVisibleAtom = atomWithStorage("cvisor.sidebar", true);
export const terminalPanelVisibleAtom = atomWithStorage("cvisor.terminal", true);
export const terminalHeightAtom = atomWithStorage("cvisor.terminalHeight", 280);
export const terminalFullscreenAtom = atom(false);

// App theme; persisted so the choice survives reloads. Defaults to dark.
export const themeAtom = atomWithStorage<ThemeName>("cvisor.theme", "dark");
