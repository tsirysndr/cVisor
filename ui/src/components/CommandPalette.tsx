import { useEffect } from "react";
import { Command } from "cmdk";
import { useAtom, useSetAtom } from "jotai";
import { useQueryClient } from "@tanstack/react-query";
import {
  IconArrowBackUp,
  IconCamera,
  IconGitBranch,
  IconKeyboard,
  IconLayoutBottombar,
  IconLayoutSidebar,
  IconMoonStars,
  IconPlus,
  IconRefresh,
  IconSettings,
  IconTerminal2,
  IconTrash,
} from "@tabler/icons-react";
import {
  createModalOpenAtom,
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
import { useFreeSandbox, useSandboxes } from "../hooks/useSandboxes";
import { useSnapshot } from "../hooks/useSnapshots";

// Raycast-style palette (opened by "/" or Cmd/Ctrl-K). cmdk handles fuzzy
// filtering + arrow-key nav; we own the overlay, backdrop, and Escape.
export function CommandPalette() {
  const [open, setOpen] = useAtom(paletteOpenAtom);
  const [theme, setTheme] = useAtom(themeAtom);
  const [selected, setSelected] = useAtom(selectedSandboxAtom);
  const setRunOpen = useSetAtom(runModalOpenAtom);
  const setCreateOpen = useSetAtom(createModalOpenAtom);
  const setSettingsOpen = useSetAtom(settingsOpenAtom);
  const setHelpOpen = useSetAtom(helpOpenAtom);
  const setSidebar = useSetAtom(sidebarVisibleAtom);
  const setPanel = useSetAtom(terminalPanelVisibleAtom);
  const setPicker = useSetAtom(snapshotPickerAtom);
  const setView = useSetAtom(viewAtom);
  const free = useFreeSandbox();
  const snapshot = useSnapshot();
  const qc = useQueryClient();
  const { data: sandboxes } = useSandboxes();

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
      className="fixed inset-0 z-50 flex items-start justify-center bg-black/60 p-4 pt-[15vh]"
      onClick={() => setOpen(false)}
    >
      <Command
        label="Command palette"
        onClick={(e) => e.stopPropagation()}
        className="w-full max-w-lg overflow-hidden rounded-large border border-content3 bg-content1 shadow-[0_0_30px_rgba(255,42,109,0.15)]"
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

          {sandboxes && sandboxes.length > 0 && (
            <Command.Group
              heading="Sandboxes"
              className="px-1 text-[11px] uppercase tracking-wide text-default-400"
            >
              {sandboxes.map((sb) => (
                <Command.Item
                  key={sb.id}
                  // Match on name and id so a partial id also finds it.
                  value={`${sb.name} ${sb.id}`}
                  onSelect={() =>
                    run(() => {
                      setView("sandboxes");
                      setSelected(sb.id);
                      setPanel(true);
                    })
                  }
                  className="flex items-center gap-2 px-3 py-2 text-sm text-default-600"
                >
                  <span className="text-secondary">
                    <IconTerminal2 size={16} />
                  </span>
                  <span className="min-w-0 flex-1 truncate">{sb.name}</span>
                  <span className="truncate text-[11px] text-default-400">
                    {sb.id}
                  </span>
                </Command.Item>
              ))}
            </Command.Group>
          )}

          <Command.Group
            heading="Sandbox"
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
              onSelect={() => run(() => setPanel(true))}
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
              icon={<IconCamera size={16} />}
              disabled={!selected}
              onSelect={() =>
                run(() => selected && snapshot.mutate({ id: selected }))
              }
            >
              Snapshot sandbox
            </Item>
            <Item
              icon={<IconGitBranch size={16} />}
              onSelect={() => run(() => setPicker("branch"))}
            >
              Branch from snapshot…
            </Item>
            <Item
              icon={<IconArrowBackUp size={16} />}
              disabled={!selected}
              onSelect={() => run(() => setPicker("rollback"))}
            >
              Rollback to snapshot…
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
              icon={<IconLayoutSidebar size={16} />}
              onSelect={() => run(() => setSidebar((s) => !s))}
            >
              Toggle sidebar
            </Item>
            <Item
              icon={<IconLayoutBottombar size={16} />}
              onSelect={() => run(() => setPanel((p) => !p))}
            >
              Toggle terminal panel
            </Item>
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
              icon={<IconKeyboard size={16} />}
              onSelect={() => run(() => setHelpOpen(true))}
            >
              Keyboard shortcuts
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
      className="flex items-center gap-2 px-3 py-2 text-sm text-default-600 data-[disabled=true]:opacity-40"
    >
      <span className="text-primary">{icon}</span>
      {children}
    </Command.Item>
  );
}
