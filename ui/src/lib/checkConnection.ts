import { GraphQLClient } from "graphql-request";
import { HEALTH, type Health } from "../graphql/queries";

// Validate a candidate config by firing the health query with its token.
export async function checkConnection(cfg: {
  graphqlUrl: string;
  token: string;
}): Promise<Health> {
  const client = new GraphQLClient(cfg.graphqlUrl, {
    headers: cfg.token ? { Authorization: `Bearer ${cfg.token}` } : {},
  });
  const d = await client.request<{ health: Health }>(HEALTH);
  if (!d.health?.ok) throw new Error("daemon reported not-ok health");
  return d.health;
}
