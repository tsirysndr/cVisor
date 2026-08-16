import { Button, Tooltip } from "@heroui/react";
import { useAtom, useSetAtom } from "jotai";
import {
  IconLayoutBottombar,
  IconLayoutBottombarFilled,
  IconLayoutSidebar,
  IconLayoutSidebarFilled,
  IconMoon,
  IconPlus,
  IconSearch,
  IconSettings,
  IconSun,
} from "@tabler/icons-react";
import { useHealth } from "../hooks/useHealth";
import { isMac, isTauri } from "../transport";
import {
  createModalOpenAtom,
  paletteOpenAtom,
  settingsOpenAtom,
  sidebarVisibleAtom,
  terminalPanelVisibleAtom,
  themeAtom,
} from "../state/atoms";
import { PrimaryButton } from "./Buttons";
import { Logo } from "./Logo";
import { InlineSkeleton } from "./Skeletons";

export function TopBar() {
  const { data: health, isLoading, isError } = useHealth();
  const [theme, setTheme] = useAtom(themeAtom);
  const [sidebar, setSidebar] = useAtom(sidebarVisibleAtom);
  const [panel, setPanel] = useAtom(terminalPanelVisibleAtom);
  const setPaletteOpen = useSetAtom(paletteOpenAtom);
  const setSettingsOpen = useSetAtom(settingsOpenAtom);
  const setCreateOpen = useSetAtom(createModalOpenAtom);

  const connected = !!health?.ok && !isError;
  const dot = connected ? "bg-success" : isError ? "bg-danger" : "bg-warning";
  const glow = connected
    ? "shadow-[0_0_8px_#05FFA1]"
    : isError
      ? "shadow-[0_0_8px_#FF3864]"
      : "shadow-[0_0_8px_#FFD319]";

  // On macOS the native traffic lights overlay the top-left corner, so the bar
  // doubles as the window drag region and leaves room for them.
  const macDesktop = isTauri() && isMac();

  return (
    <header
      data-tauri-drag-region
      className={`flex h-12 shrink-0 items-center gap-3 border-b border-content3 bg-content1 pr-3 ${
        macDesktop ? "pl-20" : "pl-3"
      }`}
    >
      <div className="flex items-center gap-0.5">
        <Tooltip content={sidebar ? "Hide sidebar (⌘B)" : "Show sidebar (⌘B)"}>
          <Button
            isIconOnly
            size="sm"
            variant="light"
            radius="none"
            aria-label="Toggle sidebar"
            className="text-default-500 data-[hover=true]:text-secondary"
            onPress={() => setSidebar((s) => !s)}
          >
            {sidebar ? (
              <IconLayoutSidebarFilled size={18} />
            ) : (
              <IconLayoutSidebar size={18} />
            )}
          </Button>
        </Tooltip>
        <Tooltip content={panel ? "Hide terminal (⌘J)" : "Show terminal (⌘J)"}>
          <Button
            isIconOnly
            size="sm"
            variant="light"
            radius="none"
            aria-label="Toggle terminal panel"
            className="text-default-500 data-[hover=true]:text-secondary"
            onPress={() => setPanel((p) => !p)}
          >
            {panel ? (
              <IconLayoutBottombarFilled size={18} />
            ) : (
              <IconLayoutBottombar size={18} />
            )}
          </Button>
        </Tooltip>
      </div>

      <div
        data-tauri-drag-region
        className="flex items-center gap-2 font-semibold"
      >
        <Logo size={22} />
        <span className="pointer-events-none hidden sm:inline">cVisor</span>
      </div>

      <div
        data-tauri-drag-region
        className="flex items-center gap-2 text-xs text-default-500"
      >
        {isLoading ? (
          <InlineSkeleton width={110} height={10} />
        ) : (
          <>
            <span className={`h-2 w-2 rounded-full ${dot} ${glow}`} />
            <span className="pointer-events-none">
              {connected ? `connected · v${health?.version}` : "disconnected"}
            </span>
          </>
        )}
      </div>

      <div data-tauri-drag-region className="flex-1" />

      <div className="flex items-center gap-1">
        <Button
          size="sm"
          variant="flat"
          radius="none"
          startContent={<IconSearch size={16} />}
          onPress={() => setPaletteOpen(true)}
        >
          <span className="hidden sm:inline">Search</span>
          <kbd className="ml-1 rounded bg-content3 px-1 text-[10px]">/</kbd>
        </Button>
        <Tooltip content="Toggle theme">
          <Button
            isIconOnly
            size="sm"
            variant="light"
            radius="none"
            aria-label="Toggle theme"
            onPress={() => setTheme(theme === "dark" ? "light" : "dark")}
          >
            {theme === "dark" ? <IconSun size={16} /> : <IconMoon size={16} />}
          </Button>
        </Tooltip>
        <Tooltip content="Settings">
          <Button
            isIconOnly
            size="sm"
            variant="light"
            radius="none"
            aria-label="Settings"
            onPress={() => setSettingsOpen(true)}
          >
            <IconSettings size={16} />
          </Button>
        </Tooltip>
        <PrimaryButton
          size="sm"
          startContent={<IconPlus size={16} />}
          onPress={() => setCreateOpen(true)}
        >
          New sandbox
        </PrimaryButton>
      </div>
    </header>
  );
}
