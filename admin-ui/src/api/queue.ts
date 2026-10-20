// The Queue page (`/api/queue`): counts, jobs, and retry / forget / cancel.
import { useInfiniteQuery, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useApi, useSession } from "./context";
import { guardedWrite } from "./queries";
import type { JobRunPage, QueueJobDetail, QueueJobPage, QueueOverview } from "./types";

/** Write scope of queue actions (see `guardedWrite`). */
export const QUEUE = "system:queue";
const enc = encodeURIComponent;

/** Filters of the job list (all optional). */
export interface QueueFilters {
  status?: string;
  queue?: string;
  kind?: string;
  /** RFC 3339 bounds (on `created_at` for jobs, `finished_at` for the history). */
  since?: string;
  until?: string;
}

const clean = (filters: Record<string, string | undefined>) =>
  Object.fromEntries(Object.entries(filters).filter((e): e is [string, string] => !!e[1]));

/** Polling: `refetchInterval` while auto refresh may run (see `lib/refresh`). */
export type Poll = number | false;

export function useQueueOverview(poll: Poll = false) {
  const api = useApi();
  return useQuery({ queryKey: ["queue", "overview"], queryFn: () => api.get<QueueOverview>("/queue"), refetchInterval: poll });
}

/**
 * Jobs, newest first. Polling runs only while one page is loaded: an infinite
 * refetch would reload every page, and rows would jump under "load more".
 */
export function useQueueJobs(filters: QueueFilters, poll: Poll = false) {
  const api = useApi();
  return useInfiniteQuery({
    queryKey: ["queue", "jobs", clean({ ...filters })],
    queryFn: ({ pageParam }) => api.get<QueueJobPage>("/queue/jobs", clean({ ...filters, cursor: pageParam })),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (last) => last.meta.nextCursor,
    refetchInterval: (query) => ((query.state.data?.pages.length ?? 0) > 1 ? false : poll),
  });
}

/** Finished jobs, newest first (polled like `useQueueJobs`). */
export function useQueueHistory(filters: QueueFilters, poll: Poll = false) {
  const api = useApi();
  return useInfiniteQuery({
    queryKey: ["queue", "history", clean({ ...filters })],
    queryFn: ({ pageParam }) => api.get<JobRunPage>("/queue/history", clean({ ...filters, cursor: pageParam })),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (last) => last.meta.nextCursor,
    refetchInterval: (query) => ((query.state.data?.pages.length ?? 0) > 1 ? false : poll),
  });
}

/** One job (while it exists) and its finished runs. */
export function useQueueJob(id: string | null) {
  const api = useApi();
  return useQuery({
    queryKey: ["queue", "job", id],
    queryFn: () => api.get<QueueJobDetail>(`/queue/jobs/${enc(id ?? "")}`),
    enabled: id !== null,
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
