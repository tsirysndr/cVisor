import { createClient, type Client } from "graphql-ws";
import { activeConfig } from "../config";

// Lazily-built shared graphql-ws client. The token rides in the connection_init
// payload as `{ Authorization: "Bearer <token>" }`, per the daemon's ws auth.
let client: Client | null = null;

export function getWsClient(): Client {
  if (!client) {
    const cfg = activeConfig();
    client = createClient({
      url: cfg.wsUrl,
      lazy: true,
      connectionParams: {
        Authorization: cfg.token ? `Bearer ${cfg.token}` : "",
      },
    });
  }
  return client;
}

// Drop the client so the next subscription reconnects with the current config.
export function resetWsClient() {
  client?.dispose();
  client = null;
}
