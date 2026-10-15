// CSV imports: upload (`POST …/imports`, the file as the body), preview,
// run (once), and the import's state while its job writes the rows.
import { useMutation, useQuery } from "@tanstack/react-query";
import { useApi, useSession } from "./context";
import type { ImportPreview, ImportView } from "./types";

const enc = encodeURIComponent;

export function useUploadImport(slug: string) {
  const session = useSession();
  return useMutation({
    mutationFn: (file: File) =>
      session.api.request<ImportView>("POST", `/resources/${enc(slug)}/imports`, new Blob([file], { type: "text/csv" })),
  });
}

export function usePreviewImport() {
  const session = useSession();
  return useMutation({
    mutationFn: ({ id, mapping }: { id: string; mapping: Record<string, string> }) =>
      session.api.request<ImportPreview>("POST", `/imports/${enc(id)}/preview`, { mapping }),
  });
}

export function useRunImport() {
  const session = useSession();
  return useMutation({
    mutationFn: ({ id, mapping }: { id: string; mapping: Record<string, string> }) =>
      session.api.request<ImportView>("POST", `/imports/${enc(id)}/run`, { mapping }),
  });
}

/** An import's state, polled while it runs. */
export function useImport(id: string | null) {
  const api = useApi();
  return useQuery({
    queryKey: ["imports", id],
    queryFn: () => api.get<ImportView>(`/imports/${enc(id ?? "")}`),
    enabled: id !== null,
    refetchInterval: (query) => (query.state.data?.status === "running" ? 2_000 : false),
  });
}
