// The Queue page (`/api/queue`): counts, jobs, and retry / forget / cancel.
import { useInfiniteQuery, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useApi, useSession } from "./context";
import { guardedWrite } from "./queries";
import type { QueueJobPage, QueueOverview } from "./types";

/** Write scope of queue actions (see `guardedWrite`). */
export const QUEUE = "system:queue";
const enc = encodeURIComponent;

/** Filters of the job list (all optional). */
export interface QueueFilters {
  status?: string;
  queue?: string;
  kind?: string;
}

const clean = (filters: Record<string, string | undefined>) =>
  Object.fromEntries(Object.entries(filters).filter((e): e is [string, string] => !!e[1]));

export function useQueueOverview() {
  const api = useApi();
  return useQuery({ queryKey: ["queue", "overview"], queryFn: () => api.get<QueueOverview>("/queue") });
}

export function useQueueJobs(filters: QueueFilters) {
  const api = useApi();
  return useInfiniteQuery({
    queryKey: ["queue", "jobs", clean({ ...filters })],
    queryFn: ({ pageParam }) => api.get<QueueJobPage>("/queue/jobs", clean({ ...filters, cursor: pageParam })),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (last) => last.meta.nextCursor,
  });
}

export type JobAction = "retry" | "forget" | "cancel";

/** Retry, forget or cancel one job; refreshes the page and the audit log. */
export function useJobAction() {
  const api = useApi();
  const session = useSession();
  const client = useQueryClient();
  return useMutation({
    mutationFn: ({ id, action }: { id: string; action: JobAction }) =>
      guardedWrite(session, QUEUE, () => api.post<void>(`/queue/jobs/${enc(id)}/${action}`, {})),
    onSettled: () =>
      Promise.all([
        client.invalidateQueries({ queryKey: ["queue"] }),
        client.invalidateQueries({ queryKey: ["audit"] }),
      ]),
  });
}
