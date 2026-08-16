import { Button, Select, SelectItem, Tooltip } from "@heroui/react";
import { useAtom, useAtomValue, useSetAtom } from "jotai";
import {
  IconArrowsMaximize,
  IconArrowsMinimize,
  IconRobot,
  IconX,
} from "@tabler/icons-react";
import {
  agentCliAtom,
  agentFullscreenAtom,
  agentPanelVisibleAtom,
  selectedSandboxAtom,
} from "../state/atoms";
import { useSandboxes } from "../hooks/useSandboxes";
import { isTauri } from "../transport";
import { Terminal } from "./Terminal";

// The AI agent CLIs pre-installed in the cVisor images; `command` is what runs
// on the panel's PTY.
export const AGENT_CLIS = [
  { key: "claude", label: "Claude Code", command: "claude" },
  { key: "codex", label: "Codex", command: "codex" },
  { key: "gemini", label: "Gemini CLI", command: "gemini" },
  { key: "opencode", label: "opencode", command: "opencode" },
  { key: "kiro", label: "Kiro", command: "kiro-cli chat" },
  { key: "kilo", label: "Kilo Code", command: "kilo" },
  { key: "amp", label: "Amp", command: "amp" },
];

// Right-docked panel: an interactive PTY running the chosen AI agent CLI,
// either inside the selected sandbox or (desktop only) on the host machine,
// where the CLI can use local cvisor skills to drive sandboxes. Switching
// agent, target, or sandbox restarts the PTY (keyed remount).
export function AgentPanel() {
  const setVisible = useSetAtom(agentPanelVisibleAtom);
  const [cli, setCli] = useAtom(agentCliAtom);
  const selected = useAtomValue(selectedSandboxAtom);
  const [fullscreen, setFullscreen] = useAtom(agentFullscreenAtom);
  const { data: sandboxes } = useSandboxes();

  const agent = AGENT_CLIS.find((a) => a.key === cli) ?? AGENT_CLIS[0];
  const sandbox = sandboxes?.find((s) => s.id === selected);
  // The sandbox/host target switch is temporarily hidden; the panel always runs
  // the agent on the host. Host PTYs only exist on the desktop.
  const host = isTauri();

  return (
    <aside
      className={
        fullscreen
          ? "absolute inset-0 z-30 flex flex-col bg-content1"
          : "flex w-[30rem] min-w-72 shrink-0 flex-col border-l border-content3 bg-content1"
      }
    >
      <div className="flex h-10 shrink-0 items-center gap-2 border-b border-content3 bg-content1 px-2">
        <IconRobot size={15} className="shrink-0 text-secondary" />
        <Select
          aria-label="AI agent"
          variant="bordered"
          radius="sm"
          size="sm"
          className="w-36"
          selectedKeys={[agent.key]}
          onSelectionChange={(k) => {
            const v = [...k][0];
            if (typeof v === "string") setCli(v);
          }}
        >
          {AGENT_CLIS.map((a) => (
            <SelectItem key={a.key}>{a.label}</SelectItem>
          ))}
        </Select>
        <span className="min-w-0 flex-1 truncate text-right text-[11px] text-default-400">
          {host
            ? "on this machine"
            : sandbox
              ? sandbox.name
              : "no sandbox selected"}
        </span>
        <Tooltip content={fullscreen ? "Exit fullscreen" : "Fullscreen"}>
          <Button
            isIconOnly
            size="sm"
            variant="light"
            radius="sm"
            aria-label="Toggle agent fullscreen"
            onPress={() => setFullscreen((f) => !f)}
          >
            {fullscreen ? (
              <IconArrowsMinimize size={15} />
            ) : (
              <IconArrowsMaximize size={15} />
            )}
          </Button>
        </Tooltip>
        <Tooltip content="Close agent panel">
          <Button
            isIconOnly
            size="sm"
            variant="light"
            radius="sm"
            aria-label="Close agent panel"
            onPress={() => {
            setFullscreen(false);
            setVisible(false);
          }}
          >
            <IconX size={15} />
          </Button>
        </Tooltip>
      </div>
      <div className="min-h-0 flex-1 overflow-hidden">
        {host ? (
          <Terminal
            key={`host:${agent.key}`}
            sandboxId=""
            command={agent.command}
            host
          />
        ) : selected ? (
          <Terminal
            key={`${selected}:${agent.key}`}
            sandboxId={selected}
            command={agent.command}
          />
        ) : (
          <div className="flex h-full flex-col items-center justify-center gap-2 p-6 text-center text-default-500">
            <IconRobot size={36} className="text-primary" stroke={1.5} />
            <p className="text-sm">Select a sandbox to start {agent.label}.</p>
          </div>
        )}
      </div>
    </aside>
  );
}
