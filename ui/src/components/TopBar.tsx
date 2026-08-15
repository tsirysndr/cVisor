import { Button, Tooltip } from "@heroui/react";
import { useAtom, useSetAtom } from "jotai";
import {
  IconMoon,
  IconSearch,
  IconSettings,
  IconShieldLock,
  IconSun,
} from "@tabler/icons-react";
import { useHealth } from "../hooks/useHealth";
import {
  paletteOpenAtom,
  settingsOpenAtom,
  themeAtom,
} from "../state/atoms";
import { InlineSkeleton } from "./Skeletons";

export function TopBar() {
  const { data: health, isLoading, isError } = useHealth();
  const [theme, setTheme] = useAtom(themeAtom);
  const setPaletteOpen = useSetAtom(paletteOpenAtom);
  const setSettingsOpen = useSetAtom(settingsOpenAtom);

  const connected = !!health?.ok && !isError;
  const dot = connected ? "bg-success" : isError ? "bg-danger" : "bg-warning";

  return (
    <header className="flex h-12 shrink-0 items-center gap-3 border-b border-content3 bg-content1 px-3">
      <div className="flex items-center gap-2 font-semibold">
        <IconShieldLock size={20} className="text-primary" />
        <span>cVisor</span>
      </div>

      <div className="flex items-center gap-2 text-xs text-default-500">
        {isLoading ? (
          <InlineSkeleton width={110} height={10} />
        ) : (
          <>
            <span className={`h-2 w-2 rounded-full ${dot}`} />
            <span>
              {connected
                ? `connected · v${health?.version}`
                : "disconnected"}
            </span>
          </>
        )}
      </div>

      <div className="ml-auto flex items-center gap-1">
        <Button
          size="sm"
          variant="flat"
          startContent={<IconSearch size={16} />}
          onPress={() => setPaletteOpen(true)}
        >
          <span className="hidden sm:inline">Commands</span>
          <kbd className="ml-1 rounded bg-content3 px-1 text-[10px]">/</kbd>
        </Button>
        <Tooltip content="Toggle theme">
          <Button
            isIconOnly
            size="sm"
            variant="light"
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
            aria-label="Settings"
            onPress={() => setSettingsOpen(true)}
          >
            <IconSettings size={16} />
          </Button>
        </Tooltip>
      </div>
    </header>
  );
}
