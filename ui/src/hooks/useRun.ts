import { useMutation } from "@tanstack/react-query";
import { getTransport, type RunVars } from "../transport";

export type { RunVars };

export function useRun() {
  return useMutation({
    mutationFn: (vars: RunVars) => getTransport().run(vars),
  });
}
