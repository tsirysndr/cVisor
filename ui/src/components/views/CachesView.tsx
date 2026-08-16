import { useEffect, useRef } from "react";
import { Button, Tooltip } from "@heroui/react";
import { useAtomValue } from "jotai";
import { IconDatabase, IconTrash } from "@tabler/icons-react";
import { cursorAtom } from "../../state/atoms";
import { useCacheClear, useCacheRemove } from "../../hooks/useCaches";
import {
  useInfiniteCaches,
  useInfiniteScroll,
} from "../../hooks/useInfinitePaged";
import { humanSize } from "../../lib/format";
import { PrimaryButton } from "../Buttons";
import { LoadingMoreRow, SandboxListSkeleton } from "../Skeletons";
import { ViewShell } from "./ViewShell";

export function CachesView() {
  const {
    items: caches,
    isLoading,
    hasNextPage,
    isFetchingNextPage,
    fetchNextPage,
  } = useInfiniteCaches();
  const cursor = useAtomValue(cursorAtom);
  const remove = useCacheRemove();
  const clear = useCacheClear();

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
    <ViewShell
      title="Caches"
      action={
        caches.length > 0 ? (
          <PrimaryButton
            size="sm"
            startContent={<IconTrash size={16} />}
            isLoading={clear.isPending}
            onPress={() => clear.mutate(undefined)}
          >
            Clear all
          </PrimaryButton>
        ) : undefined
      }
    >
      {isLoading ? (
        <SandboxListSkeleton />
      ) : caches.length > 0 ? (
        <>
          <ul className="flex flex-col gap-1">
            {caches.map((c, i) => {
              const active = i === cursor;
              return (
                <li
                  key={c.name}
                  ref={i === cursor ? activeRef : undefined}
                  className={`group flex items-center gap-3 border px-3 py-2.5 text-sm transition ${
                    active
                      ? "border-primary/60 bg-primary/10 shadow-[0_0_10px_rgba(255,42,109,0.25)]"
                      : "border-content3 bg-content1 hover:border-secondary/40 hover:bg-content2"
                  }`}
                >
                  <IconDatabase size={16} className="shrink-0 text-secondary" />
                  <div className="min-w-0 flex-1">
                    <div className="truncate font-medium text-foreground">
                      {c.name}
                    </div>
                    <div className="text-[11px] text-default-400">
                      {humanSize(c.size)}
                    </div>
                  </div>
                  <Tooltip content="Remove cache entry">
                    <Button
                      isIconOnly
                      size="sm"
                      variant="light"
                      radius="sm"
                      aria-label="Remove cache entry"
                      isLoading={remove.isPending}
                      onPress={() => remove.mutate({ key: c.name })}
                    >
                      <IconTrash size={16} className="text-danger" />
                    </Button>
                  </Tooltip>
                </li>
              );
            })}
          </ul>
          <div ref={sentinelRef} className="h-1" />
          {isFetchingNextPage && <LoadingMoreRow />}
        </>
      ) : (
        <div className="flex flex-col items-center justify-center gap-3 py-16 text-default-500">
          <IconDatabase size={40} className="text-secondary" stroke={1.5} />
          <p className="text-sm">No cache entries.</p>
        </div>
      )}
    </ViewShell>
  );
}
