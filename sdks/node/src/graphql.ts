// Optional GraphQL client for the cVisor daemon's HTTP API. Pure `fetch` +
// `JSON` — no native library — so it runs on any platform (macOS included),
// unlike the FFI-backed `Sandbox`.

/** A GraphQL `{ data, errors }` envelope as returned by the daemon. */
interface GraphQLResponse<T = Record<string, unknown>> {
  data?: T;
  errors?: Array<{ message: string }>;
}

/**
 * A thin client for the cVisor daemon's GraphQL endpoint.
 *
 *   const gql = new GraphQLClient("http://127.0.0.1:8080/graphql", token);
 *   const { health } = await gql.query(`{ health { version ok } }`);
 */
export class GraphQLClient {
  constructor(
    private readonly url: string,
    private readonly token: string,
  ) {}

  /** Run a GraphQL query document; resolves with the `data` object. */
  query<T = Record<string, unknown>>(
    document: string,
    variables?: Record<string, unknown>,
  ): Promise<T> {
    return this.execute<T>(document, variables);
  }

  /** Run a GraphQL mutation document; resolves with the `data` object. */
  mutate<T = Record<string, unknown>>(
    document: string,
    variables?: Record<string, unknown>,
  ): Promise<T> {
    return this.execute<T>(document, variables);
  }

  private async execute<T>(
    document: string,
    variables?: Record<string, unknown>,
  ): Promise<T> {
    const res = await fetch(this.url, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        Authorization: `Bearer ${this.token}`,
      },
      body: JSON.stringify({ query: document, variables: variables ?? {} }),
    });
    if (!res.ok) {
      throw new Error(`GraphQL request failed: HTTP ${res.status} ${res.statusText}`);
    }
    const body = (await res.json()) as GraphQLResponse<T>;
    if (body.errors && body.errors.length > 0) {
      throw new Error(body.errors.map((e) => e.message).join("; "));
    }
    return body.data as T;
  }
}
