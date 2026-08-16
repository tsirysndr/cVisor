import { useAtom, useSetAtom } from "jotai";
import {
  IconCategory2,
  IconCamera,
  IconDatabase,
  IconKeyboard,
  IconSettings,
} from "@tabler/icons-react";
import {
  helpOpenAtom,
  settingsOpenAtom,
  viewAtom,
  type ViewKey,
} from "../state/atoms";
import { useSandboxes } from "../hooks/useSandboxes";
import { useSnapshots } from "../hooks/useSnapshots";
import { useCaches } from "../hooks/useCaches";

const ITEMS: { key: ViewKey; label: string; icon: typeof IconCategory2 }[] = [
  { key: "sandboxes", label: "Sandboxes", icon: IconCategory2 },
  { key: "snapshots", label: "Snapshots", icon: IconCamera },
  { key: "caches", label: "Caches", icon: IconDatabase },
];

export function Sidebar() {
  const [view, setView] = useAtom(viewAtom);
  const setHelpOpen = useSetAtom(helpOpenAtom);
  const setSettingsOpen = useSetAtom(settingsOpenAtom);
  const { data: sandboxes } = useSandboxes();
  const { data: snapshots } = useSnapshots();
  const { data: caches } = useCaches();

  const counts: Record<ViewKey, number> = {
    sandboxes: sandboxes?.length ?? 0,
    snapshots: snapshots?.length ?? 0,
    caches: caches?.length ?? 0,
  };

  return (
    <aside className="flex w-56 shrink-0 flex-col border-r border-content3 bg-content1 p-2">
      <nav className="flex flex-col gap-0.5">
        {ITEMS.map((it) => {
          const Icon = it.icon;
          const active = view === it.key;
          return (
            <button
              key={it.key}
              onClick={() => setView(it.key)}
              className={`group flex items-center gap-3 px-3 py-2 text-sm transition ${
                active
                  ? "bg-primary/15 text-foreground shadow-[inset_2px_0_0_#FF2A6D]"
                  : "text-default-500 hover:bg-content2 hover:text-foreground"
              }`}
            >
              <Icon
                size={18}
                className={active ? "text-primary" : "text-default-400"}
              />
              <span className="flex-1 text-left font-medium">{it.label}</span>
              {counts[it.key] > 0 && (
                <span
                  className={`px-1.5 py-0.5 text-[11px] font-semibold tabular-nums ${
                    active
                      ? "bg-primary text-primary-foreground"
                      : "bg-content3 text-default-400"
                  }`}
                >
                  {counts[it.key]}
                </span>
              )}
            </button>
          );
        })}
      </nav>

      <div className="flex-1" />

      <div className="mt-2 border-t border-content3 pt-2">
        <button
          onClick={() => setHelpOpen(true)}
          className="flex w-full items-center gap-3 px-3 py-2 text-sm text-default-500 transition hover:bg-content2 hover:text-foreground"
        >
          <IconKeyboard size={18} className="text-default-400" />
          <span className="font-medium">Shortcuts</span>
          <kbd className="ml-auto neon-key">?</kbd>
        </button>
        <button
          onClick={() => setSettingsOpen(true)}
          className="flex w-full items-center gap-3 px-3 py-2 text-sm text-default-500 transition hover:bg-content2 hover:text-foreground"
        >
          <IconSettings size={18} className="text-default-400" />
          <span className="font-medium">Settings</span>
        </button>
      </div>
    </aside>
  );
}
