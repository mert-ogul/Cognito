import { getAppHealth, type AppHealth } from "@/lib/tauri";
import { useQuery } from "@tanstack/react-query";

export function useAppHealth() {
  return useQuery<AppHealth>({
    queryKey: ["health"],
    queryFn: getAppHealth,
    refetchInterval: (q) => (q.state.data?.warmUpPending ? 2000 : 10000),
  });
}
