// CSV exports of resource lists: at once (`GET …/export`) or in the
// background (`POST …/exports`, then `GET exports`, `GET exports/{id}/file`).
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useApi, useSession } from "./context";
import type { ExportView } from "./types";

const enc = encodeURIComponent;

/** Saves `blob` as a file named `name` (a temporary link, revoked at once). */
export function save(blob: Blob, name: string) {
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = name;
  document.body.append(link);
  link.click();
  link.remove();
  setTimeout(() => URL.revokeObjectURL(url), 0);
}

const fileName = (slug: string) => `${slug}-${new Date().toISOString().slice(0, 10)}.csv`;

/** The list as shown now (`query`: the list's query string, without the page). */
export function useExportNow(slug: string) {
  const api = useApi();
  return useMutation({
    mutationFn: async (query: URLSearchParams) => save(await api.download(`/resources/${enc(slug)}/export?${query}`), fileName(slug)),
  });
}

/** Starts a background export of the list. */
export function useExportLater(slug: string) {
  const session = useSession();
  const client = useQueryClient();
  return useMutation({
    mutationFn: (query: URLSearchParams) => session.api.request<ExportView>("POST", `/resources/${enc(slug)}/exports?${query}`, {}),
    onSettled: () => client.invalidateQueries({ queryKey: ["exports"] }),
  });
}

/** The user's background exports of `slug`, polled while one is not finished. */
export function useExports(slug: string) {
  const api = useApi();
  return useQuery({
    queryKey: ["exports"],
    queryFn: () => api.get<ExportView[]>("/exports"),
    select: (all) => all.filter((e) => e.resource === slug),
    refetchInterval: (query) =>
      (query.state.data ?? []).some((e) => e.resource === slug && (e.status === "pending" || e.status === "running")) ? 2_000 : false,
  });
}

export function useDownloadExport() {
  const api = useApi();
  return useMutation({
    mutationFn: async (item: ExportView) => save(await api.download(`/exports/${enc(item.id)}/file`), fileName(item.resource)),
  });
}
