// Settings pages (`/api/settings/{group}`).
import { keepPreviousData, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useLocale } from "@/i18n";
import { useApi, useSession } from "./context";
import { guardedWrite } from "./queries";
import type { MailTestResult, SettingsResponse } from "./types";

/** Write scope of a group (see `guardedWrite`). */
export const settingsScope = (group: string) => `settings:${group}`;
const enc = encodeURIComponent;

export function useSettings(group: string) {
  const api = useApi();
  // Labels come translated.
  const locale = useLocale();
  return useQuery({
    queryKey: ["settings", group, locale],
    queryFn: () => api.get<SettingsResponse>(`/settings/${enc(group)}`),
    placeholderData: keepPreviousData,
  });
}

/** `POST settings/mail/test`: a test mail with the saved mail settings. */
export function useMailTest() {
  const api = useApi();
  return useMutation({
    mutationFn: (to: string) => api.request<MailTestResult>("POST", "/settings/mail/test", { to }),
  });
}

export function useSaveSettings(group: string) {
  const api = useApi();
  const session = useSession();
  const client = useQueryClient();
  return useMutation({
    mutationFn: (input: { version: string; data: Record<string, unknown> }) =>
      guardedWrite(session, settingsScope(group), () => api.request<SettingsResponse>("PUT", `/settings/${enc(group)}`, input)),
    onSuccess: () =>
      Promise.all([
        client.invalidateQueries({ queryKey: ["settings", group] }),
        client.invalidateQueries({ queryKey: ["audit"] }),
      ]),
  });
}
