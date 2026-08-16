import { useEffect } from "react";
import { useSetAtom } from "jotai";
import { createModalOpenAtom, settingsOpenAtom } from "../state/atoms";
import { isTauri } from "../transport";
import { useHealth } from "./useHealth";

// Desktop-only glue for the menu-bar tray: tray menu actions open the matching
// modals, and the daemon health probe updates the tray's status line.
export function useTrayActions() {
  const setCreateOpen = useSetAtom(createModalOpenAtom);
  const setSettingsOpen = useSetAtom(settingsOpenAtom);

  useEffect(() => {
    if (!isTauri()) return;
    let unlisten: (() => void) | undefined;
    let disposed = false;
    void import("@tauri-apps/api/event").then(({ listen }) =>
      listen<string>("menu://action", (e) => {
        if (e.payload === "create") setCreateOpen(true);
        if (e.payload === "settings") setSettingsOpen(true);
      }).then((fn) => {
        if (disposed) fn();
        else unlisten = fn;
      }),
    );
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [setCreateOpen, setSettingsOpen]);
}

export function useTrayStatus() {
  const { data: health, isError, error } = useHealth();

  useEffect(() => {
    if (!isTauri()) return;
    const ok = !!health?.ok && !isError;
    const detail = isError ? (error as Error)?.message ?? "" : "";
    void import("@tauri-apps/api/core").then(({ invoke }) =>
      invoke("set_tray_status", { ok, detail }).catch(() => {}),
    );
  }, [health, isError, error]);
}
