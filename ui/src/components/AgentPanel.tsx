import { useEffect } from "react";
import {
  Button,
  Dropdown,
  DropdownItem,
  DropdownMenu,
  DropdownTrigger,
  Select,
  SelectItem,
  Tooltip,
} from "@heroui/react";
import { useAtom, useAtomValue } from "jotai";
import {
  IconArrowsMaximize,
  IconArrowsMinimize,
  IconChevronDown,
  IconPlus,
  IconRobot,
  IconX,
} from "@tabler/icons-react";
import {
  activeAgentTabAtom,
  agentCliAtom,
  agentFullscreenAtom,
  agentPanelVisibleAtom,
  agentTabsAtom,
  selectedSandboxAtom,
  type AgentTab,
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

const newTabId = () =>
  `a${Date.now().toString(36)}${Math.random().toString(36).slice(2, 6)}`;

const cliLabel = (key: string) =>
  AGENT_CLIS.find((a) => a.key === key)?.label ?? key;

const cliCommand = (key: string) =>
  AGENT_CLIS.find((a) => a.key === key)?.command ?? key;

// Right-docked panel of AI agent sessions. Each session is an interactive PTY
// running an agent CLI, either inside the sandbox selected when it was opened
// or (desktop only) on the host machine, where the CLI can use local cvisor
// skills to drive sandboxes. Every session's terminal stays mounted (hidden
// when inactive) so an ongoing agent task survives tab switches and panel
// show/hide; only closing a session from the list kills it. The header select
// picks the CLI for new sessions; + starts one.
export function AgentPanel() {
  const [visible, setVisible] = useAtom(agentPanelVisibleAtom);
  const [cli, setCli] = useAtom(agentCliAtom);
  const selected = useAtomValue(selectedSandboxAtom);
  const [fullscreen, setFullscreen] = useAtom(agentFullscreenAtom);
  const [tabs, setTabs] = useAtom(agentTabsAtom);
  const [active, setActive] = useAtom(activeAgentTabAtom);
  const { data: sandboxes } = useSandboxes();

  // The sandbox/host target switch is temporarily hidden; the panel always
  // runs the agent on the host. Host PTYs only exist on the desktop.
  const host = isTauri();

  const openTab = (cliKey: string) => {
    const tab: AgentTab = {
      id: newTabId(),
      cli: cliKey,
      host,
      sandboxId: selected,
    };
    setTabs((t) => [...t, tab]);
    setActive(tab.id);
  };

  // Showing the panel with no sessions starts one with the chosen CLI.
  useEffect(() => {
    if (visible && tabs.length === 0) openTab(cli);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [visible]);

  const activeTab = tabs.find((t) => t.id === active) ?? tabs[0];

  // Sessions of the same CLI get an index suffix: Claude Code, Claude Code ·2 …
  const label = (tab: AgentTab) => {
    const peers = tabs.filter((t) => t.cli === tab.cli);
    const i = peers.findIndex((t) => t.id === tab.id);
    return i > 0 ? `${cliLabel(tab.cli)} ·${i + 1}` : cliLabel(tab.cli);
  };

  const target = (tab: AgentTab) =>
    tab.host
      ? "on this machine"
      : (sandboxes?.find((s) => s.id === tab.sandboxId)?.name ??
        "no sandbox selected");

  // Closing a session unmounts its terminal, which ends the PTY — the one
  // deliberate way to stop an agent. Last one gone hides the panel.
  const closeTab = (id: string) => {
    const next = tabs.filter((t) => t.id !== id);
    setTabs(next);
    if (next.length === 0) {
      setActive(null);
      setFullscreen(false);
      setVisible(false);
    } else if (activeTab?.id === id) {
      const i = tabs.findIndex((t) => t.id === id);
      setActive(next[Math.min(i, next.length - 1)].id);
    }
  };

  // Hidden ≠ closed: the panel stays mounted so live agent sessions survive
  // show/hide; only closing a session from the list ends it.
  const shown = visible && tabs.length > 0;

  return (
    <aside
      className={
        !shown
          ? "hidden"
          : fullscreen
            ? "absolute inset-0 z-30 flex flex-col bg-content1"
            : "flex w-[30rem] min-w-72 shrink-0 flex-col border-l border-content3 bg-content1"
      }
    >
      <div className="flex h-10 shrink-0 items-center gap-2 border-b border-content3 bg-content1 px-2">
        <IconRobot size={15} className="shrink-0 text-secondary" />
        <Select
          aria-label="Agent CLI for new sessions"
          variant="bordered"
          radius="sm"
          size="sm"
          className="w-36"
          selectedKeys={[cli]}
          onSelectionChange={(k) => {
            const v = [...k][0];
            if (typeof v === "string") setCli(v);
          }}
        >
          {AGENT_CLIS.map((a) => (
            <SelectItem key={a.key}>{a.label}</SelectItem>
          ))}
        </Select>
        <Tooltip content={`New ${cliLabel(cli)} session`}>
          <Button
            isIconOnly
            size="sm"
            variant="light"
            radius="sm"
            aria-label="New agent session"
            onPress={() => openTab(cli)}
          >
            <IconPlus size={15} />
          </Button>
        </Tooltip>
        <Dropdown placement="bottom-end">
          <DropdownTrigger>
            <Button
              size="sm"
              variant="bordered"
              radius="sm"
              className="min-w-0 flex-1 justify-between px-2"
              endContent={
                <IconChevronDown size={13} className="shrink-0 text-default-400" />
              }
            >
              <span className="truncate text-xs">
                {activeTab ? `${label(activeTab)} · ${target(activeTab)}` : "no session"}
              </span>
            </Button>
          </DropdownTrigger>
          <DropdownMenu
            aria-label="Active agent sessions"
            selectionMode="single"
            selectedKeys={activeTab ? [activeTab.id] : []}
            onAction={(key) => {
              if (typeof key === "string") setActive(key);
            }}
            items={tabs}
          >
            {(tab) => (
              <DropdownItem
                key={tab.id}
                description={target(tab)}
                endContent={
                  <span
                    role="button"
                    aria-label={`Close ${label(tab)} session`}
                    onClick={(e) => {
                      e.stopPropagation();
                      closeTab(tab.id);
                    }}
                    className="grid h-5 w-5 place-items-center rounded-sm text-default-400 transition hover:bg-content3 hover:text-danger"
                  >
                    <IconX size={12} />
                  </span>
                }
              >
                {label(tab)}
              </DropdownItem>
            )}
          </DropdownMenu>
        </Dropdown>
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
        <Tooltip content="Hide agent panel (sessions keep running)">
          <Button
            isIconOnly
            size="sm"
            variant="light"
            radius="sm"
            aria-label="Hide agent panel"
            onPress={() => {
              setFullscreen(false);
              setVisible(false);
            }}
          >
            <IconX size={15} />
          </Button>
        </Tooltip>
      </div>
      <div className="relative min-h-0 flex-1 overflow-hidden">
        {tabs.map((tab) => (
          <div
            key={tab.id}
            className={tab.id === activeTab?.id ? "h-full" : "hidden"}
          >
            {tab.host ? (
              <Terminal sandboxId="" command={cliCommand(tab.cli)} host />
            ) : tab.sandboxId ? (
              <Terminal
                sandboxId={tab.sandboxId}
                command={cliCommand(tab.cli)}
              />
            ) : (
              <div className="flex h-full flex-col items-center justify-center gap-2 p-6 text-center text-default-500">
                <IconRobot size={36} className="text-primary" stroke={1.5} />
                <p className="text-sm">
                  Select a sandbox, then open a new {cliLabel(tab.cli)} session.
                </p>
              </div>
            )}
          </div>
        ))}
      </div>
    </aside>
  );
}
