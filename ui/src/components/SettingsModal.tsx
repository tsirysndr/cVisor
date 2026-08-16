import { useState } from "react";
import { Modal, ModalBody, ModalContent, ModalHeader } from "@heroui/react";
import { useAtom, useSetAtom } from "jotai";
import { useQueryClient } from "@tanstack/react-query";
import {
  IconCloudUpload,
  IconCpu,
  IconPlugConnected,
} from "@tabler/icons-react";
import { configAtom, isInjected } from "../config";
import { resetWsClient } from "../graphql/ws";
import { isMac, isTauri } from "../transport";
import { selectedSandboxAtom, settingsOpenAtom } from "../state/atoms";
import { ConfigForm } from "./ConfigForm";
import { CacheSettings } from "./CacheSettings";
import { MicroVmSettings } from "./MicroVmSettings";

type SectionKey = "connection" | "cache" | "microvm";

// Re-openable app settings: a wide modal with sidebar navigation. Sections:
// Connection (daemon endpoint/token, with Disconnect), Cache (default store +
// format), and — on macOS desktop only — the bsdkrun microVM.
export function SettingsModal() {
  const [open, setOpen] = useAtom(settingsOpenAtom);
  const [config, setConfig] = useAtom(configAtom);
  const setSelected = useSetAtom(selectedSandboxAtom);
  const [section, setSection] = useState<SectionKey>("connection");
  const qc = useQueryClient();

  const disconnect = () => {
    setConfig(null);
    setSelected(null);
    resetWsClient();
    qc.clear();
    setOpen(false);
  };

  const sections: { key: SectionKey; label: string; icon: typeof IconCpu }[] = [
    { key: "connection", label: "Connection", icon: IconPlugConnected },
    { key: "cache", label: "Cache", icon: IconCloudUpload },
    // The bsdkrun microVM only exists on macOS; hidden elsewhere.
    ...(isTauri() && isMac()
      ? [{ key: "microvm" as const, label: "microVM", icon: IconCpu }]
      : []),
  ];

  return (
    // Plain dark backdrop (no blur), matching the command palette overlay.
    <Modal
      isOpen={open}
      onOpenChange={setOpen}
      size="3xl"
      scrollBehavior="inside"
      classNames={{ backdrop: "bg-black/60" }}
    >
      <ModalContent>
        <ModalHeader>Settings</ModalHeader>
        <ModalBody className="pb-6">
          <div className="flex min-h-[22rem] gap-4">
            <nav className="flex w-40 shrink-0 flex-col gap-0.5 border-r border-content3 pr-3">
              {sections.map((s) => {
                const Icon = s.icon;
                const active = section === s.key;
                return (
                  <button
                    key={s.key}
                    onClick={() => setSection(s.key)}
                    className={`flex items-center gap-2 rounded px-3 py-2 text-left text-sm transition ${
                      active
                        ? "bg-primary/15 text-foreground shadow-[inset_2px_0_0_#FF2A6D]"
                        : "text-default-500 hover:bg-content2 hover:text-foreground"
                    }`}
                  >
                    <Icon
                      size={16}
                      className={active ? "text-primary" : "text-default-400"}
                    />
                    {s.label}
                  </button>
                );
              })}
            </nav>
            <div className="flex min-w-0 flex-1 flex-col">
              {section === "connection" &&
                (isInjected ? (
                  <p className="text-sm text-default-500">
                    Config was provided by the launcher and cannot be changed
                    here.
                  </p>
                ) : (
                  <ConfigForm
                    initial={config ?? undefined}
                    submitLabel="Save"
                    onSaved={() => setOpen(false)}
                    onDisconnect={disconnect}
                  />
                ))}
              {section === "cache" && <CacheSettings />}
              {section === "microvm" && <MicroVmSettings />}
            </div>
          </div>
        </ModalBody>
      </ModalContent>
    </Modal>
  );
}
