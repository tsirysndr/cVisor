import { useEffect, useRef } from "react";
import { useInfiniteQuery } from "@tanstack/react-query";
import { getTransport, type CacheEntry, type Sandbox } from "../transport";
import { useCacheDefaults } from "./useCaches";

const PAGE_SIZE = 50;

// Client-side infinite scroll: the daemon returns the full list, so we fetch it
// and page through it in-memory. `getNextPageParam` walks the offset forward
// until the list is exhausted.
function useInfinitePaged<T>(
  baseKey: readonly unknown[],
  fetchAll: () => Promise<T[]>,
) {
  const query = useInfiniteQuery({
    queryKey: [...baseKey, "infinite"],
    initialPageParam: 0,
    refetchInterval: 5000,
    queryFn: async ({ pageParam }) => {
      const all = await fetchAll();
      return {
        items: all.slice(pageParam, pageParam + PAGE_SIZE),
        nextOffset: pageParam + PAGE_SIZE,
        total: all.length,
      };
    },
    getNextPageParam: (last) =>
      last.nextOffset < last.total ? last.nextOffset : undefined,
  });

  const items = query.data?.pages.flatMap((p) => p.items) ?? [];
  const total = query.data?.pages[0]?.total ?? 0;
  return { ...query, items, total };
}

export function useInfiniteSandboxes() {
  return useInfinitePaged<Sandbox>(["sandboxes"], () =>
    getTransport().listSandboxes(),
  );
}

export function useInfiniteSnapshots() {
  return useInfinitePaged<CacheEntry>(["snapshots"], () =>
    getTransport().listSnapshots(),
  );
}

export function useInfiniteCaches() {
  const { backend } = useCacheDefaults();
  return useInfinitePaged<CacheEntry>(["caches", backend ?? ""], () =>
    getTransport().cacheList(backend),
  );
}

// Attach the returned ref to a bottom sentinel; it calls `fetchNextPage` when it
// scrolls into view and there are more pages.
export function useInfiniteScroll(opts: {
  hasNextPage: boolean;
  isFetchingNextPage: boolean;
  fetchNextPage: () => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const { hasNextPage, isFetchingNextPage, fetchNextPage } = opts;
  useEffect(() => {
    const el = ref.current;
    if (!el || !hasNextPage) return;
    const io = new IntersectionObserver((entries) => {
      if (entries[0]?.isIntersecting && hasNextPage && !isFetchingNextPage) {
        fetchNextPage();
      }
    });
    io.observe(el);
    return () => io.disconnect();
  }, [hasNextPage, isFetchingNextPage, fetchNextPage]);
  return ref;
}
