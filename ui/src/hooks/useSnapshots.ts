import {
  useMutation,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";
import { getTransport } from "../transport";

const KEY = ["snapshots"] as const;

export function useSnapshots() {
  return useQuery({
    queryKey: KEY,
    queryFn: () => getTransport().listSnapshots(),
  });
}

export function useSnapshot() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (vars: { id: string; snapshotId?: string }) =>
      getTransport().snapshot(vars.id, vars.snapshotId),
    onSuccess: () => qc.invalidateQueries({ queryKey: KEY }),
  });
}

export function useDeleteSnapshot() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => getTransport().deleteSnapshot(id),
    onSuccess: () => qc.invalidateQueries({ queryKey: KEY }),
  });
}

export function useRollback() {
  return useMutation({
    mutationFn: (vars: { id: string; snapshotId: string }) =>
      getTransport().rollback(vars.id, vars.snapshotId),
  });
}
