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
import {
  activeTerminalTabAtom,
  selectedSandboxAtom,
  terminalTabsAtom,
} from "../state/atoms";

const KEY = ["sandboxes"] as const;

// A freed sandbox must vanish from the UI immediately: clear the selection and
// close its terminal tabs (unmounting a tab tears down its live session).
function clearSelectedIf(id: string) {
  const store = getDefaultStore();
  if (store.get(selectedSandboxAtom) === id) store.set(selectedSandboxAtom, null);
  store.set(terminalTabsAtom, (t) => t.filter((tab) => tab.sandboxId !== id));
  const remaining = store.get(terminalTabsAtom);
  const active = store.get(activeTerminalTabAtom);
  if (active && !remaining.some((tab) => tab.id === active)) {
    store.set(activeTerminalTabAtom, null);
  }
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
    mutationFn: async (vars?: {
      name?: string;
      repoUrl?: string;
      limits?: Limits;
      allowNetwork?: boolean;
      allowListen?: boolean;
    }) => {
      const t = getTransport();
      const sb = await t.createSandbox(vars?.name || null, vars?.repoUrl || null);
      // Flags and limits are applied via configure; create only takes a name.
      // Skip the extra round-trip when everything matches the daemon defaults
      // (network on, listen off, no limits).
      const hasLimits = !!vars?.limits && Object.keys(vars.limits).length > 0;
      const nonDefaultFlags =
        vars?.allowNetwork === false || vars?.allowListen === true;
      if (hasLimits || nonDefaultFlags) {
        return t.configure({
          id: sb.id,
          allowNetwork: vars?.allowNetwork,
          allowListen: vars?.allowListen,
          limits: vars?.limits,
        });
      }
      return sb;
    },
    onSuccess: () => qc.invalidateQueries({ queryKey: KEY }),
  });
}

// Guarantee a selected sandbox before opening a panel that needs one: keep the
// selection if set, else select the first existing sandbox, else create one.
export function useEnsureSandbox() {
  const { data: sandboxes } = useSandboxes();
  const create = useCreateSandbox();
  return async (): Promise<string | null> => {
    const store = getDefaultStore();
    const selected = store.get(selectedSandboxAtom);
    if (selected) return selected;
    const first = sandboxes?.[0];
    if (first) {
      store.set(selectedSandboxAtom, first.id);
      return first.id;
    }
    try {
      const sb = await create.mutateAsync({});
      store.set(selectedSandboxAtom, sb.id);
      return sb.id;
    } catch {
      return null;
    }
  };
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
