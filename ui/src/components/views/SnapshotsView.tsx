import { useEffect, useRef } from "react";
import { Button, Tooltip } from "@heroui/react";
import { useAtomValue, useSetAtom } from "jotai";
import {
  IconArrowBackUp,
  IconCamera,
  IconGitBranch,
  IconTrash,
} from "@tabler/icons-react";
import {
  cursorAtom,
  selectedSandboxAtom,
  viewAtom,
} from "../../state/atoms";
import { useBranch } from "../../hooks/useSandboxes";
import { useDeleteSnapshot, useRollback } from "../../hooks/useSnapshots";
import {
  useInfiniteScroll,
  useInfiniteSnapshots,
} from "../../hooks/useInfinitePaged";
import { humanSize } from "../../lib/format";
import { LoadingMoreRow, SandboxListSkeleton } from "../Skeletons";
import { ViewShell } from "./ViewShell";

export function SnapshotsView() {
  const {
    items: snapshots,
    isLoading,
    hasNextPage,
    isFetchingNextPage,
    fetchNextPage,
  } = useInfiniteSnapshots();
  const cursor = useAtomValue(cursorAtom);
  const selected = useAtomValue(selectedSandboxAtom);
  const setSelected = useSetAtom(selectedSandboxAtom);
  const setView = useSetAtom(viewAtom);
  const branch = useBranch();
  const rollback = useRollback();
  const del = useDeleteSnapshot();

  const activeRef = useRef<HTMLLIElement>(null);
  useEffect(() => {
    activeRef.current?.scrollIntoView({ block: "nearest" });
  }, [cursor]);

  const sentinelRef = useInfiniteScroll({
    hasNextPage: !!hasNextPage,
    isFetchingNextPage,
    fetchNextPage,
  });

  return (
    <ViewShell title="Snapshots">
      {isLoading ? (
        <SandboxListSkeleton />
      ) : snapshots.length > 0 ? (
        <>
          <ul className="flex flex-col gap-1">
            {snapshots.map((s, i) => {
              const active = i === cursor;
              return (
                <li
                  key={s.name}
                  ref={i === cursor ? activeRef : undefined}
                  className={`group flex items-center gap-3 border px-3 py-2.5 text-sm transition ${
                    active
                      ? "border-primary/60 bg-primary/10 shadow-[0_0_10px_rgba(255,42,109,0.25)]"
                      : "border-content3 bg-content1 hover:border-secondary/40 hover:bg-content2"
                  }`}
                >
                  <IconCamera
                    size={16}
                    className="shrink-0 text-neon-purple"
                  />
                  <div className="min-w-0 flex-1">
                    <div className="truncate font-medium text-foreground">
                      {s.name}
                    </div>
                    <div className="text-[11px] text-default-400">
                      {humanSize(s.size)}
                    </div>
                  </div>
                  <div className="flex items-center gap-0.5">
                    <Tooltip content="Branch into new sandbox">
                      <Button
                        isIconOnly
                        size="sm"
                        variant="light"
                        radius="none"
                        aria-label="Branch"
                        isLoading={branch.isPending}
                        onPress={() =>
                          branch.mutate(
                            { snapshotId: s.name },
                            {
                              onSuccess: (sb) => {
                                setSelected(sb.id);
                                setView("sandboxes");
                              },
                            },
                          )
                        }
                      >
                        <IconGitBranch size={16} className="text-secondary" />
                      </Button>
                    </Tooltip>
                    <Tooltip
                      content={
                        selected
                          ? "Rollback selected sandbox to this"
                          : "Select a sandbox first"
                      }
                    >
                      <Button
                        isIconOnly
                        size="sm"
                        variant="light"
                        radius="none"
                        aria-label="Rollback"
                        isDisabled={!selected}
                        isLoading={rollback.isPending}
                        onPress={() =>
                          selected &&
                          rollback.mutate({ id: selected, snapshotId: s.name })
                        }
                      >
                        <IconArrowBackUp
                          size={16}
                          className="text-default-400"
                        />
                      </Button>
                    </Tooltip>
                    <Tooltip content="Delete snapshot">
                      <Button
                        isIconOnly
                        size="sm"
                        variant="light"
                        radius="none"
                        aria-label="Delete snapshot"
                        isLoading={del.isPending}
                        onPress={() => del.mutate(s.name)}
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
          <IconCamera size={40} className="text-neon-purple" stroke={1.5} />
          <p className="text-sm">
            No snapshots yet. Snapshot a sandbox from the Sandboxes view.
          </p>
        </div>
      )}
    </ViewShell>
  );
}
