import { useQuery } from "@tanstack/react-query";
import { getTransport } from "../transport";

export function useHealth() {
  return useQuery({
    queryKey: ["health"],
    queryFn: () => getTransport().health(),
    refetchInterval: 10000,
    retry: false,
  });
}
