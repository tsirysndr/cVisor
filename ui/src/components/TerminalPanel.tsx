import { useEffect } from "react";
import { Button, Tooltip } from "@heroui/react";
import { useAtom, useAtomValue, useSetAtom } from "jotai";
import {
  IconArrowsMaximize,
  IconArrowsMinimize,
  IconPlus,
  IconTerminal2,
  IconX,
} from "@tabler/icons-react";
import {
  activeTerminalTabAtom,
  selectedSandboxAtom,
  terminalFullscreenAtom,
  terminalHeightAtom,
  terminalPanelVisibleAtom,
  terminalTabsAtom,
  type TerminalTab,
} from "../state/atoms";
import { useSandboxes } from "../hooks/useSandboxes";
import { Terminal } from "./Terminal";

const MIN_H = 140;

const newTabId = () =>
  `t${Date.now().toString(36)}${Math.random().toString(36).slice(2, 6)}`;

// IDE-style bottom-docked terminal. Each tab is an independent PTY session —
// open as many per sandbox as you like via the + button. Every tab's terminal
// stays mounted (hidden when inactive) so sessions survive tab switches and
// panel show/hide. Resizable by dragging its top edge; can go fullscreen.
export function TerminalPanel() {
  const selected = useAtomValue(selectedSandboxAtom);
  const setSelected = useSetAtom(selectedSandboxAtom);
  const [visible, setVisible] = useAtom(terminalPanelVisibleAtom);
  const [tabs, setTabs] = useAtom(terminalTabsAtom);
  const [active, setActive] = useAtom(activeTerminalTabAtom);
  const [fullscreen, setFullscreen] = useAtom(terminalFullscreenAtom);
  const [height, setHeight] = useAtom(terminalHeightAtom);
  const { data: sandboxes } = useSandboxes();

  // Selecting a sandbox while the dock is visible focuses its most recent tab,
  // opening a first one if none exists yet.
  useEffect(() => {
    if (!selected || !visible) return;
    setTabs((t) => {
      const existing = [...t].reverse().find((tab) => tab.sandboxId === selected);
      if (existing) {
        setActive((a) =>
          t.some((tab) => tab.id === a && tab.sandboxId === selected)
            ? a
            : existing.id,
        );
        return t;
      }
      const tab: TerminalTab = { id: newTabId(), sandboxId: selected };
      setActive(tab.id);
      return [...t, tab];
    });
  }, [selected, visible, setTabs, setActive]);

  // Drop tabs whose sandbox no longer exists (freed elsewhere).
  useEffect(() => {
    if (!sandboxes) return;
    const alive = new Set(sandboxes.map((s) => s.id));
    setTabs((t) =>
      t.every((tab) => alive.has(tab.sandboxId))
        ? t
        : t.filter((tab) => alive.has(tab.sandboxId)),
    );
  }, [sandboxes, setTabs]);

  // Hidden ≠ closed: when the panel is toggled off (or has no tabs yet) it
  // stays mounted with `hidden` so live terminal sessions survive show/hide.
  const shown = visible && tabs.length > 0;
  const activeTab = tabs.find((t) => t.id === active) ?? tabs[0];

  const sandboxName = (id: string) =>
    sandboxes?.find((s) => s.id === id)?.name ?? id;

  // Tabs of the same sandbox get an index suffix: name, name ·2, name ·3 …
  const label = (tab: TerminalTab) => {
    const peers = tabs.filter((t) => t.sandboxId === tab.sandboxId);
    const i = peers.findIndex((t) => t.id === tab.id);
    const name = sandboxName(tab.sandboxId);
    return i > 0 ? `${name} ·${i + 1}` : name;
  };

  const focusTab = (tab: TerminalTab) => {
    setActive(tab.id);
    setSelected(tab.sandboxId);
  };

  const openTab = (sandboxId: string) => {
    const tab: TerminalTab = { id: newTabId(), sandboxId };
    setTabs((t) => [...t, tab]);
    setActive(tab.id);
    setSelected(sandboxId);
  };

  const closeTab = (id: string) => {
    const next = tabs.filter((t) => t.id !== id);
    setTabs(next);
    if (next.length === 0) {
      setActive(null);
      setFullscreen(false);
      setVisible(false);
    } else if (activeTab?.id === id) {
      // Focus the neighbor: same index if one follows, else the new last tab.
      const i = tabs.findIndex((t) => t.id === id);
      focusTab(next[Math.min(i, next.length - 1)]);
    }
  };

  const onResizeStart = (e: React.MouseEvent) => {
    e.preventDefault();
    const startY = e.clientY;
    const startH = height;
    const maxH = window.innerHeight - 220;
    const onMove = (ev: MouseEvent) => {
      const next = startH + (startY - ev.clientY);
      setHeight(Math.max(MIN_H, Math.min(maxH, next)));
    };
    const onUp = () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
      document.body.style.userSelect = "";
      document.body.style.cursor = "";
    };
    document.body.style.userSelect = "none";
    document.body.style.cursor = "ns-resize";
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  };

  return (
    <div
      // Clamp so the dock can never overflow its column (e.g. a height
      // persisted from a larger window) and hide the terminal's bottom rows
      // behind the status line.
      style={
        !shown || fullscreen
          ? undefined
          : { height: `min(${height}px, calc(100% - 8rem))` }
      }
      className={
        !shown
          ? "hidden"
          : fullscreen
            ? "absolute inset-0 z-30 flex flex-col bg-content1"
            : "relative flex shrink-0 flex-col border-t border-content3 bg-content1"
      }
    >
      {!fullscreen && (
        <div
          onMouseDown={onResizeStart}
          className="group absolute inset-x-0 -top-1 z-10 h-2 cursor-ns-resize"
        >
          <div className="mx-auto mt-[3px] h-0.5 w-10 rounded-full bg-content4 transition group-hover:bg-primary group-hover:shadow-[0_0_8px_#FF2A6D]" />
        </div>
      )}

      <div className="flex h-8 shrink-0 items-center border-b border-content3 bg-content1 pr-1">
        <IconTerminal2 size={14} className="mx-2 shrink-0 text-secondary" />
        <div className="flex min-w-0 flex-1 items-stretch gap-px self-stretch overflow-x-auto">
          {tabs.map((tab) => (
            <div
              key={tab.id}
              onClick={() => focusTab(tab)}
              className={`group/tab flex cursor-pointer items-center gap-1.5 whitespace-nowrap px-3 text-xs transition ${
                tab.id === activeTab?.id
                  ? "border-b-2 border-primary bg-content2 text-foreground"
                  : "text-default-500 hover:bg-content2 hover:text-foreground"
              }`}
            >
              <span className="max-w-40 truncate">{label(tab)}</span>
              <button
                aria-label={`Close ${label(tab)} terminal`}
                onClick={(e) => {
                  e.stopPropagation();
                  closeTab(tab.id);
                }}
                className="grid h-4 w-4 place-items-center rounded-sm text-default-400 opacity-0 transition hover:bg-content3 hover:text-danger group-hover/tab:opacity-100"
              >
                <IconX size={11} />
              </button>
            </div>
          ))}
          <Tooltip content="New terminal for this sandbox">
            <button
              aria-label="New terminal tab"
              onClick={() => {
                const target = activeTab?.sandboxId ?? selected;
                if (target) openTab(target);
              }}
              className="grid w-7 shrink-0 place-items-center self-center rounded-sm text-default-400 transition hover:bg-content2 hover:text-foreground"
            >
              <IconPlus size={14} />
            </button>
          </Tooltip>
        </div>
        <div className="ml-1 flex shrink-0 items-center gap-0.5">
          <Tooltip content={fullscreen ? "Exit fullscreen" : "Fullscreen"}>
            <Button
              isIconOnly
              size="sm"
              variant="light"
              radius="sm"
              aria-label="Toggle fullscreen"
              onPress={() => setFullscreen((f) => !f)}
            >
              {fullscreen ? (
                <IconArrowsMinimize size={15} />
              ) : (
                <IconArrowsMaximize size={15} />
              )}
            </Button>
          </Tooltip>
          <Tooltip content="Hide terminal">
            <Button
              isIconOnly
              size="sm"
              variant="light"
              radius="sm"
              aria-label="Hide terminal"
              onPress={() => {
                setFullscreen(false);
                setVisible(false);
              }}
            >
              <IconX size={15} />
            </Button>
          </Tooltip>
        </div>
      </div>

      <div className="relative min-h-0 flex-1 overflow-hidden">
        {tabs.map((tab) => (
          <div
            key={tab.id}
            className={tab.id === activeTab?.id ? "h-full" : "hidden"}
          >
            <Terminal sandboxId={tab.sandboxId} />
          </div>
        ))}
      </div>
    </div>
  );
}
