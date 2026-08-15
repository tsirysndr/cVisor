import { GraphQLClient, type RequestDocument, type Variables } from "graphql-request";
import { activeConfig, authHeaders } from "../config";

// Build a client per request so it always reflects the live config (URL/token
// can change at runtime via the Settings form). Construction is cheap.
export function graphQLRequest<T>(
  document: RequestDocument,
  variables?: Variables,
): Promise<T> {
  const client = new GraphQLClient(activeConfig().graphqlUrl, {
    headers: authHeaders(),
  });
  return client.request<T>(document, variables);
}
