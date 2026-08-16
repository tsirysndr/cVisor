import { useEffect, useState } from "react";
import {
  Modal,
  ModalBody,
  ModalContent,
  ModalFooter,
  ModalHeader,
  Switch,
} from "@heroui/react";
import { useAtom } from "jotai";
import { sandboxSettingsAtom } from "../state/atoms";
import { useConfigure, useSandboxes } from "../hooks/useSandboxes";
import {
  LimitsFields,
  limitsFormDefaults,
  limitsFromForm,
  type LimitsFormValues,
} from "./LimitsFields";
import { PrimaryButton, TextButton } from "./Buttons";

// Edit a sandbox's network flags and resource limits (applied via configure).
export function SandboxSettingsModal() {
  const [editing, setEditing] = useAtom(sandboxSettingsAtom);
  const { data: sandboxes } = useSandboxes();
  const configure = useConfigure();

  const sandbox = sandboxes?.find((s) => s.id === editing);

  const [allowNetwork, setAllowNetwork] = useState(true);
  const [allowListen, setAllowListen] = useState(false);
  const [limitsValues, setLimitsValues] = useState<LimitsFormValues>(
    limitsFormDefaults(),
  );
  const [limitsError, setLimitsError] = useState<string | null>(null);

  // Re-seed the form each time the modal opens for a sandbox.
  useEffect(() => {
    if (sandbox) {
      setAllowNetwork(sandbox.allowNetwork);
      setAllowListen(sandbox.allowListen);
      setLimitsValues(limitsFormDefaults(sandbox.limits));
      setLimitsError(null);
    }
  }, [sandbox?.id]); // eslint-disable-line react-hooks/exhaustive-deps

  const close = () => setEditing(null);

  const save = () => {
    if (!sandbox) return;
    const parsed = limitsFromForm(limitsValues);
    if ("error" in parsed) {
      setLimitsError(parsed.error);
      return;
    }
    setLimitsError(null);
    configure.mutate(
      {
        id: sandbox.id,
        allowNetwork,
        allowListen,
        limits: parsed.limits,
      },
      { onSuccess: close },
    );
  };

  return (
    // Plain dark backdrop (no blur), matching the command palette overlay.
    <Modal
      isOpen={editing !== null}
      onOpenChange={(o) => !o && close()}
      classNames={{ backdrop: "bg-black/60" }}
    >
      <ModalContent>
        <ModalHeader className="flex-col items-start gap-0">
          <span>Sandbox settings</span>
          <span className="text-xs font-normal text-default-500">
            {sandbox ? `${sandbox.name} · ${sandbox.id}` : editing}
          </span>
        </ModalHeader>
        <ModalBody className="gap-4">
          <div className="flex flex-col gap-2">
            <Switch
              size="sm"
              isSelected={allowNetwork}
              onValueChange={setAllowNetwork}
            >
              Allow outbound network
            </Switch>
            <Switch
              size="sm"
              isSelected={allowListen}
              onValueChange={setAllowListen}
            >
              Allow listening servers
            </Switch>
          </div>
          <LimitsFields values={limitsValues} onChange={setLimitsValues} />
          {limitsError && <p className="text-sm text-danger">{limitsError}</p>}
          {configure.isError && (
            <p className="text-sm text-danger">
              {(configure.error as Error).message}
            </p>
          )}
        </ModalBody>
        <ModalFooter className="grid grid-cols-2 gap-2">
          <TextButton onPress={close}>Cancel</TextButton>
          <PrimaryButton isLoading={configure.isPending} onPress={save}>
            Save
          </PrimaryButton>
        </ModalFooter>
      </ModalContent>
    </Modal>
  );
}
