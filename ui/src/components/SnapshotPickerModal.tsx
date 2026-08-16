import {
  Modal,
  ModalBody,
  ModalContent,
  ModalFooter,
  ModalHeader,
} from "@heroui/react";
import { useAtom, useAtomValue, useSetAtom } from "jotai";
import { IconCamera } from "@tabler/icons-react";
import {
  selectedSandboxAtom,
  snapshotPickerAtom,
  viewAtom,
} from "../state/atoms";
import { useBranch } from "../hooks/useSandboxes";
import { useRollback, useSnapshots } from "../hooks/useSnapshots";
import { humanSize } from "../lib/format";
import { TextButton } from "./Buttons";

// Pick a snapshot to branch a new sandbox from, or to roll the selected sandbox
// back to. Opened by the `b` / `r` shortcuts and the command palette.
export function SnapshotPickerModal() {
  const [mode, setMode] = useAtom(snapshotPickerAtom);
  const selected = useAtomValue(selectedSandboxAtom);
  const setSelected = useSetAtom(selectedSandboxAtom);
  const setView = useSetAtom(viewAtom);
  const { data: snapshots, isLoading } = useSnapshots();
  const branch = useBranch();
  const rollback = useRollback();

  const close = () => setMode(null);

  const pick = (name: string) => {
    if (mode === "branch") {
      branch.mutate(
        { snapshotId: name },
        {
          onSuccess: (sb) => {
            setSelected(sb.id);
            setView("sandboxes");
            close();
          },
        },
      );
    } else if (mode === "rollback" && selected) {
      rollback.mutate({ id: selected, snapshotId: name }, { onSuccess: close });
    }
  };

  return (
    <Modal
      isOpen={mode !== null}
      onOpenChange={(o) => !o && close()}
      classNames={{ backdrop: "bg-black/60" }}
      scrollBehavior="inside"
    >
      <ModalContent>
        <ModalHeader className="flex-col items-start gap-0">
          <span>{mode === "branch" ? "Branch from snapshot" : "Rollback to snapshot"}</span>
          <span className="text-xs font-normal text-default-500">
            {mode === "rollback"
              ? selected
                ? `into ${selected}`
                : "select a sandbox first"
              : "creates a new sandbox"}
          </span>
        </ModalHeader>
        <ModalBody>
          {isLoading ? (
            <p className="py-6 text-center text-sm text-default-400">Loading…</p>
          ) : snapshots && snapshots.length > 0 ? (
            <ul className="flex flex-col gap-1">
              {snapshots.map((s) => (
                <li key={s.name}>
                  <button
                    disabled={mode === "rollback" && !selected}
                    onClick={() => pick(s.name)}
                    className="flex w-full items-center gap-3 border border-content3 bg-content1 px-3 py-2.5 text-left text-sm transition hover:border-primary/60 hover:bg-primary/10 disabled:cursor-not-allowed disabled:opacity-40"
                  >
                    <IconCamera size={16} className="text-neon-purple" />
                    <span className="min-w-0 flex-1 truncate text-foreground">
                      {s.name}
                    </span>
                    <span className="text-[11px] text-default-400">
                      {humanSize(s.size)}
                    </span>
                  </button>
                </li>
              ))}
            </ul>
          ) : (
            <p className="py-6 text-center text-sm text-default-400">
              No snapshots available.
            </p>
          )}
        </ModalBody>
        <ModalFooter>
          <TextButton onPress={close}>Cancel</TextButton>
        </ModalFooter>
      </ModalContent>
    </Modal>
  );
}
