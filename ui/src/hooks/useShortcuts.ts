import { useEffect } from "react";
import { useAtom, useAtomValue, useSetAtom } from "jotai";
import {
  createModalOpenAtom,
  cursorAtom,
  helpOpenAtom,
  paletteOpenAtom,
  runModalOpenAtom,
  selectedSandboxAtom,
  settingsOpenAtom,
  sidebarVisibleAtom,
  snapshotPickerAtom,
  terminalPanelVisibleAtom,
  themeAtom,
  viewAtom,
} from "../state/atoms";
import { useBranch } from "./useSandboxes";
import { useSnapshot } from "./useSnapshots";
import {
  useInfiniteCaches,
  useInfiniteSandboxes,
  useInfiniteSnapshots,
} from "./useInfinitePaged";

// True when a key event targets an editable surface (form field or the
// terminal) where global single-key shortcuts must not fire.
function isTyping(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null;
  if (!el) return false;
  if (
    el.tagName === "INPUT" ||
    el.tagName === "TEXTAREA" ||
    el.isContentEditable
  ) {
    return true;
  }
  return !!el.closest?.(".xterm");
}

// Centralized global keyboard shortcuts. Reset the list cursor when the view
// changes; arrow keys move it and can trigger the next page near the bottom.
export function useShortcuts() {
  const [view] = useAtom(viewAtom);
  const [cursor, setCursor] = useAtom(cursorAtom);
  const setSelected = useSetAtom(selectedSandboxAtom);
  const [palette, setPalette] = useAtom(paletteOpenAtom);
  const setHelp = useSetAtom(helpOpenAtom);
  const setCreate = useSetAtom(createModalOpenAtom);
  const setPicker = useSetAtom(snapshotPickerAtom);
  const setSidebar = useSetAtom(sidebarVisibleAtom);
  const setPanel = useSetAtom(terminalPanelVisibleAtom);
  const setTheme = useSetAtom(themeAtom);
  const selected = useAtomValue(selectedSandboxAtom);

  const help = useAtomValue(helpOpenAtom);
  const create = useAtomValue(createModalOpenAtom);
  const settings = useAtomValue(settingsOpenAtom);
  const run = useAtomValue(runModalOpenAtom);
  const picker = useAtomValue(snapshotPickerAtom);
  const setView = useSetAtom(viewAtom);

  const sandboxes = useInfiniteSandboxes();
  const snapshots = useInfiniteSnapshots();
  const caches = useInfiniteCaches();
  const branch = useBranch();
  const snapshot = useSnapshot();

  // Reset the cursor to the top whenever the active view changes.
  useEffect(() => setCursor(0), [view, setCursor]);

  const active =
    view === "sandboxes"
      ? sandboxes
      : view === "snapshots"
        ? snapshots
        : caches;
  const items = active.items;
  const overlayOpen = palette || help || create || settings || run || picker !== null;

  useEffect(() => {
    const move = (delta: number) => {
      if (items.length === 0) return;
      const next = Math.max(0, Math.min(items.length - 1, cursor + delta));
      setCursor(next);
      if (view === "sandboxes") {
        const sb = sandboxes.items[next];
        if (sb) setSelected(sb.id);
      }
      if (
        delta > 0 &&
        next >= items.length - 1 &&
        active.hasNextPage &&
        !active.isFetchingNextPage
      ) {
        active.fetchNextPage();
      }
    };

    const activate = () => {
      const item = items[cursor];
      if (!item) return;
      if (view === "sandboxes") {
        setSelected(sandboxes.items[cursor].id);
        setPanel(true);
      } else if (view === "snapshots") {
        const snap = snapshots.items[cursor];
        branch.mutate(
          { snapshotId: snap.name },
          {
            onSuccess: (sb) => {
              setSelected(sb.id);
              setView("sandboxes");
            },
          },
        );
      }
    };

    const onKey = (e: KeyboardEvent) => {
      const mod = e.metaKey || e.ctrlKey;
      const key = e.key.toLowerCase();

      // Modifier chords work regardless of focus.
      if (mod && key === "k") {
        e.preventDefault();
        setPalette((o) => !o);
        return;
      }
      if (mod && key === "b") {
        e.preventDefault();
        setSidebar((s) => !s);
        return;
      }
      if (mod && key === "j") {
        e.preventDefault();
        setPanel((p) => !p);
        return;
      }
      if (mod) return;

      if (isTyping(e.target)) return;
      // Overlays own their own navigation/close keys.
      if (overlayOpen) return;

      switch (e.key) {
        case "/":
          e.preventDefault();
          setPalette(true);
          break;
        case "?":
          e.preventDefault();
          setHelp(true);
          break;
        case "ArrowDown":
          e.preventDefault();
          move(1);
          break;
        case "ArrowUp":
          e.preventDefault();
          move(-1);
          break;
        case "Enter":
          e.preventDefault();
          activate();
          break;
        case "t":
          if (selected) setPanel(true);
          break;
        case "s":
          if (selected) snapshot.mutate({ id: selected });
          break;
        case "b":
          setPicker("branch");
          break;
        case "r":
          if (selected) setPicker("rollback");
          break;
        case "c":
          setCreate(true);
          break;
        case "d":
          setTheme((t) => (t === "dark" ? "light" : "dark"));
          break;
        default:
          break;
      }
    };

    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [
    active,
    branch,
    cursor,
    items,
    overlayOpen,
    sandboxes,
    snapshots,
    selected,
    setCreate,
    setHelp,
    setPalette,
    setPanel,
    setPicker,
    setSelected,
    setSidebar,
    setCursor,
    setTheme,
    setView,
    snapshot,
    view,
  ]);
}
