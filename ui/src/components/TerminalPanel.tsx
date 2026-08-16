import { Button, Tooltip } from "@heroui/react";
import { useAtom, useAtomValue, useSetAtom } from "jotai";
import {
  IconArrowsMaximize,
  IconArrowsMinimize,
  IconTerminal2,
  IconX,
} from "@tabler/icons-react";
import {
  selectedSandboxAtom,
  terminalFullscreenAtom,
  terminalHeightAtom,
  terminalPanelVisibleAtom,
} from "../state/atoms";
import { useSandboxes } from "../hooks/useSandboxes";
import { Terminal } from "./Terminal";

const MIN_H = 140;

// IDE-style bottom-docked terminal for the selected sandbox. Resizable by
// dragging its top edge; can go fullscreen; hidden when no sandbox is selected
// or the panel is toggled off. Its container has an explicit height so the
// terminal's FitAddon always sees a non-zero layout.
export function TerminalPanel() {
  const selected = useAtomValue(selectedSandboxAtom);
  const visible = useAtomValue(terminalPanelVisibleAtom);
  const setVisible = useSetAtom(terminalPanelVisibleAtom);
  const [fullscreen, setFullscreen] = useAtom(terminalFullscreenAtom);
  const [height, setHeight] = useAtom(terminalHeightAtom);
  const { data: sandboxes } = useSandboxes();

  if (!selected || !visible) return null;
  const sandbox = sandboxes?.find((s) => s.id === selected);
  const label = sandbox ? sandbox.name : selected;

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
      style={fullscreen ? undefined : { height }}
      className={
        fullscreen
          ? "absolute inset-0 z-30 flex flex-col bg-[#171530]"
          : "relative flex shrink-0 flex-col border-t border-content3 bg-[#171530]"
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

      <div className="flex h-8 shrink-0 items-center gap-2 border-b border-content3 bg-content1 px-3">
        <IconTerminal2 size={14} className="text-secondary" />
        <span className="text-xs font-medium text-foreground">{label}</span>
        <span className="text-[11px] text-default-400">terminal</span>
        <div className="ml-auto flex items-center gap-0.5">
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

      <div className="min-h-0 flex-1 overflow-hidden">
        <Terminal key={selected} sandboxId={selected} />
      </div>
    </div>
  );
}
