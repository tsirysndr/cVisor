import { useMutation } from "@tanstack/react-query";
import { graphQLRequest } from "../graphql/client";
import { RUN, type RunResult } from "../graphql/queries";

export interface RunVars {
  id?: string | null;
  command: string;
  timeoutMs?: number;
}

export function useRun() {
  return useMutation({
    mutationFn: async (vars: RunVars) =>
      (await graphQLRequest<{ run: RunResult }>(RUN, vars)).run,
  });
}
