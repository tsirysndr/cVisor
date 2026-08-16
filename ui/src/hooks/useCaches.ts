import {
  useMutation,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";
import { getTransport } from "../transport";

const KEY = ["caches"] as const;

export function useCaches(backend?: string) {
  return useQuery({
    queryKey: [...KEY, backend ?? ""],
    queryFn: () => getTransport().cacheList(backend),
  });
}

export function useCacheRemove() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (vars: { key: string; backend?: string; format?: string }) =>
      getTransport().cacheRemove(vars.key, vars.backend, vars.format),
    onSuccess: () => qc.invalidateQueries({ queryKey: KEY }),
  });
}

export function useCacheClear() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (backend?: string) => getTransport().cacheClear(backend),
    onSuccess: () => qc.invalidateQueries({ queryKey: KEY }),
  });
}
