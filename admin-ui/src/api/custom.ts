// Custom pages (`/api/pages/{slug}`) and slots beside records (`/api/slots`).
import { useQuery } from "@tanstack/react-query";
import { useLocale } from "@/i18n";
import { useApi } from "./context";
import { encodeSubPath } from "@/lib/paths";
import type { CustomPageResponse, SlotsResponse } from "./types";

/**
 * Keys include the language (the server renders translated HTML) and the
 * session epoch (content of an earlier session is never shown again).
 */
/** The page `slug` at the canonical sub-path `path` (`""` for none). */
export function useCustomPage(slug: string, path = "", options: { enabled?: boolean } = {}) {
  const api = useApi();
  const locale = useLocale();
  return useQuery({
    queryKey: ["custom", "page", slug, locale, api.epoch, path],
    // Rebuilt segment by segment: never sent as one raw string.
    queryFn: () =>
      api.get<CustomPageResponse>(`/pages/${encodeURIComponent(slug)}${path ? `/${encodeSubPath(path)}` : ""}`),
    enabled: options.enabled ?? true,
  });
}

export function useSlots(resource: string, key: string) {
  const api = useApi();
  const locale = useLocale();
  return useQuery({
    queryKey: ["custom", "slots", resource, key, locale, api.epoch],
    queryFn: () => api.get<SlotsResponse>("/slots", { resource, key }),
  });
}
