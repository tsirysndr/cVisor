import { useEffect } from "react";
import { Button, Tooltip } from "@heroui/react";
import { useAtom, useAtomValue, useSetAtom } from "jotai";
import {
  IconArrowsMaximize,
  IconArrowsMinimize,
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
} from "../state/atoms";
import { useSandboxes } from "../hooks/useSandboxes";
import { Terminal } from "./Terminal";

const MIN_H = 140;

// IDE-style bottom-docked terminal with one tab per sandbox. Every tab's
// terminal stays mounted (hidden when inactive) so sessions survive switches.
// Resizable by dragging its top edge; can go fullscreen; hidden when no tabs
// are open or the panel is toggled off.
export function TerminalPanel() {
  const selected = useAtomValue(selectedSandboxAtom);
  const setSelected = useSetAtom(selectedSandboxAtom);
  const [visible, setVisible] = useAtom(terminalPanelVisibleAtom);
  const [tabs, setTabs] = useAtom(terminalTabsAtom);
  const [active, setActive] = useAtom(activeTerminalTabAtom);
  const [fullscreen, setFullscreen] = useAtom(terminalFullscreenAtom);
  const [height, setHeight] = useAtom(terminalHeightAtom);
  const { data: sandboxes } = useSandboxes();

  // Selecting a sandbox while the dock is visible opens (or focuses) its tab.
  useEffect(() => {
    if (!selected || !visible) return;
    setTabs((t) => (t.includes(selected) ? t : [...t, selected]));
    setActive(selected);
  }, [selected, visible, setTabs, setActive]);

  // Drop tabs whose sandbox no longer exists (freed elsewhere).
  useEffect(() => {
    if (!sandboxes) return;
    const alive = new Set(sandboxes.map((s) => s.id));
    setTabs((t) => (t.every((id) => alive.has(id)) ? t : t.filter((id) => alive.has(id))));
    setActive((a) => (a && !alive.has(a) ? null : a));
  }, [sandboxes, setTabs, setActive]);

  // Hidden ≠ closed: when the panel is toggled off (or has no tabs yet) it
  // stays mounted with `hidden` so live terminal sessions survive show/hide.
  const shown = visible && tabs.length > 0;
  const activeTab = active && tabs.includes(active) ? active : tabs[0];

  const label = (id: string) =>
    sandboxes?.find((s) => s.id === id)?.name ?? id;

  const focusTab = (id: string) => {
    setActive(id);
    setSelected(id);
  };

  const closeTab = (id: string) => {
    const next = tabs.filter((t) => t !== id);
    setTabs(next);
    if (next.length === 0) {
      setActive(null);
      setFullscreen(false);
      setVisible(false);
    } else if (activeTab === id) {
      // Focus the neighbor: same index if one follows, else the new last tab.
      const i = tabs.indexOf(id);
      focusTab(next[Math.min(i, next.length - 1)]);
    }
  };

  const onResizeStart = (e: React.MouseEvent) => {
    e.preventDefault();
    const startY = e.clientY;
    const startH = height;
    const maxH = window.innerHeight - 160;
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
      style={!shown || fullscreen ? undefined : { height }}
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
          {tabs.map((id) => (
            <div
              key={id}
              onClick={() => focusTab(id)}
              className={`group/tab flex cursor-pointer items-center gap-1.5 whitespace-nowrap px-3 text-xs transition ${
                id === activeTab
                  ? "border-b-2 border-primary bg-content2 text-foreground"
                  : "text-default-500 hover:bg-content2 hover:text-foreground"
              }`}
            >
              <span className="max-w-40 truncate">{label(id)}</span>
              <button
                aria-label={`Close ${label(id)} terminal`}
                onClick={(e) => {
                  e.stopPropagation();
                  closeTab(id);
                }}
                className="grid h-4 w-4 place-items-center rounded-sm text-default-400 opacity-0 transition hover:bg-content3 hover:text-danger group-hover/tab:opacity-100"
              >
                <IconX size={11} />
              </button>
            </div>
          ))}
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
        {tabs.map((id) => (
          <div
            key={id}
            className={id === activeTab ? "h-full" : "hidden"}
          >
            <Terminal sandboxId={id} />
          </div>
        ))}
      </div>
    </div>
  );
}
