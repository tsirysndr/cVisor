import { useEffect } from "react";
import { Command } from "cmdk";
import { useAtom, useSetAtom } from "jotai";
import { useQueryClient } from "@tanstack/react-query";
import {
  IconMoonStars,
  IconPlus,
  IconRefresh,
  IconSettings,
  IconTerminal2,
  IconTrash,
} from "@tabler/icons-react";
import {
  createModalOpenAtom,
  paletteOpenAtom,
  runModalOpenAtom,
  selectedSandboxAtom,
  settingsOpenAtom,
  themeAtom,
} from "../state/atoms";
import { useFreeSandbox } from "../hooks/useSandboxes";

// Raycast-style palette (opened by "/" or Cmd/Ctrl-K). cmdk handles fuzzy
// filtering + arrow-key nav; we own the overlay, backdrop, and Escape.
export function CommandPalette() {
  const [open, setOpen] = useAtom(paletteOpenAtom);
  const [theme, setTheme] = useAtom(themeAtom);
  const [selected, setSelected] = useAtom(selectedSandboxAtom);
  const setRunOpen = useSetAtom(runModalOpenAtom);
  const setCreateOpen = useSetAtom(createModalOpenAtom);
  const setSettingsOpen = useSetAtom(settingsOpenAtom);
  const free = useFreeSandbox();
  const qc = useQueryClient();

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, setOpen]);

  if (!open) return null;

  const run = (fn: () => void) => {
    setOpen(false);
    fn();
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-start justify-center bg-black/50 p-4 pt-[15vh]"
      onClick={() => setOpen(false)}
    >
      <Command
        label="Command palette"
        onClick={(e) => e.stopPropagation()}
        className="w-full max-w-lg overflow-hidden rounded-xl border border-content3 bg-content1 shadow-2xl"
      >
        <Command.Input
          autoFocus
          placeholder="Type a command or search…"
          className="w-full border-b border-content3 bg-transparent px-4 py-3 text-sm text-foreground outline-none placeholder:text-default-400"
        />
        <Command.List className="max-h-80 overflow-y-auto p-2">
          <Command.Empty className="px-3 py-6 text-center text-sm text-default-400">
            No results.
          </Command.Empty>

          <Command.Group
            heading="Actions"
            className="px-1 text-[11px] uppercase tracking-wide text-default-400"
          >
            <Item
              icon={<IconPlus size={16} />}
              onSelect={() => run(() => setCreateOpen(true))}
            >
              Create sandbox
            </Item>
            <Item
              icon={<IconTerminal2 size={16} />}
              disabled={!selected}
              onSelect={() => run(() => setSelected(selected))}
            >
              Open terminal
            </Item>
            <Item
              icon={<IconTerminal2 size={16} />}
              onSelect={() => run(() => setRunOpen(true))}
            >
              Run command…
            </Item>
            <Item
              icon={<IconTrash size={16} />}
              disabled={!selected}
              onSelect={() =>
                run(() => {
                  if (selected)
                    free.mutate(selected, {
                      onSuccess: () => setSelected(null),
                    });
                })
              }
            >
              Free sandbox
            </Item>
          </Command.Group>

          <Command.Group
            heading="View"
            className="mt-2 px-1 text-[11px] uppercase tracking-wide text-default-400"
          >
            <Item
              icon={<IconRefresh size={16} />}
              onSelect={() => run(() => qc.invalidateQueries())}
            >
              Refresh
            </Item>
            <Item
              icon={<IconMoonStars size={16} />}
              onSelect={() =>
                run(() => setTheme(theme === "dark" ? "light" : "dark"))
              }
            >
              Toggle theme
            </Item>
            <Item
              icon={<IconSettings size={16} />}
              onSelect={() => run(() => setSettingsOpen(true))}
            >
              Settings
            </Item>
          </Command.Group>
        </Command.List>
      </Command>
    </div>
  );
}

function Item({
  icon,
  children,
  onSelect,
  disabled,
}: {
  icon: React.ReactNode;
  children: React.ReactNode;
  onSelect: () => void;
  disabled?: boolean;
}) {
  return (
    <Command.Item
      onSelect={onSelect}
      disabled={disabled}
      className="flex items-center gap-2 rounded-md px-3 py-2 text-sm text-default-600 data-[disabled=true]:opacity-40"
    >
      <span className="text-primary">{icon}</span>
      {children}
    </Command.Item>
  );
}
