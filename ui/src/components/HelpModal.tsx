import {
  Modal,
  ModalBody,
  ModalContent,
  ModalHeader,
} from "@heroui/react";
import { useAtom } from "jotai";
import { helpOpenAtom } from "../state/atoms";

const isMac =
  typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform);
const MOD = isMac ? "⌘" : "Ctrl";

const GROUPS: { heading: string; items: { keys: string[]; label: string }[] }[] =
  [
    {
      heading: "General",
      items: [
        { keys: ["/"], label: "Open command palette" },
        { keys: [MOD, "K"], label: "Toggle command palette" },
        { keys: ["?"], label: "Keyboard shortcuts (this)" },
        { keys: ["Esc"], label: "Close dialog / palette" },
      ],
    },
    {
      heading: "Layout",
      items: [
        { keys: [MOD, "B"], label: "Show / hide sidebar" },
        { keys: [MOD, "J"], label: "Show / hide terminal panel" },
      ],
    },
    {
      heading: "Lists",
      items: [
        { keys: ["↑"], label: "Move selection up" },
        { keys: ["↓"], label: "Move selection down" },
        { keys: ["Enter"], label: "Activate selected row" },
      ],
    },
    {
      heading: "Sandbox",
      items: [
        { keys: ["c"], label: "New sandbox" },
        { keys: ["t"], label: "Open terminal (selected)" },
        { keys: ["s"], label: "Snapshot (selected)" },
        { keys: ["b"], label: "Branch from a snapshot" },
        { keys: ["r"], label: "Rollback (selected) to a snapshot" },
      ],
    },
  ];

export function HelpModal() {
  const [open, setOpen] = useAtom(helpOpenAtom);

  return (
    <Modal isOpen={open} onOpenChange={setOpen} backdrop="blur" size="lg">
      <ModalContent>
        <ModalHeader className="flex items-center gap-2">
          <span className="text-primary drop-shadow-[0_0_6px_rgba(255,42,109,0.8)]">
            ▣
          </span>
          Keyboard shortcuts
        </ModalHeader>
        <ModalBody className="pb-6">
          <div className="grid grid-cols-1 gap-6 sm:grid-cols-2">
            {GROUPS.map((g) => (
              <div key={g.heading}>
                <div className="mb-2 text-[11px] uppercase tracking-wide text-secondary">
                  {g.heading}
                </div>
                <ul className="flex flex-col gap-1.5">
                  {g.items.map((it) => (
                    <li
                      key={it.label}
                      className="flex items-center justify-between gap-3 text-sm text-default-600"
                    >
                      <span>{it.label}</span>
                      <span className="flex shrink-0 items-center gap-1">
                        {it.keys.map((k) => (
                          <kbd key={k} className="neon-key">
                            {k}
                          </kbd>
                        ))}
                      </span>
                    </li>
                  ))}
                </ul>
              </div>
            ))}
          </div>
        </ModalBody>
      </ModalContent>
    </Modal>
  );
}
