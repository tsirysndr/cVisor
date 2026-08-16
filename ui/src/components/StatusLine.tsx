import { useAtomValue } from "jotai";
import { useHealth } from "../hooks/useHealth";
import { useSandboxes } from "../hooks/useSandboxes";
import { isTauri } from "../transport";
import {
  cursorAtom,
  selectedSandboxAtom,
  terminalPanelVisibleAtom,
  viewAtom,
} from "../state/atoms";

// Neovim-style status line pinned to the bottom of the window: a colored
// "mode" block on the left (connection state), context segments, and a
// line-ruler on the right.
export function StatusLine() {
  const { data: health, isError } = useHealth();
  const { data: sandboxes } = useSandboxes();
  const view = useAtomValue(viewAtom);
  const cursor = useAtomValue(cursorAtom);
  const selected = useAtomValue(selectedSandboxAtom);
  const termVisible = useAtomValue(terminalPanelVisibleAtom);

  const connected = !!health?.ok && !isError;
  const sandbox = sandboxes?.find((s) => s.id === selected);
  const total = sandboxes?.length ?? 0;

  return (
    <footer className="flex h-6 shrink-0 select-none items-stretch overflow-hidden border-t border-content3 bg-content1 font-mono text-[11px] leading-6">
      {/* mode block */}
      <span
        className={`px-3 font-bold tracking-wider ${
          connected
            ? "bg-success text-success-foreground"
            : "bg-danger text-danger-foreground"
        }`}
      >
        {connected ? "ONLINE" : "OFFLINE"}
      </span>
      <span className="bg-content2 px-3 text-default-600">
        {isTauri() ? "grpc" : "graphql"}
        {health?.version ? ` · v${health.version}` : ""}
      </span>

      <span className="px-3 text-default-500">{view}</span>
      {sandbox && (
        <span className="truncate px-3 text-secondary">
          ▣ {sandbox.name}
          <span className="ml-2 text-default-400">{sandbox.id}</span>
        </span>
      )}

      <span className="flex-1" />

      {termVisible && selected && (
        <span className="px-3 text-default-500">term</span>
      )}
      <span className="bg-content2 px-3 text-default-600">
        {total} {total === 1 ? "sandbox" : "sandboxes"}
      </span>
      {/* ruler: cursor position in the active list, like nvim's line:col */}
      <span className="bg-primary px-3 font-bold text-primary-foreground">
        {total > 0
          ? `${Math.min(Math.max(cursor + 1, 1), total)}:${total}`
          : "–:–"}
      </span>
    </footer>
  );
}
