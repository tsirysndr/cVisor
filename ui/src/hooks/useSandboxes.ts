import {
  useMutation,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";
import { getDefaultStore } from "jotai";
import {
  getTransport,
  type ConfigureInput,
  type Limits,
  type Sandbox,
} from "../transport";
import { selectedSandboxAtom } from "../state/atoms";

const KEY = ["sandboxes"] as const;

// Clear the selected-sandbox atom when the sandbox it points at goes away.
function clearSelectedIf(id: string) {
  const store = getDefaultStore();
  if (store.get(selectedSandboxAtom) === id) store.set(selectedSandboxAtom, null);
}

export function useSandboxes() {
  return useQuery({
    queryKey: KEY,
    queryFn: () => getTransport().listSandboxes(),
    refetchInterval: 5000,
  });
}

export function useCreateSandbox() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: async (vars?: { name?: string; limits?: Limits }) => {
      const t = getTransport();
      const sb = await t.createSandbox(vars?.name || null);
      // Limits are applied via configure; create itself only takes a name.
      if (vars?.limits && Object.keys(vars.limits).length > 0) {
        return t.configure({ id: sb.id, limits: vars.limits });
      }
      return sb;
    },
    onSuccess: () => qc.invalidateQueries({ queryKey: KEY }),
  });
}

export function useConfigure() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (input: ConfigureInput) => getTransport().configure(input),
    onSuccess: () => qc.invalidateQueries({ queryKey: KEY }),
  });
}

export function useFreeSandbox() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: async (id: string) => {
      await getTransport().freeSandbox(id);
      return id;
    },
    onSuccess: (id) => {
      clearSelectedIf(id);
      qc.invalidateQueries({ queryKey: KEY });
    },
  });
}

export function useBranch() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (vars: { snapshotId: string; name?: string }): Promise<Sandbox> =>
      getTransport().branch(vars.snapshotId, vars.name),
    onSuccess: () => qc.invalidateQueries({ queryKey: KEY }),
  });
}

export function useFork() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (vars: { id: string; name?: string }): Promise<Sandbox> =>
      getTransport().fork(vars.id, vars.name),
    onSuccess: () => qc.invalidateQueries({ queryKey: KEY }),
  });
}
