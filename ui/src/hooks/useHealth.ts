import { useQuery } from "@tanstack/react-query";
import { graphQLRequest } from "../graphql/client";
import { HEALTH, type Health } from "../graphql/queries";

export function useHealth() {
  return useQuery({
    queryKey: ["health"],
    queryFn: async () =>
      (await graphQLRequest<{ health: Health }>(HEALTH)).health,
    refetchInterval: 10000,
    retry: false,
  });
}
