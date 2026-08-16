import {
  useMutation,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";
import { useAtomValue } from "jotai";
import { getTransport } from "../transport";
import { cacheBackendAtom, cacheFormatAtom } from "../state/atoms";

const KEY = ["caches"] as const;

// Explicit args win; otherwise the settings-configured defaults apply
// (cacheBackendAtom / cacheFormatAtom, empty = daemon defaults).
export function useCacheDefaults() {
  const backend = useAtomValue(cacheBackendAtom);
  const format = useAtomValue(cacheFormatAtom);
  return {
    backend: backend.trim() || undefined,
    format: format.trim() || undefined,
  };
}

export function useCaches(backend?: string) {
  const defaults = useCacheDefaults();
  const b = backend ?? defaults.backend;
  return useQuery({
    queryKey: [...KEY, b ?? ""],
    queryFn: () => getTransport().cacheList(b),
  });
}

export function useCacheRemove() {
  const qc = useQueryClient();
  const defaults = useCacheDefaults();
  return useMutation({
    mutationFn: (vars: { key: string; backend?: string; format?: string }) =>
      getTransport().cacheRemove(
        vars.key,
        vars.backend ?? defaults.backend,
        vars.format ?? defaults.format,
      ),
    onSuccess: () => qc.invalidateQueries({ queryKey: KEY }),
  });
}

export function useCacheClear() {
  const qc = useQueryClient();
  const defaults = useCacheDefaults();
  return useMutation({
    mutationFn: (backend?: string) =>
      getTransport().cacheClear(backend ?? defaults.backend),
    onSuccess: () => qc.invalidateQueries({ queryKey: KEY }),
  });
}
