import { useEffect, useRef } from "react";
import { useAtomValue } from "jotai";
import { useQueryClient } from "@tanstack/react-query";
import { configAtom, setActiveConfig } from "./config";
import { getTransport, isMac, isTauri } from "./transport";
import {
  sidebarVisibleAtom,
  themeAtom,
  viewAtom,
} from "./state/atoms";
import { applyTheme } from "./theme";
import { useShortcuts } from "./hooks/useShortcuts";
import { useTrayActions, useTrayStatus } from "./hooks/useTray";
import { TitleBar } from "./components/TitleBar";
import { TopBar } from "./components/TopBar";
import { StatusLine } from "./components/StatusLine";
import { Sidebar } from "./components/Sidebar";
import { TerminalPanel } from "./components/TerminalPanel";
import { SandboxesView } from "./components/views/SandboxesView";
import { SnapshotsView } from "./components/views/SnapshotsView";
import { CachesView } from "./components/views/CachesView";
import { CommandPalette } from "./components/CommandPalette";
import { HelpModal } from "./components/HelpModal";
import { RunCommandModal } from "./components/RunCommandModal";
import { CreateSandboxModal } from "./components/CreateSandboxModal";
import { SnapshotPickerModal } from "./components/SnapshotPickerModal";
import { SandboxSettingsModal } from "./components/SandboxSettingsModal";
import { SettingsModal } from "./components/SettingsModal";
import { SetupScreen } from "./components/SetupScreen";

export default function App() {
  const theme = useAtomValue(themeAtom);
  const config = useAtomValue(configAtom);
  useTrayActions();

  // Keep the module-level mirror the web graphql clients read in sync
  // (synchronous so children that fire queries on mount see the right config).
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
      if (config) void getTransport().setConfig(config);
      qc.clear();
    }
  }, [config, qc]);

  return (
    <div className="flex h-[100dvh] flex-col overflow-hidden bg-background text-foreground">
      {isTauri() && !isMac() && <TitleBar />}
      {config ? (
        <Workspace />
      ) : (
        <>
          <SetupScreen />
          <SettingsModal />
        </>
      )}
    </div>
  );
}

function Workspace() {
  useShortcuts();
  useTrayStatus();
  const sidebarVisible = useAtomValue(sidebarVisibleAtom);

  return (
    <>
      <TopBar />
      <div className="relative flex min-h-0 flex-1 flex-col">
        <div className="flex min-h-0 flex-1">
          {sidebarVisible && <Sidebar />}
          <main className="min-h-0 min-w-0 flex-1 overflow-hidden bg-background">
            <MainContent />
          </main>
        </div>
        <TerminalPanel />
      </div>
      <StatusLine />

      <CommandPalette />
      <HelpModal />
      <RunCommandModal />
      <CreateSandboxModal />
      <SnapshotPickerModal />
      <SandboxSettingsModal />
      <SettingsModal />
    </>
  );
}

function MainContent() {
  const view = useAtomValue(viewAtom);
  if (view === "snapshots") return <SnapshotsView />;
  if (view === "caches") return <CachesView />;
  return <SandboxesView />;
}
