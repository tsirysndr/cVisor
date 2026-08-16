import { getCurrentWindow } from "@tauri-apps/api/window";
import { IconMinus, IconSquare, IconX } from "@tabler/icons-react";
import { Logo } from "./Logo";

// Custom desktop titlebar for Linux, where decorations are disabled
// (tauri.linux.conf.json). The bar itself drags the window; the right-hand
// controls drive it. macOS uses the native overlay traffic lights instead.
export function TitleBar() {
  const win = getCurrentWindow();

  return (
    <div
      data-tauri-drag-region
      className="flex h-8 shrink-0 select-none items-center justify-between border-b border-content3 bg-[#171530] px-3"
    >
      <div
        data-tauri-drag-region
        className="flex items-center gap-2 text-[11px] font-semibold tracking-wide"
      >
        <Logo size={16} />
        <span className="text-foreground">cVisor</span>
      </div>
      <div className="flex items-center gap-1">
        <button
          aria-label="Minimize"
          onClick={() => void win.minimize()}
          className="grid h-6 w-6 place-items-center text-default-500 transition hover:text-secondary"
        >
          <IconMinus size={14} />
        </button>
        <button
          aria-label="Maximize"
          onClick={() => void win.toggleMaximize()}
          className="grid h-6 w-6 place-items-center text-default-500 transition hover:text-secondary"
        >
          <IconSquare size={12} />
        </button>
        <button
          aria-label="Close"
          onClick={() => void win.close()}
          className="grid h-6 w-6 place-items-center text-default-500 transition hover:text-danger"
        >
          <IconX size={14} />
        </button>
      </div>
    </div>
  );
}
