import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { graphQLRequest } from "../graphql/client";
import {
  CREATE_SANDBOX,
  FREE_SANDBOX,
  SANDBOXES,
  type Sandbox,
} from "../graphql/queries";

const KEY = ["sandboxes"] as const;

export function useSandboxes() {
  return useQuery({
    queryKey: KEY,
    queryFn: async () =>
      (await graphQLRequest<{ sandboxes: Sandbox[] }>(SANDBOXES)).sandboxes,
    refetchInterval: 5000,
  });
}

export function useCreateSandbox() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: async (name?: string) =>
      (
        await graphQLRequest<{ createSandbox: Sandbox }>(CREATE_SANDBOX, {
          name: name || null,
        })
      ).createSandbox,
    onSuccess: () => qc.invalidateQueries({ queryKey: KEY }),
  });
}

export function useFreeSandbox() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: async (id: string) => {
      await graphQLRequest(FREE_SANDBOX, { id });
      return id;
    },
    onSuccess: () => qc.invalidateQueries({ queryKey: KEY }),
  });
}
