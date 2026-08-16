import { useEffect, useRef, useState } from "react";
import { Button, Input, Tooltip } from "@heroui/react";
import { useAtom, useAtomValue, useSetAtom } from "jotai";
import {
  IconAdjustmentsHorizontal,
  IconCamera,
  IconFilter,
  IconGitFork,
  IconPlus,
  IconTerminal2,
  IconTrash,
} from "@tabler/icons-react";
import {
  cursorAtom,
  runModalOpenAtom,
  sandboxFilterFocusAtom,
  sandboxSettingsAtom,
  selectedSandboxAtom,
  terminalPanelVisibleAtom,
} from "../../state/atoms";
import { useFork, useFreeSandbox } from "../../hooks/useSandboxes";
import { useSnapshot } from "../../hooks/useSnapshots";
import {
  useInfiniteSandboxes,
  useInfiniteScroll,
} from "../../hooks/useInfinitePaged";
import { LoadingMoreRow, SandboxListSkeleton } from "../Skeletons";
import { ViewShell } from "./ViewShell";
import { plainTextField } from "../../lib/inputProps";

export function SandboxesView() {
  const {
    items: sandboxes,
    isLoading,
    hasNextPage,
    isFetchingNextPage,
    fetchNextPage,
  } = useInfiniteSandboxes();
  const [cursor, setCursor] = useAtom(cursorAtom);
  const [selected, setSelected] = useAtom(selectedSandboxAtom);
  const setPanel = useSetAtom(terminalPanelVisibleAtom);
  const setRunOpen = useSetAtom(runModalOpenAtom);
  const setSandboxSettings = useSetAtom(sandboxSettingsAtom);
  const [filter, setFilter] = useState("");
  const free = useFreeSandbox();
  const fork = useFork();
  const snapshot = useSnapshot();

  const activeRef = useRef<HTMLLIElement>(null);
  useEffect(() => {
    activeRef.current?.scrollIntoView({ block: "nearest" });
  }, [cursor]);

  // `f` shortcut: focus the quick filter.
  const filterRef = useRef<HTMLInputElement>(null);
  const filterFocus = useAtomValue(sandboxFilterFocusAtom);
  useEffect(() => {
    if (filterFocus > 0) filterRef.current?.focus();
  }, [filterFocus]);

  const sentinelRef = useInfiniteScroll({
    hasNextPage: !!hasNextPage,
    isFetchingNextPage,
    fetchNextPage,
  });

  // Map back to the unfiltered index so the keyboard cursor stays consistent.
  const cursorTo = (id: string) => {
    const idx = sandboxes.findIndex((s) => s.id === id);
    if (idx >= 0) setCursor(idx);
  };

  const open = (id: string) => {
    cursorTo(id);
    setSelected(id);
    setPanel(true);
  };

  const q = filter.trim().toLowerCase();
  const visible = q
    ? sandboxes.filter(
        (sb) =>
          sb.name.toLowerCase().includes(q) || sb.id.toLowerCase().includes(q),
      )
    : sandboxes;

  return (
    <ViewShell
      title="Sandboxes"
      action={
        <Input {...plainTextField}
          ref={filterRef}
          size="sm"
          variant="bordered"
          radius="sm"
          className="w-56"
          aria-label="Filter sandboxes"
          placeholder="Filter sandboxes… (f)"
          startContent={<IconFilter size={14} className="text-default-400" />}
          isClearable
          value={filter}
          onValueChange={setFilter}
        />
      }
    >
      {isLoading ? (
        <SandboxListSkeleton />
      ) : visible.length > 0 ? (
        <>
          <ul className="flex flex-col gap-1">
            {visible.map((sb, i) => {
              // The keyboard cursor indexes the unfiltered list, so only use it
              // for highlighting when no filter is applied.
              const active = (!q && i === cursor) || sb.id === selected;
              return (
                <li
                  key={sb.id}
                  ref={!q && i === cursor ? activeRef : undefined}
                  onClick={() => open(sb.id)}
                  className={`group flex cursor-pointer items-center gap-3 border px-3 py-2.5 text-sm transition ${
                    active
                      ? "border-primary/60 bg-primary/10 shadow-[0_0_10px_rgba(255,42,109,0.25)]"
                      : "border-content3 bg-content1 hover:border-secondary/40 hover:bg-content2"
                  }`}
                >
                  <span
                    className={`h-1.5 w-1.5 shrink-0 rounded-full ${
                      active
                        ? "bg-primary shadow-[0_0_6px_#FF2A6D]"
                        : "bg-default-400"
                    }`}
                  />
                  <div className="min-w-0 flex-1">
                    <div className="truncate font-medium text-foreground">
                      {sb.name}
                    </div>
                    <div className="truncate text-[11px] text-default-400">
                      {sb.id}
                      {sb.allowNetwork && (
                        <span className="ml-2 text-secondary">net</span>
                      )}
                      {sb.allowListen && (
                        <span className="ml-2 text-neon-purple">listen</span>
                      )}
                    </div>
                  </div>
                  <div
                    className="flex items-center gap-0.5 opacity-0 transition group-hover:opacity-100"
                    onClick={(e) => e.stopPropagation()}
                  >
                    <Tooltip content="Open terminal">
                      <Button
                        isIconOnly
                        size="sm"
                        variant="light"
                        radius="sm"
                        aria-label="Open terminal"
                        onPress={() => open(sb.id)}
                      >
                        <IconTerminal2 size={16} className="text-secondary" />
                      </Button>
                    </Tooltip>
                    <Tooltip content="Run command">
                      <Button
                        isIconOnly
                        size="sm"
                        variant="light"
                        radius="sm"
                        aria-label="Run command"
                        onPress={() => {
                          setSelected(sb.id);
                          cursorTo(sb.id);
                          setRunOpen(true);
                        }}
                      >
                        <IconPlus size={16} className="text-default-400" />
                      </Button>
                    </Tooltip>
                    <Tooltip content="Settings">
                      <Button
                        isIconOnly
                        size="sm"
                        variant="light"
                        radius="sm"
                        aria-label="Sandbox settings"
                        onPress={() => setSandboxSettings(sb.id)}
                      >
                        <IconAdjustmentsHorizontal
                          size={16}
                          className="text-default-400"
                        />
                      </Button>
                    </Tooltip>
                    <Tooltip content="Snapshot">
                      <Button
                        isIconOnly
                        size="sm"
                        variant="light"
                        radius="sm"
                        aria-label="Snapshot"
                        isLoading={snapshot.isPending}
                        onPress={() => snapshot.mutate({ id: sb.id })}
                      >
                        <IconCamera size={16} className="text-default-400" />
                      </Button>
                    </Tooltip>
                    <Tooltip content="Fork">
                      <Button
                        isIconOnly
                        size="sm"
                        variant="light"
                        radius="sm"
                        aria-label="Fork"
                        isLoading={fork.isPending}
                        onPress={() => fork.mutate({ id: sb.id })}
                      >
                        <IconGitFork size={16} className="text-default-400" />
                      </Button>
                    </Tooltip>
                    <Tooltip content="Free sandbox">
                      <Button
                        isIconOnly
                        size="sm"
                        variant="light"
                        radius="sm"
                        aria-label="Free sandbox"
                        onPress={() => free.mutate(sb.id)}
                      >
                        <IconTrash size={16} className="text-danger" />
                      </Button>
                    </Tooltip>
                  </div>
                </li>
              );
            })}
          </ul>
          <div ref={sentinelRef} className="h-1" />
          {isFetchingNextPage && <LoadingMoreRow />}
        </>
      ) : (
        <div className="flex flex-col items-center justify-center gap-3 py-16 text-default-500">
          <IconTerminal2 size={40} className="text-primary" stroke={1.5} />
          <p className="text-sm">
            {q
              ? `No sandboxes match “${filter.trim()}”.`
              : "No sandboxes yet. Create one to get started."}
          </p>
        </div>
      )}
    </ViewShell>
  );
}
