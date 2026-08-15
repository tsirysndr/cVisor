import { useEffect, useRef } from "react";
import { useAtom, useAtomValue } from "jotai";
import { useQueryClient } from "@tanstack/react-query";
import { IconTerminal2 } from "@tabler/icons-react";
import { configAtom, setActiveConfig } from "./config";
import { resetWsClient } from "./graphql/ws";
import {
  paletteOpenAtom,
  selectedSandboxAtom,
  themeAtom,
} from "./state/atoms";
import { applyTheme } from "./theme";
import { TopBar } from "./components/TopBar";
import { Sidebar } from "./components/Sidebar";
import { Terminal } from "./components/Terminal";
import { CommandPalette } from "./components/CommandPalette";
import { RunCommandModal } from "./components/RunCommandModal";
import { CreateSandboxModal } from "./components/CreateSandboxModal";
import { SettingsModal } from "./components/SettingsModal";
import { SetupScreen } from "./components/SetupScreen";

export default function App() {
  const theme = useAtomValue(themeAtom);
  const config = useAtomValue(configAtom);

  // Keep the module-level mirror the graphql clients read in sync (synchronous
  // so children that fire queries on mount see the right URL/token).
  setActiveConfig(config);

  useEffect(() => applyTheme(theme), [theme]);

  const qc = useQueryClient();
  const prevSig = useRef<string | null>(null);
  useEffect(() => {
    const sig = config
      ? `${config.graphqlUrl}|${config.wsUrl}|${config.token}`
      : null;
    if (sig !== prevSig.current) {
      prevSig.current = sig;
      resetWsClient();
      qc.clear();
    }
  }, [config, qc]);

  if (!config) {
    return (
      <>
        <SetupScreen />
        <SettingsModal />
      </>
    );
  }

  return <Workspace />;
}

function Workspace() {
  const selected = useAtomValue(selectedSandboxAtom);
  const [paletteOpen, setPaletteOpen] = useAtom(paletteOpenAtom);

  // Global shortcuts: Cmd/Ctrl-K toggles the palette; "/" opens it (unless the
  // user is typing into a field).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const t = e.target as HTMLElement | null;
      const typing =
        !!t &&
        (t.tagName === "INPUT" ||
          t.tagName === "TEXTAREA" ||
          t.isContentEditable);
      if (e.key === "k" && (e.metaKey || e.ctrlKey)) {
        e.preventDefault();
        setPaletteOpen((o) => !o);
      } else if (e.key === "/" && !typing && !paletteOpen) {
        e.preventDefault();
        setPaletteOpen(true);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [paletteOpen, setPaletteOpen]);

  return (
    <div className="flex h-screen flex-col">
      <TopBar />
      <div className="flex flex-1 overflow-hidden">
        <Sidebar />
        <main className="flex-1 overflow-hidden bg-background">
          {selected ? (
            <Terminal key={selected} sandboxId={selected} />
          ) : (
            <EmptyState />
          )}
        </main>
      </div>
      <CommandPalette />
      <RunCommandModal />
      <CreateSandboxModal />
      <SettingsModal />
    </div>
  );
}

function EmptyState() {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-3 text-default-500">
      <IconTerminal2 size={48} className="text-primary" stroke={1.5} />
      <p className="text-sm">
        Select a sandbox, or press{" "}
        <kbd className="rounded bg-content2 px-1.5 py-0.5 text-foreground">
          /
        </kbd>{" "}
        for commands.
      </p>
    </div>
  );
}
