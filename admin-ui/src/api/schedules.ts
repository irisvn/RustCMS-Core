// The Schedules page (`/api/schedules`): each schedule, its runs, Run now.
import { useInfiniteQuery, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useApi, useSession } from "./context";
import { guardedWrite } from "./queries";
import type { Poll } from "./queue";
import type { ScheduleList, ScheduleRunPage, ScheduleRunResponse } from "./types";

/** Write scope of Run now (see `guardedWrite`). */
export const SCHEDULES = "system:schedules";
const enc = encodeURIComponent;

export function useSchedules(poll: Poll = false) {
  const api = useApi();
  return useQuery({ queryKey: ["schedules", "list"], queryFn: () => api.get<ScheduleList>("/schedules"), refetchInterval: poll });
}

/** Runs of `name`, newest first (polled while one page is loaded). */
export function useScheduleRuns(name: string | null, poll: Poll = false) {
  const api = useApi();
  return useInfiniteQuery({
    queryKey: ["schedules", "runs", name],
    queryFn: ({ pageParam }) =>
      api.get<ScheduleRunPage>(`/schedules/${enc(name ?? "")}/runs`, pageParam ? { cursor: pageParam } : {}),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (last) => last.meta.nextCursor,
    enabled: name !== null,
    refetchInterval: (query) => ((query.state.data?.pages.length ?? 0) > 1 ? false : poll),
  });
}

/** Runs a schedule now; refreshes the page, the queue and the audit log. */
export function useRunNow() {
  const api = useApi();
  const session = useSession();
  const client = useQueryClient();
  return useMutation({
    mutationFn: (name: string) =>
      guardedWrite(session, SCHEDULES, () => api.post<ScheduleRunResponse>(`/schedules/${enc(name)}/run`, {})),
    onSettled: () =>
      Promise.all([
        client.invalidateQueries({ queryKey: ["schedules"] }),
        client.invalidateQueries({ queryKey: ["queue"] }),
        client.invalidateQueries({ queryKey: ["audit"] }),
      ]),
  });
}
