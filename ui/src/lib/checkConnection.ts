import { getTransport } from "../transport";
import type { CvisorConfig } from "../config";
import type { Health } from "../transport";

// Validate a candidate config through the active transport (GraphQL health in
// the browser, a gRPC health command on the desktop).
export function checkConnection(cfg: CvisorConfig): Promise<Health> {
  return getTransport().checkConnection(cfg);
}
