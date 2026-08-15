import { atom } from "jotai";
import type { ThemeName } from "../theme";

// Currently selected sandbox (id) shown in the main pane.
export const selectedSandboxAtom = atom<string | null>(null);

// Raycast-style command palette visibility.
export const paletteOpenAtom = atom(false);

// Modal visibilities.
export const runModalOpenAtom = atom(false);
export const createModalOpenAtom = atom(false);
export const settingsOpenAtom = atom(false);

// App theme; defaults to dark.
export const themeAtom = atom<ThemeName>("dark");
