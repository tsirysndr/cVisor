import {
  Modal,
  ModalBody,
  ModalContent,
  ModalHeader,
} from "@heroui/react";
import { useAtom, useSetAtom } from "jotai";
import { useQueryClient } from "@tanstack/react-query";
import { configAtom, isInjected } from "../config";
import { resetWsClient } from "../graphql/ws";
import { isMac, isTauri } from "../transport";
import { selectedSandboxAtom, settingsOpenAtom } from "../state/atoms";
import { ConfigForm } from "./ConfigForm";
import { MicroVmSettings } from "./MicroVmSettings";

// Re-openable connection settings. Also offers Disconnect (clears localStorage
// and returns to the Setup screen).
export function SettingsModal() {
  const [open, setOpen] = useAtom(settingsOpenAtom);
  const [config, setConfig] = useAtom(configAtom);
  const setSelected = useSetAtom(selectedSandboxAtom);
  const qc = useQueryClient();

  const disconnect = () => {
    setConfig(null);
    setSelected(null);
    resetWsClient();
    qc.clear();
    setOpen(false);
  };

  return (
    // Plain dark backdrop (no blur), matching the command palette overlay.
    <Modal
      isOpen={open}
      onOpenChange={setOpen}
      classNames={{ backdrop: "bg-black/60" }}
    >
      <ModalContent>
        <ModalHeader>Connection settings</ModalHeader>
        <ModalBody className="pb-6">
          {isInjected ? (
            <p className="text-sm text-default-500">
              Config was provided by the launcher and cannot be changed here.
            </p>
          ) : (
            <ConfigForm
              initial={config ?? undefined}
              submitLabel="Save"
              onSaved={() => setOpen(false)}
              onDisconnect={disconnect}
            />
          )}
          {/* The bsdkrun microVM only exists on macOS; hidden elsewhere. */}
          {isTauri() && isMac() && <MicroVmSettings />}
        </ModalBody>
      </ModalContent>
    </Modal>
  );
}
